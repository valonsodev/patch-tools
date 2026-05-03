use crate::morphe_render::{to_morphe_code_string, to_morphe_code_string_with_class_fingerprint};
use crate::types::{InstructionFeature, MethodFingerprintDto};
use anyhow::{Result, bail};
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::HashSet;

use super::candidates::{
    Candidate, SemanticFingerprint, SignatureCombo, build_fingerprint, signature_combinations,
    sort_instructions_by_selectivity, to_semantic,
};
use super::index::FingerprintIndex;
use super::stability::{compare_fingerprints, sort_fingerprints};
use super::uniqueness::{class_scope_for_method, is_unique};
use super::variants::expand_variants;

const MAX_EMITTED_FINGERPRINTS: usize = 5000;
const FINGERPRINT_BATCH_SIZE: usize = 4096;

pub struct ClassFingerprintCandidate {
    pub source_method_id: String,
    pub fingerprint: MethodFingerprintDto,
}

pub fn generate_all(
    index: &FingerprintIndex,
    target_method_id: &str,
    limit: usize,
) -> Result<Vec<MethodFingerprintDto>> {
    generate_all_scoped(index, target_method_id, limit, None)
}

pub fn generate_class_scoped(
    index: &FingerprintIndex,
    target_method_id: &str,
    limit: usize,
) -> Result<Vec<MethodFingerprintDto>> {
    let scope = class_scope_for_method(index, target_method_id)?;
    generate_all_scoped(index, target_method_id, limit, Some(scope))
}

pub fn best_class_fingerprint_for_method(
    index: &FingerprintIndex,
    target_method_id: &str,
) -> Result<ClassFingerprintCandidate> {
    let class_type = index
        .method_by_id(target_method_id)
        .map(|(_, method)| method.class_type.clone())
        .ok_or_else(|| anyhow::anyhow!("Method not found in index: {target_method_id}"))?;
    generate_class_fingerprints(index, &class_type, 1)?
        .into_iter()
        .next()
        .ok_or_else(|| {
            anyhow::anyhow!(
                "Could not distinguish class '{class_type}' from all other classes using any method fingerprint."
            )
        })
}

pub fn apply_class_fingerprint_to_results(
    results: &mut [MethodFingerprintDto],
    class_fingerprint: &MethodFingerprintDto,
) {
    for fp in results {
        fp.size = fp
            .size
            .saturating_add(class_fingerprint.size)
            .saturating_add(1);
        fp.morphe_code = to_morphe_code_string_with_class_fingerprint(fp, class_fingerprint);
    }
}

pub fn generate_class_fingerprints(
    index: &FingerprintIndex,
    class_type: &str,
    limit: usize,
) -> Result<Vec<ClassFingerprintCandidate>> {
    let method_indices = index
        .class_method_indices
        .get(class_type)
        .ok_or_else(|| anyhow::anyhow!("Class not found in index: {class_type}"))?;

    let mut candidates = Vec::new();
    for &method_idx in method_indices {
        let method_idx = usize::try_from(method_idx).expect("method index exceeds usize range");
        let method = index
            .method(method_idx)
            .ok_or_else(|| anyhow::anyhow!("Method missing for class fingerprint candidate"))?;
        let method_id = &method.method_id;

        let Ok(mut fingerprints) = generate_all(index, method_id, 1) else {
            continue;
        };
        let Some(fingerprint) = fingerprints.pop() else {
            continue;
        };
        candidates.push(ClassFingerprintCandidate {
            source_method_id: method_id.clone(),
            fingerprint,
        });
    }

    if candidates.is_empty() {
        bail!(
            "Could not distinguish class '{class_type}' from all other classes using any method fingerprint."
        );
    }

    candidates.sort_by(|a, b| compare_fingerprints(&a.fingerprint, &b.fingerprint));
    candidates.truncate(limit.min(MAX_EMITTED_FINGERPRINTS));
    Ok(candidates)
}

fn generate_all_scoped(
    index: &FingerprintIndex,
    target_method_id: &str,
    limit: usize,
    scope: Option<&[u32]>,
) -> Result<Vec<MethodFingerprintDto>> {
    let target_idx = index
        .method_by_id(target_method_id)
        .ok_or_else(|| anyhow::anyhow!("Method not found in index: {target_method_id}"))?;
    let (target_idx, source) = target_idx;

    let mut search = FingerprintSearch::new(index, target_idx, source, scope);
    for total_size in 1..=search.max_total_size() {
        if search.collect_total_size(total_size) {
            break;
        }
    }

    let mut results = search.into_results();

    if results.is_empty() {
        let scope_label = match scope {
            Some(_) => "all other methods in its class",
            None => "all other methods",
        };
        bail!("Could not distinguish target method '{target_method_id}' from {scope_label}.");
    }

    sort_fingerprints(&mut results);

    results.truncate(limit.min(MAX_EMITTED_FINGERPRINTS));

    for fp in &mut results {
        fp.morphe_code = to_morphe_code_string(fp);
    }

    Ok(results)
}

