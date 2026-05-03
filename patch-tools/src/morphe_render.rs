use std::fmt::Write as _;

use crate::types::{InstructionFeature, MethodFingerprintDto, instruction_feature};

/// Dex access flag constants matching dexlib2's `AccessFlags` enum.
const ACCESS_FLAG_NAMES: &[(i32, &str)] = &[
    (0x0001, "PUBLIC"),
    (0x0002, "PRIVATE"),
    (0x0004, "PROTECTED"),
    (0x0008, "STATIC"),
    (0x0010, "FINAL"),
    (0x0020, "SYNCHRONIZED"),
    (0x0040, "BRIDGE"),
    (0x0080, "VARARGS"),
    (0x0100, "NATIVE"),
    (0x0200, "INTERFACE"),
    (0x0400, "ABSTRACT"),
    (0x0800, "STRICTFP"),
    (0x1000, "SYNTHETIC"),
    (0x2000, "ANNOTATION"),
    (0x4000, "ENUM"),
    (0x10000, "CONSTRUCTOR"),
    (0x20000, "DECLARED_SYNCHRONIZED"),
];

/// Render a `MethodFingerprintDto` as a Morphe Fingerprint(...) Kotlin constructor call.
pub fn to_morphe_code_string(fp: &MethodFingerprintDto) -> String {
    render_fingerprint(&fingerprint_parts(fp))
}

/// Render a method fingerprint that scopes itself to a previously matched class fingerprint.
pub fn to_morphe_code_string_with_class_fingerprint(
    fp: &MethodFingerprintDto,
    class_fingerprint: &MethodFingerprintDto,
) -> String {
    let mut parts = Vec::new();
    let nested = indent_following_lines(&to_morphe_code_string(class_fingerprint), "\t");
    parts.push(format!("classFingerprint = {nested}"));
    parts.extend(fingerprint_parts(fp));
    render_fingerprint(&parts)
}

/// Render a fallback Fingerprint(...) that only pins class and method name.
pub fn to_morphe_name_only_fingerprint_string(defining_class: &str, method_name: &str) -> String {
    format!(
        "Fingerprint(\n\tdefiningClass = {},\n\tname = {}\n)",
        kotlin_string(defining_class),
        kotlin_string(method_name),
    )
}

fn render_instruction_filter(instr: &InstructionFeature) -> String {
    match instr.kind_ref() {
        instruction_feature::Kind::StringConst(kind) => {
            format!("string({})", kotlin_string(&kind.string))
        }
        instruction_feature::Kind::Literal(kind) => format!("literal({})", kind.value),
        instruction_feature::Kind::MethodCall(kind) => {
            let params = kind
                .parameters
                .iter()
                .map(|p| kotlin_string(p))
                .collect::<Vec<_>>()
                .join(", ");
            let dc = if kind.use_this_defining_class {
                "this"
            } else {
                kind.defining_class.as_str()
            };
            format!(
                "methodCall(definingClass = {}, name = {}, parameters = listOf({}), returnType = {})",
                kotlin_string(dc),
                kotlin_string(&kind.name),
                params,
                kotlin_string(&kind.return_type)
            )
        }
        instruction_feature::Kind::FieldAccess(kind) => {
            let dc = if kind.use_this_defining_class {
                "this"
            } else {
                kind.defining_class.as_str()
            };
            format!(
                "fieldAccess(definingClass = {}, name = {}, type = {})",
                kotlin_string(dc),
                kotlin_string(&kind.name),
                kotlin_string(&kind.field_type)
            )
        }
        instruction_feature::Kind::NewInstance(kind) => {
            format!("newInstance({})", kotlin_string(&kind.instance_type))
        }
        instruction_feature::Kind::InstanceOf(kind) => {
            format!("instanceOf({})", kotlin_string(&kind.instance_type))
        }
        instruction_feature::Kind::CheckCast(kind) => {
            format!("checkCast({})", kotlin_string(&kind.cast_type))
        }
    }
}

fn fingerprint_parts(fp: &MethodFingerprintDto) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();

    // Return type — omit for constructors (Morphe normalizes V away)
    let is_constructor = fp.access_flags.is_some_and(|f| f & 0x10000 != 0);
    if let Some(ref rt) = fp.return_type
        && !(is_constructor && rt == "V")
    {
        parts.push(format!("returnType = {}", kotlin_string(rt)));
    }

    // Access flags
    if let Some(flags) = fp.access_flags {
        let flag_strs: Vec<String> = ACCESS_FLAG_NAMES
            .iter()
            .filter(|(mask, _)| flags & mask != 0)
            .map(|(_, name)| format!("AccessFlags.{name}"))
            .collect();
        if !flag_strs.is_empty() {
            parts.push(format!("accessFlags = listOf({})", flag_strs.join(", ")));
        }
    }

    // Parameters
    if let Some(params) = fp.parameter_values() {
        if params.is_empty() {
            parts.push("parameters = listOf()".to_string());
        } else {
            let param_str = params
                .iter()
                .map(|p| kotlin_string(p))
                .collect::<Vec<_>>()
                .join(",\n\t\t");
            parts.push(format!("parameters = listOf(\n\t\t{param_str}\n\t)"));
        }
    }

    // Instruction filters
    if !fp.instructions.is_empty() {
        let filter_strs: Vec<String> = fp
            .instructions
            .iter()
            .map(render_instruction_filter)
            .collect();
        let filter_str = filter_strs.join(",\n\t\t");
        parts.push(format!("filters = listOf(\n\t\t{filter_str}\n\t)"));
    }

    parts
}

fn render_fingerprint(parts: &[String]) -> String {
    let body = parts.join(",\n\t");
    format!("Fingerprint(\n\t{body}\n)")
}

fn indent_following_lines(s: &str, prefix: &str) -> String {
    let mut lines = s.lines();
    let Some(first) = lines.next() else {
        return String::new();
    };

    let mut out = first.to_string();
    for line in lines {
        out.push('\n');
        out.push_str(prefix);
        out.push_str(line);
    }
    out
}

fn kotlin_string(s: &str) -> String {
    format!("\"{}\"", escape_kotlin(s))
}

fn escape_kotlin(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\u{0008}' => out.push_str("\\b"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '$' => out.push_str("\\$"),
            c if (c as u32) >= 32 && (c as u32) <= 126 => out.push(c),
            c => {
                write!(out, "\\u{:04x}", c as u32).expect("writing to a String cannot fail");
            }
        }
    }
    out
}
