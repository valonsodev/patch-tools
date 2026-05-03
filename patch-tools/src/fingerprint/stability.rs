use std::cmp::Ordering;

use crate::types::{
    InspectMethodResponse, InstructionFeature, MethodFingerprintDto, ScoredInstruction,
    instruction_feature,
};

use super::index::FingerprintIndex;
use anyhow::Result;

// =============================================================================
// Stability inspection
// =============================================================================

/// Compute stability scores for every instruction feature in a method.
pub fn inspect_stability(
    index: &FingerprintIndex,
    target_method_id: &str,
) -> Result<InspectMethodResponse> {
    let source = index
        .method_by_id(target_method_id)
        .map(|(_, method)| method)
        .ok_or_else(|| anyhow::anyhow!("Method not found in index: {target_method_id}"))?;

    let signature = &source.signature;
    let mut total_score = 0i32;

    total_score += minification_score(&signature.return_type);
    for p in &signature.parameters {
        total_score += minification_score(p);
    }

    let mut scored: Vec<ScoredInstruction> = source
        .instructions
        .iter()
        .map(|instr| {
            let score = instruction_stability(instr);
            total_score += score;
            ScoredInstruction {
                instruction: Some(instr.clone()),
                stability_score: score,
                label: instruction_label(instr),
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        a.stability_score
            .cmp(&b.stability_score)
            .then(a.label.cmp(&b.label))
    });

    Ok(InspectMethodResponse {
        method_id: target_method_id.to_string(),
        signature: Some(signature.clone()),
        scored_instructions: scored,
        total_stability_score: total_score,
    })
}

/// Produce a compact human-readable label for an instruction feature.
fn instruction_label(instr: &InstructionFeature) -> String {
    match instr.kind_ref() {
        instruction_feature::Kind::StringConst(kind) => {
            let s = if kind.string.len() > 40 {
                format!("{}…", &kind.string[..40])
            } else {
                kind.string.clone()
            };
            format!("string(\"{s}\")")
        }
        instruction_feature::Kind::Literal(kind) => format!("literal({})", kind.value),
        instruction_feature::Kind::MethodCall(kind) => {
            let dc = if kind.use_this_defining_class {
                "this"
            } else {
                kind.defining_class.as_str()
            };
            format!("methodCall({dc}->{}", kind.name)
        }
        instruction_feature::Kind::FieldAccess(kind) => {
            let dc = if kind.use_this_defining_class {
                "this"
            } else {
                kind.defining_class.as_str()
            };
            format!("fieldAccess({dc}.{})", kind.name)
        }
        instruction_feature::Kind::NewInstance(kind) => {
            format!("newInstance({})", kind.instance_type)
        }
        instruction_feature::Kind::InstanceOf(kind) => {
            format!("instanceOf({})", kind.instance_type)
        }
        instruction_feature::Kind::CheckCast(kind) => {
            format!("checkCast({})", kind.cast_type)
        }
    }
}

// =============================================================================
// Ranking / sorting
// =============================================================================

/// JVM primitive type descriptors and names — never minified.
const PRIMITIVE_NAMES: &[&str] = &[
    "boolean", "byte", "char", "double", "float", "int", "long", "short", "void", "z", "b", "c",
    "d", "f", "i", "j", "s", "v",
];

/// Well-known package prefixes — not obfuscation.
const KNOWN_PACKAGES: &[&str] = &[
    "android", "androidx", "app", "com", "dalvik", "dev", "io", "java", "javax", "kotlin",
    "kotlinx", "me", "net", "okhttp3", "okio", "org",
];

/// Compiler-generated suffixes / infixes that look short or strange but are
/// stable across versions.  Matched case-insensitively.
const COMPILER_PATTERNS: &[&str] = &[
    "Companion",
    "DefaultImpls",
    "WhenMappings",
    "Intrinsics",
    "Lambda",
    "lambda",
    "Metadata",
    "Delegates",
];

/// Substrings that signal a compiler-generated name when they appear anywhere
/// inside a segment (e.g. `$Lambda$`, `$$1`).  Checked with `contains`.
const COMPILER_INFIXES: &[&str] = &[
    "$Lambda$", "$lambda$", "$$", "$1", "$2", "$3", "$4", "$5", "$6", "$7", "$8", "$9",
];

/// Short 2-letter identifiers that are real words / common abbreviations in
/// Android / JVM code — should *not* be treated as minified.
const KNOWN_SHORT_SEGMENTS: &[&str] = &[
    "io", "db", "ui", "rx", "id", "tv", "os", "gl", "di", "vm", "op", "am", "pm", "wm", "ok", "no",
    "on", "to", "of", "at", "by", "do", "go", "in", "is", "up", "br", "fs", "gc", "gm", "hw", "ip",
    "ir", "jb", "js", "md", "ml", "mr", "ms", "mv", "mx", "nw", "pb", "px", "qs", "sb", "sh", "sp",
    "sq", "ss", "tb", "tg", "tz", "ws",
];

/// Return a minification score for a fully-qualified name.
///
/// * 0 – definitely not minified (known package, primitive, compiler-generated)
/// * 1 – mildly suspicious (mix of readable and suspicious segments)
/// * 2 – likely minified (majority of segments are suspicious)
/// * 3 – almost certainly minified (all relevant segments are single-letter / tiny)
pub(crate) fn minification_score(fqn: &str) -> i32 {
    if fqn.is_empty() {
        return 0;
    }

    let mut normalized = fqn.trim();

    while normalized.starts_with('[') {
        normalized = &normalized[1..];
    }
    if let Some(s) = normalized.strip_prefix('L')
        && let Some(s2) = s.strip_suffix(';')
    {
        normalized = s2;
    }
    if normalized.contains('<') {
        normalized = normalized.split('<').next().unwrap_or(normalized);
    }
    if normalized.is_empty() {
        return 0;
    }

    if PRIMITIVE_NAMES.contains(&normalized.to_lowercase().as_str()) {
        return 0;
    }

    for infix in COMPILER_INFIXES {
        if fqn.contains(infix) {
            return 0;
        }
    }

    let segments: Vec<&str> = normalized
        .split(&['/', '.', '$'][..])
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return 0;
    }

    let relevant: Vec<&&str> = segments
        .iter()
        .skip_while(|s| KNOWN_PACKAGES.contains(&s.to_lowercase().as_str()))
        .collect();
    let relevant = if relevant.is_empty() {
        return 0;
    } else {
        relevant.into_iter().rev().take(4).collect::<Vec<_>>()
    };

    let suspicious: i32 = relevant.iter().map(|s| segment_minification_score(s)).sum();
    let count = i32::try_from(relevant.len()).expect("relevant package segments capped at 4");

    if suspicious == 0 {
        return 0;
    }

    if suspicious >= count * 2 {
        3
    } else if suspicious > count {
        2
    } else {
        1
    }
}

