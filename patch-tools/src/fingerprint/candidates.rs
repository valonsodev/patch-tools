use crate::access_flags::DexAccessFlag;
use crate::types::{InstructionFeature, MethodFingerprintDto, MethodSignatureDto, ParameterList};

use super::index::FingerprintIndex;
use super::tokens::{InstructionToken, feature_to_token};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct SemanticFingerprint {
    pub(crate) return_type: Option<String>,
    pub(crate) access_flags: Option<i32>,
    pub(crate) parameters: Option<Vec<String>>,
    pub(crate) instructions: Vec<InstructionToken>,
}

#[derive(Clone)]
pub(crate) struct SignatureCombo {
    pub(crate) return_type: Option<String>,
    pub(crate) access_flags: Option<i32>,
    pub(crate) parameters: Option<Vec<String>>,
}

impl SignatureCombo {
    pub(crate) fn size(&self) -> usize {
        usize::from(self.return_type.is_some())
            + usize::from(self.access_flags.is_some())
            + usize::from(self.parameters.is_some())
    }

    pub(crate) fn effective_return_type(&self) -> Option<String> {
        if let (Some(flags), Some(return_type)) = (self.access_flags, &self.return_type)
            && DexAccessFlag::Constructor.is_set(flags)
            && return_type == "V"
        {
            return None;
        }
        self.return_type.clone()
    }
}

pub(crate) struct Candidate {
    pub(crate) semantic: SemanticFingerprint,
    pub(crate) combo_idx: usize,
    pub(crate) effective_rt: Option<String>,
    pub(crate) variant: Vec<InstructionFeature>,
}

pub(crate) fn sort_instructions_by_selectivity(
    index: &FingerprintIndex,
    instructions: &[InstructionFeature],
) -> Vec<InstructionFeature> {
    let mut sorted: Vec<InstructionFeature> = instructions.to_vec();
    sorted.sort_by_cached_key(|feat| {
        let token = feature_to_token(feat);
        index
            .instruction_token_posting
            .get(&token)
            .map_or(0, Vec::len)
    });
    sorted
}

pub(crate) fn to_semantic(
    rt: Option<&str>,
    af: Option<i32>,
    params: Option<&[String]>,
    instructions: &[InstructionFeature],
) -> SemanticFingerprint {
    SemanticFingerprint {
        return_type: rt.map(ToOwned::to_owned),
        access_flags: af,
        parameters: params.map(ToOwned::to_owned),
        instructions: instructions.iter().map(feature_to_token).collect(),
    }
}

pub(crate) fn build_fingerprint(
    sig_combo: &SignatureCombo,
    effective_rt: Option<&String>,
    instructions: Vec<InstructionFeature>,
) -> MethodFingerprintDto {
    let mut size = instructions.len();
    if effective_rt.is_some() {
        size += 1;
    }
    if sig_combo.access_flags.is_some() {
        size += 1;
    }
    if sig_combo.parameters.is_some() {
        size += 1;
    }

    MethodFingerprintDto {
        return_type: effective_rt.cloned(),
        access_flags: sig_combo.access_flags,
        parameters: sig_combo
            .parameters
            .clone()
            .map(|values| ParameterList { values }),
        instructions,
        size: u32::try_from(size).expect("fingerprint size exceeds u32 range"),
        morphe_code: String::new(),
    }
}

pub(crate) fn signature_combinations(signature: &MethodSignatureDto) -> Vec<SignatureCombo> {
    let mut combos = Vec::new();
    for mask in 0..8_u8 {
        combos.push(SignatureCombo {
            return_type: if mask & 1 != 0 {
                Some(signature.return_type.clone())
            } else {
                None
            },
            access_flags: if mask & 2 != 0 {
                Some(signature.access_flags)
            } else {
                None
            },
            parameters: if mask & 4 != 0 {
                Some(signature.parameters.clone())
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