struct FingerprintSearch<'a> {
    index: &'a FingerprintIndex,
    target_idx: usize,
    scope: Option<&'a [u32]>,
    sig_combos: Vec<SignatureCombo>,
    sorted_instructions: Vec<InstructionFeature>,
    seen: HashSet<SemanticFingerprint>,
    results: Vec<MethodFingerprintDto>,
    batch: Vec<Candidate>,
}

impl<'a> FingerprintSearch<'a> {
    fn new(
        index: &'a FingerprintIndex,
        target_idx: usize,
        source: &super::index::MethodEntry,
        scope: Option<&'a [u32]>,
    ) -> Self {
        Self {
            index,
            target_idx,
            scope,
            sig_combos: signature_combinations(&source.signature),
            sorted_instructions: sort_instructions_by_selectivity(index, &source.instructions),
            seen: HashSet::new(),
            results: Vec::new(),
            batch: Vec::with_capacity(FINGERPRINT_BATCH_SIZE),
        }
    }

    fn max_total_size(&self) -> usize {
        let max_general = self
            .sig_combos
            .iter()
            .map(SignatureCombo::size)
            .max()
            .unwrap_or(0);
        max_general + self.sorted_instructions.len()
    }

    fn collect_total_size(&mut self, total_size: usize) -> bool {
        for combo_idx in 0..self.sig_combos.len() {
            let sig_combo = self.sig_combos[combo_idx].clone();
            let Some(instr_size) = total_size.checked_sub(sig_combo.size()) else {
                continue;
            };
            if instr_size > self.sorted_instructions.len() {
                continue;
            }

            if self.collect_instruction_size(total_size, combo_idx, &sig_combo, instr_size) {
                return true;
            }
        }

        self.flush_batch();
        self.results.len() >= MAX_EMITTED_FINGERPRINTS
    }

    fn collect_instruction_size(
        &mut self,
        total_size: usize,
        combo_idx: usize,
        sig_combo: &SignatureCombo,
        instr_size: usize,
    ) -> bool {
        let sorted_instructions = self.sorted_instructions.clone();
        let effective_rt = sig_combo.effective_return_type();
        let instr_combos: Box<dyn Iterator<Item = Vec<&InstructionFeature>>> = if instr_size == 0 {
            Box::new(std::iter::once(Vec::new()))
        } else {
            Box::new(sorted_instructions.iter().combinations(instr_size))
        };

        for mut instr_combo in instr_combos {
            instr_combo.sort_by_key(|f| f.index);
            if self.collect_variants(
                total_size,
                combo_idx,
                sig_combo,
                effective_rt.as_deref(),
                &instr_combo,
            ) {
                return true;
            }
        }

        false
    }

    fn collect_variants(
        &mut self,
        total_size: usize,
        combo_idx: usize,
        sig_combo: &SignatureCombo,
        effective_rt: Option<&str>,
        instr_combo: &[&InstructionFeature],
    ) -> bool {
        for variant in expand_variants(instr_combo) {
            let semantic = to_semantic(
                effective_rt,
                sig_combo.access_flags,
                sig_combo.parameters.as_deref(),
                &variant,
            );

            if !self.seen.insert(semantic.clone()) {
                continue;
            }

            self.batch.push(Candidate {
                semantic,
                combo_idx,
                effective_rt: effective_rt.map(str::to_owned),
                variant,
            });

            if self.batch.len() >= FINGERPRINT_BATCH_SIZE {
                self.flush_batch();
                if total_size > 1 && self.results.len() >= MAX_EMITTED_FINGERPRINTS {
                    return true;
                }
            }
        }

        false
    }

    fn flush_batch(&mut self) {
        flush_batch(
            &mut self.batch,
            self.index,
            self.target_idx,
            self.scope,
            &self.sig_combos,
            &mut self.results,
        );
    }

    fn into_results(mut self) -> Vec<MethodFingerprintDto> {
        self.flush_batch();
        self.results
    }
}

fn flush_batch(
    batch: &mut Vec<Candidate>,
    index: &FingerprintIndex,
    target_idx: usize,
    scope: Option<&[u32]>,
    sig_combos: &[SignatureCombo],
    results: &mut Vec<MethodFingerprintDto>,
) {
    if batch.is_empty() {
        return;
    }
    let candidates = std::mem::take(batch);
    let unique: Vec<Candidate> = candidates
        .into_par_iter()
        .filter(|c| is_unique(index, &c.semantic, target_idx, scope))
        .collect();
    for c in unique {
        results.push(build_fingerprint(
            &sig_combos[c.combo_idx],
            c.effective_rt.as_ref(),
            c.variant,
        ));
    }
}