/// Score a single path segment for minification likelihood.
fn segment_minification_score(segment: &str) -> i32 {
    let s = segment.trim().trim_end_matches(';');
    if s.is_empty() {
        return 0;
    }
    let lower = s.to_lowercase();

    if KNOWN_PACKAGES.contains(&lower.as_str()) || PRIMITIVE_NAMES.contains(&lower.as_str()) {
        return 0;
    }

    if COMPILER_PATTERNS.iter().any(|p| p.eq_ignore_ascii_case(s)) {
        return 0;
    }

    if KNOWN_SHORT_SEGMENTS.contains(&lower.as_str()) {
        return 0;
    }

    if s.chars().all(|c| c.is_ascii_digit()) {
        return 0;
    }

    if !s.chars().any(char::is_alphabetic) {
        return 0;
    }

    if s.len() == 1 {
        return 2;
    }

    if s.len() == 2 && s.chars().all(char::is_alphanumeric) {
        return 1;
    }

    let bytes = lower.as_bytes();
    let alpha_count = bytes.iter().take_while(|b| b.is_ascii_lowercase()).count();
    if alpha_count <= 2 && alpha_count > 0 && bytes[alpha_count..].iter().all(u8::is_ascii_digit) {
        return 1;
    }

    0
}

/// Compute a stability score for a fingerprint.  **Lower is more stable**.
pub(crate) fn stability_score(fp: &MethodFingerprintDto) -> i32 {
    let mut score = 0i32;

    if let Some(ref rt) = fp.return_type {
        score += minification_score(rt);
    }
    if let Some(params) = fp.parameter_values() {
        for p in params {
            score += minification_score(p);
        }
    }

    for instr in &fp.instructions {
        score += instruction_stability(instr);
    }

    score
}

