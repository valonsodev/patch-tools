use anyhow::Result;
use std::collections::HashMap;

use super::index::{FingerprintIndex, MethodEntry};
use super::stability::{instruction_stability, minification_score};
use super::tokens::{InstructionToken, feature_to_token};
use crate::access_flags::DexAccessFlag;
use crate::types::MethodSignatureDto;

#[derive(Debug, Clone)]
pub struct MethodMapCandidate {
    pub method_id: String,
    pub similarity: f64,
}

struct WeightedProfile {
    tokens: Vec<(InstructionToken, f64)>,
    token_bag: HashMap<InstructionToken, f64>,
    total_weight: f64,
}

pub fn map_methods(
    source_index: &FingerprintIndex,
    source_method_id: &str,
    target_index: &FingerprintIndex,
    limit: usize,
) -> Result<Vec<MethodMapCandidate>> {
    let (_, source) = source_index
        .method_by_id(source_method_id)
        .ok_or_else(|| anyhow::anyhow!("Method not found in index: {source_method_id}"))?;
    let source_profile = build_profile(source);
    let source_feature_count = source.instructions.len();

    let mut candidates = target_index
        .methods
        .iter()
        .map(|target| {
            let target_profile = build_profile(target);
            let similarity =
                method_similarity(source, &source_profile, target, &target_profile) * 100.0;
            let feature_count_delta = source_feature_count.abs_diff(target.instructions.len());
            (similarity, feature_count_delta, target.method_id.clone())
        })
        .collect::<Vec<_>>();

    candidates.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    candidates.truncate(limit);

    Ok(candidates
        .into_iter()
        .map(|(similarity, _, method_id)| MethodMapCandidate {
            method_id,
            similarity,
        })
        .collect())
}

fn method_similarity(
    source: &MethodEntry,
    source_profile: &WeightedProfile,
    target: &MethodEntry,
    target_profile: &WeightedProfile,
) -> f64 {
    let instruction_overlap =
        weighted_jaccard(&source_profile.token_bag, &target_profile.token_bag);
    let ordered_coverage = ordered_coverage(source_profile, target_profile);
    let signature = signature_similarity(&source.signature, &target.signature);
    let identity = identity_similarity(source, target);

    (instruction_overlap * 0.45)
        + (ordered_coverage * 0.20)
        + (signature * 0.20)
        + (identity * 0.15)
}

fn build_profile(method: &MethodEntry) -> WeightedProfile {
    let tokens = method
        .instructions
        .iter()
        .map(|instruction| {
            (
                feature_to_token(instruction),
                instruction_weight(instruction_stability(instruction)),
            )
        })
        .collect::<Vec<_>>();

    let mut token_bag = HashMap::new();
    for (token, weight) in &tokens {
        *token_bag.entry(token.clone()).or_insert(0.0) += *weight;
    }

    let total_weight = tokens.iter().map(|(_, weight)| *weight).sum();

    WeightedProfile {
        tokens,
        token_bag,
        total_weight,
    }
}

fn instruction_weight(stability: i32) -> f64 {
    match stability {
        ..=-2 => 8.0,
        -1 => 6.0,
        0 => 3.0,
        1 => 2.0,
        _ => 1.0,
    }
}

fn weighted_jaccard(
    source: &HashMap<InstructionToken, f64>,
    target: &HashMap<InstructionToken, f64>,
) -> f64 {
    if source.is_empty() && target.is_empty() {
        return 1.0;
    }

    let mut intersection = 0.0;
    let mut union = 0.0;
    for (token, source_weight) in source {
        let target_weight = target.get(token).copied().unwrap_or(0.0);
        intersection += source_weight.min(target_weight);
        union += source_weight.max(target_weight);
    }
    for (token, target_weight) in target {
        if !source.contains_key(token) {
            union += *target_weight;
        }
    }

    if union == 0.0 {
        0.0
    } else {
        intersection / union
    }
}

