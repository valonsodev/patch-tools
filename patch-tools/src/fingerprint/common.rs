use crate::morphe_render::to_morphe_code_string;
use crate::types::{InstructionFeature, MethodFingerprintDto};
use anyhow::{Result, bail};
use itertools::Itertools;
use rayon::prelude::*;
use std::collections::HashSet;

use super::candidates::{
    Candidate, SemanticFingerprint, SignatureCombo, build_fingerprint, to_semantic,
};
use super::index::{FingerprintIndex, MethodEntry};
use super::stability::sort_fingerprints;
use super::tokens::feature_to_token;
use super::uniqueness::is_unique;
use super::variants::expand_variants;

const MAX_EMITTED_FINGERPRINTS: usize = 5000;
const FINGERPRINT_BATCH_SIZE: usize = 4096;

struct CommonSearchTarget<'a> {
    index: &'a FingerprintIndex,
    target_idx: usize,
    source: &'a MethodEntry,
}

pub fn generate_common<'a>(
    targets: impl IntoIterator<Item = (&'a FingerprintIndex, &'a str)>,
    limit: usize,
) -> Result<Vec<MethodFingerprintDto>> {
    let mut resolved = Vec::new();
    for (index, method_id) in targets {
        let Some((target_idx, source)) = index.method_by_id(method_id) else {
            bail!("Method not found in index: {method_id}");
        };
        resolved.push(CommonSearchTarget {
            index,
            target_idx,
            source,
        });
    }

    if resolved.len() < 2 {
        bail!("common-fingerprint requires at least 2 APK/method pairs");
    }

    let mut search = CommonFingerprintSearch::new(resolved);
    for total_size in 1..=search.max_total_size() {
        if search.collect_total_size(total_size) {
            break;
        }
    }

    let mut results = search.into_results();
    if results.is_empty() {
        bail!(
            "Could not find a common fingerprint that uniquely identifies the selected method in every APK."
        );
    }

    sort_fingerprints(&mut results);
    results.truncate(limit.min(MAX_EMITTED_FINGERPRINTS));

    for fp in &mut results {
        fp.morphe_code = to_morphe_code_string(fp);
    }

    Ok(results)
}

struct CommonFingerprintSearch<'a> {
    targets: Vec<CommonSearchTarget<'a>>,
    sig_combos: Vec<SignatureCombo>,
    sorted_instructions: Vec<InstructionFeature>,
    seen: HashSet<SemanticFingerprint>,
    results: Vec<MethodFingerprintDto>,
    batch: Vec<Candidate>,
}

impl<'a> CommonFingerprintSearch<'a> {
    fn new(targets: Vec<CommonSearchTarget<'a>>) -> Self {
        let sig_combos = common_signature_combinations(&targets);
        let sorted_instructions = common_instruction_bases(&targets);
        Self {
            targets,
            sig_combos,
            sorted_instructions,
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
            if !variant_is_common(&self.targets, &variant) {
                continue;
            }

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
        flush_common_batch(
            &mut self.batch,
            &self.targets,
            &self.sig_combos,
            &mut self.results,
        );
    }

    fn into_results(mut self) -> Vec<MethodFingerprintDto> {
        self.flush_batch();
        self.results
    }
}

fn common_signature_combinations(targets: &[CommonSearchTarget<'_>]) -> Vec<SignatureCombo> {
    let first = &targets[0].source.signature;

    let return_type = targets
        .iter()
        .all(|target| target.source.signature.return_type == first.return_type)
        .then(|| first.return_type.clone());
    let access_flags = targets
        .iter()
        .all(|target| target.source.signature.access_flags == first.access_flags)
        .then_some(first.access_flags);
    let parameters = targets
        .iter()
        .all(|target| target.source.signature.parameters == first.parameters)
        .then(|| first.parameters.clone());

    let mut combos = Vec::new();
    for mask in 0..8_u8 {
        combos.push(SignatureCombo {
            return_type: if mask & 1 != 0 {
                return_type.clone()
            } else {
                None
            },
            access_flags: if mask & 2 != 0 { access_flags } else { None },
            parameters: if mask & 4 != 0 {
                parameters.clone()
            } else {
                None
            },
        });
    }

    combos.sort_by_key(SignatureCombo::size);
    combos.dedup_by(|a, b| {
        a.return_type == b.return_type
            && a.access_flags == b.access_flags
            && a.parameters == b.parameters
    });
    combos
}

fn common_instruction_bases(targets: &[CommonSearchTarget<'_>]) -> Vec<InstructionFeature> {
    let mut scored = targets[0]
        .source
        .instructions
        .iter()
        .enumerate()
        .filter_map(|(position, feature)| {
            common_instruction_score(targets, feature)
                .map(|score| (score, position, feature.clone()))
        })
        .collect::<Vec<_>>();

    scored.sort_by_key(|(score, position, _)| (*score, *position));
    scored.into_iter().map(|(_, _, feature)| feature).collect()
}

fn common_instruction_score(
    targets: &[CommonSearchTarget<'_>],
    feature: &InstructionFeature,
) -> Option<usize> {
    let single = [feature];
    expand_variants(&single)
        .into_iter()
        .filter(|variant| variant_is_common(targets, variant))
        .map(|variant| {
            variant.iter().fold(0usize, |acc, feature| {
                let token = feature_to_token(feature);
                targets.iter().fold(acc, |acc, target| {
                    let count = target
                        .index
                        .instruction_token_posting
                        .get(&token)
                        .map_or(usize::MAX / 4, Vec::len);
                    acc.saturating_add(count)
                })
            })
        })
        .min()
}

fn variant_is_common(targets: &[CommonSearchTarget<'_>], variant: &[InstructionFeature]) -> bool {
    let tokens = variant.iter().map(feature_to_token).collect::<Vec<_>>();
    targets.iter().all(|target| {
        tokens
            .iter()
            .all(|token| target.source.instruction_token_set.contains(token))
    })
}

fn flush_common_batch(
    batch: &mut Vec<Candidate>,
    targets: &[CommonSearchTarget<'_>],
    sig_combos: &[SignatureCombo],
    results: &mut Vec<MethodFingerprintDto>,
) {
    if batch.is_empty() {
        return;
    }

    let candidates = std::mem::take(batch);
    let unique = candidates
        .into_par_iter()
        .filter(|candidate| {
            targets
                .iter()
                .all(|target| is_unique(target.index, &candidate.semantic, target.target_idx, None))
        })
        .collect::<Vec<_>>();

    for candidate in unique {
        results.push(build_fingerprint(
            &sig_combos[candidate.combo_idx],
            candidate.effective_rt.as_ref(),
            candidate.variant,
        ));
    }
}