/// Per-instruction stability contribution.
pub(crate) fn instruction_stability(instr: &InstructionFeature) -> i32 {
    match instr.kind_ref() {
        instruction_feature::Kind::StringConst(_) => -2,
        instruction_feature::Kind::Literal(_) => -1,
        instruction_feature::Kind::MethodCall(kind) => {
            let mut s = 0;
            if !kind.use_this_defining_class {
                s += minification_score(&kind.defining_class);
            }
            s += minification_score(&kind.name);
            for p in &kind.parameters {
                s += minification_score(p);
            }
            s += minification_score(&kind.return_type);
            s
        }
        instruction_feature::Kind::FieldAccess(kind) => {
            let mut s = 0;
            if !kind.use_this_defining_class {
                s += minification_score(&kind.defining_class);
            }
            s += minification_score(&kind.name);
            s += minification_score(&kind.field_type);
            s
        }
        instruction_feature::Kind::NewInstance(kind) => minification_score(&kind.instance_type),
        instruction_feature::Kind::InstanceOf(kind) => minification_score(&kind.instance_type),
        instruction_feature::Kind::CheckCast(kind) => minification_score(&kind.cast_type),
    }
}

pub(crate) fn sort_fingerprints(fps: &mut [MethodFingerprintDto]) {
    fps.sort_by(compare_fingerprints);
}

pub(crate) fn compare_fingerprints(a: &MethodFingerprintDto, b: &MethodFingerprintDto) -> Ordering {
    let a_stability = stability_score(a);
    let b_stability = stability_score(b);
    let a_rank = ranking_cost(a, a_stability);
    let b_rank = ranking_cost(b, b_stability);

    a_rank
        .cmp(&b_rank)
        .then(a.size.cmp(&b.size))
        .then(a_stability.cmp(&b_stability))
        .then(count_type(b, is_string_const).cmp(&count_type(a, is_string_const)))
        .then(count_type(b, is_literal).cmp(&count_type(a, is_literal)))
        .then(count_type(b, is_field_access).cmp(&count_type(a, is_field_access)))
        .then({
            let a_rt = i32::from(a.return_type.is_none());
            let b_rt = i32::from(b.return_type.is_none());
            a_rt.cmp(&b_rt)
        })
        .then({
            let a_p = i32::from(a.parameters.is_none());
            let b_p = i32::from(b.parameters.is_none());
            a_p.cmp(&b_p)
        })
}

fn ranking_cost(fp: &MethodFingerprintDto, stability: i32) -> i32 {
    const SIZE_WEIGHT: i32 = 3;

    // Strings are the strongest stable signal we emit, so give them an extra
    // bonus on top of their stability score. This lets multi-string fingerprints
    // outrank terse single-call / parameter-only matches without changing the
    // reported size.
    let size = i32::try_from(fp.size).unwrap_or(i32::MAX / SIZE_WEIGHT);
    let string_bonus = i32::try_from(count_type(fp, is_string_const)).unwrap_or(i32::MAX);

    size.saturating_mul(SIZE_WEIGHT)
        .saturating_add(stability)
        .saturating_sub(string_bonus)
}

fn count_type(fp: &MethodFingerprintDto, predicate: fn(&InstructionFeature) -> bool) -> usize {
    fp.instructions.iter().filter(|i| predicate(i)).count()
}

fn is_string_const(instr: &InstructionFeature) -> bool {
    matches!(instr.kind_ref(), instruction_feature::Kind::StringConst(_))
}

fn is_literal(instr: &InstructionFeature) -> bool {
    matches!(instr.kind_ref(), instruction_feature::Kind::Literal(_))
}

fn is_field_access(instr: &InstructionFeature) -> bool {
    matches!(instr.kind_ref(), instruction_feature::Kind::FieldAccess(_))
}