fn ordered_coverage(source: &WeightedProfile, target: &WeightedProfile) -> f64 {
    if source.tokens.is_empty() {
        return if target.tokens.is_empty() { 1.0 } else { 0.0 };
    }

    let mut next_target_index = 0usize;
    let mut matched_weight = 0.0;

    for (source_token, source_weight) in &source.tokens {
        let Some(found_index) = target
            .tokens
            .iter()
            .enumerate()
            .skip(next_target_index)
            .find_map(|(index, (target_token, _))| (target_token == source_token).then_some(index))
        else {
            continue;
        };
        matched_weight += *source_weight;
        next_target_index = found_index.saturating_add(1);
    }

    if source.total_weight == 0.0 {
        0.0
    } else {
        matched_weight / source.total_weight
    }
}

fn signature_similarity(source: &MethodSignatureDto, target: &MethodSignatureDto) -> f64 {
    let return_score = type_similarity(&source.return_type, &target.return_type);
    let parameter_score = parameter_similarity(&source.parameters, &target.parameters);
    let access_score = access_similarity(source.access_flags, target.access_flags);

    (return_score * 0.40) + (parameter_score * 0.45) + (access_score * 0.15)
}

fn parameter_similarity(source: &[String], target: &[String]) -> f64 {
    if source == target {
        return 1.0;
    }
    if source.is_empty() || target.is_empty() {
        return if source.len() == target.len() {
            1.0
        } else {
            0.0
        };
    }

    let aligned = source
        .iter()
        .zip(target.iter())
        .map(|(source, target)| type_similarity(source, target))
        .sum::<f64>();
    let max_len = usize_to_f64(source.len().max(target.len()));

    (aligned / max_len) * 0.75
        + if source.len() == target.len() {
            0.25
        } else {
            0.0
        }
}

fn access_similarity(source: i32, target: i32) -> f64 {
    let relevant_flags = DexAccessFlag::mask_for(DexAccessFlag::MAP_SIMILARITY_RELEVANT);
    let source = source & relevant_flags;
    let target = target & relevant_flags;
    if source == target {
        return 1.0;
    }

    let differing_bits = f64::from((source ^ target).count_ones());
    let total_bits = f64::from(relevant_flags.count_ones());
    1.0 - (differing_bits / total_bits)
}

fn usize_to_f64(value: usize) -> f64 {
    f64::from(u32::try_from(value).expect("parameter count fits in u32"))
}

fn type_similarity(source: &str, target: &str) -> f64 {
    if source == target {
        return 1.0;
    }

    let source_kind = descriptor_kind(source);
    let target_kind = descriptor_kind(target);
    if source_kind == target_kind {
        0.35
    } else {
        0.0
    }
}

#[derive(PartialEq, Eq)]
enum DescriptorKind<'a> {
    Object,
    Primitive(&'a str),
}

fn identity_similarity(source: &MethodEntry, target: &MethodEntry) -> f64 {
    let class_score = class_similarity(source, target);
    let member_score = member_similarity(&source.method_id, &target.method_id);

    (class_score * 0.70) + (member_score * 0.30)
}

fn class_similarity(source: &MethodEntry, target: &MethodEntry) -> f64 {
    if source.class_type == target.class_type {
        return 1.0;
    }

    let source_score = minification_score(&source.class_type);
    let target_score = minification_score(&target.class_type);
    if source_score == 0 && target_score == 0 {
        strsim::jaro_winkler(&source.class_type, &target.class_type).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn member_similarity(source_method_id: &str, target_method_id: &str) -> f64 {
    let source_name = method_name(source_method_id);
    let target_name = method_name(target_method_id);

    if source_name == target_name {
        return 1.0;
    }

    if minification_score(source_name) == 0 && minification_score(target_name) == 0 {
        strsim::jaro_winkler(source_name, target_name).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn method_name(method_id: &str) -> &str {
    method_id
        .split_once("->")
        .map_or(method_id, |(_, name_and_signature)| {
            name_and_signature
                .split_once('(')
                .map_or(name_and_signature, |(name, _)| name)
        })
}

fn descriptor_kind(descriptor: &str) -> DescriptorKind<'_> {
    let normalized = descriptor.trim_start_matches('[');
    if normalized.starts_with('L') {
        DescriptorKind::Object
    } else {
        DescriptorKind::Primitive(normalized)
    }
}
