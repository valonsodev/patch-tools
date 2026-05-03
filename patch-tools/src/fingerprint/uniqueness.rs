use std::collections::HashSet;

use anyhow::Result;

use super::candidates::SemanticFingerprint;
use super::index::{FingerprintIndex, GeneralFeatureKey};
use super::tokens::{InstructionToken, intersect_sorted, is_subsequence};

pub(crate) fn class_scope_for_method<'a>(
    index: &'a FingerprintIndex,
    target_method_id: &str,
) -> Result<&'a [u32]> {
    let class_type = index
        .method_by_id(target_method_id)
        .map(|(_, method)| method.class_type.as_str())
        .ok_or_else(|| anyhow::anyhow!("Method not found in index: {target_method_id}"))?;
    let scope = index
        .class_method_indices
        .get(class_type)
        .ok_or_else(|| anyhow::anyhow!("Class not found in index: {class_type}"))?;
    Ok(scope.as_slice())
}

pub(crate) fn is_unique(
    index: &FingerprintIndex,
    fingerprint: &SemanticFingerprint,
    target_idx: usize,
    scope: Option<&[u32]>,
) -> bool {
    let target_idx_u32 = u32::try_from(target_idx).expect("target index exceeds u32 range");
    let mut general_keys = Vec::new();
    if let Some(ref return_type) = fingerprint.return_type {
        general_keys.push(GeneralFeatureKey::ReturnType(return_type.clone()));
    }
    if let Some(access_flags) = fingerprint.access_flags {
        general_keys.push(GeneralFeatureKey::AccessFlags(access_flags));
    }
    if let Some(ref parameters) = fingerprint.parameters {
        general_keys.push(GeneralFeatureKey::Parameters(parameters.clone()));
    }
    let instr_tokens = &fingerprint.instructions;
    let instr_set: HashSet<&InstructionToken> = instr_tokens.iter().collect();

    let mut posting_lists: Vec<&Vec<u32>> = Vec::new();
    for key in &general_keys {
        match index.general_feature_posting.get(key) {
            Some(posting) => posting_lists.push(posting),
            None => return false,
        }
    }
    for token in &instr_set {
        match index.instruction_token_posting.get(*token) {
            Some(posting) => posting_lists.push(posting),
            None => return false,
        }
    }

    let candidate_pool = if posting_lists.is_empty() {
        None
    } else {
        posting_lists.sort_by_key(|posting| posting.len());
        let mut pool = posting_lists[0].clone();
        for posting_list in posting_lists.iter().skip(1) {
            pool = intersect_sorted(&pool, posting_list);
            if pool.is_empty() {
                return false;
            }
        }
        if pool.binary_search(&target_idx_u32).is_err() {
            return false;
        }
        Some(pool)
    };

    let scoped_pool = match (&candidate_pool, scope) {
        (Some(pool), Some(scope_indices)) => intersect_sorted(pool, scope_indices),
        (Some(pool), None) => pool.clone(),
        (None, _) => Vec::new(),
    };

    if scoped_pool.binary_search(&target_idx_u32).is_err() {
        return false;
    }

    let candidates = match &candidate_pool {
        Some(_) if !scoped_pool.is_empty() => scoped_pool.as_slice(),
        _ => return false,
    };

    let mut match_count = 0u32;
    let mut matched_target = false;

    for &method_idx in candidates {
        let method_idx_usize =
            usize::try_from(method_idx).expect("method index exceeds usize range");
        let indexed = &index.methods[method_idx_usize];

        if !general_keys
            .iter()
            .all(|key| indexed.general_features.contains(key))
        {
            continue;
        }

        if !instr_tokens.is_empty() {
            if !instr_set
                .iter()
                .all(|token| indexed.instruction_token_set.contains(token))
            {
                continue;
            }
            if !is_subsequence(instr_tokens, &indexed.instruction_token_positions) {
                continue;
            }
        }

        match_count += 1;
        if method_idx_usize == target_idx {
            matched_target = true;
        }
        if match_count > 1 {
            return false;
        }
    }

    match_count == 1 && matched_target
}
