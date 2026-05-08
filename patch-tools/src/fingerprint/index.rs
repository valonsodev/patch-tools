use crate::types::{InstructionFeature, MethodData, MethodSignatureDto};
use std::collections::{HashMap, HashSet};

use super::tokens::{InstructionToken, feature_to_index_tokens};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum GeneralFeatureKey {
    ReturnType(String),
    AccessFlags(i32),
    Parameters(Vec<String>),
}

pub(crate) struct MethodEntry {
    pub method_id: String,
    pub class_type: String,
    pub signature: MethodSignatureDto,
    pub instructions: Vec<InstructionFeature>,
    pub general_features: HashSet<GeneralFeatureKey>,
    pub instruction_token_set: HashSet<InstructionToken>,
    pub instruction_token_positions: HashMap<InstructionToken, Vec<u32>>,
}

pub struct FingerprintIndex {
    pub(crate) methods: Vec<MethodEntry>,
    pub(crate) method_index_by_id: HashMap<String, usize>,
    pub(crate) class_method_indices: HashMap<String, Vec<u32>>,
    pub(crate) general_feature_posting: HashMap<GeneralFeatureKey, Vec<u32>>,
    pub(crate) instruction_token_posting: HashMap<InstructionToken, Vec<u32>>,
}

impl FingerprintIndex {
    pub(crate) fn method_by_id(&self, method_id: &str) -> Option<(usize, &MethodEntry)> {
        self.method_index_by_id
            .get(method_id)
            .copied()
            .and_then(|index| self.methods.get(index).map(|method| (index, method)))
    }

    pub(crate) fn method(&self, index: usize) -> Option<&MethodEntry> {
        self.methods.get(index)
    }
}

pub fn build_index(methods: Vec<MethodData>) -> FingerprintIndex {
    let mut method_index_by_id = HashMap::with_capacity(methods.len());
    let mut class_method_indices: HashMap<String, Vec<u32>> = HashMap::new();
    let mut indexed_methods = Vec::with_capacity(methods.len());

    for (index, method) in methods.into_iter().enumerate() {
        // The proto fields are guaranteed populated by `engine_jni::validate_method_data`.
        let info = method
            .info
            .expect("validated method data missing info (must be checked at the JNI boundary)");
        let features = method
            .features
            .expect("validated method data missing features (must be checked at the JNI boundary)");
        let signature = features.signature.expect(
            "validated method features missing signature (must be checked at the JNI boundary)",
        );
        let method_id = info.unique_id.clone();
        let class_type = info.defining_class.clone();
        let index_u32 = u32::try_from(index).expect("method index exceeds u32 range");

        let mut positions_map: HashMap<InstructionToken, Vec<u32>> = HashMap::new();
        for (instruction_index, feature) in features.instructions.iter().enumerate() {
            let instruction_index =
                u32::try_from(instruction_index).expect("instruction index exceeds u32 range");
            for token in feature_to_index_tokens(feature) {
                positions_map
                    .entry(token)
                    .or_default()
                    .push(instruction_index);
            }
        }

        let mut general_features = HashSet::new();
        general_features.insert(GeneralFeatureKey::ReturnType(signature.return_type.clone()));
        general_features.insert(GeneralFeatureKey::AccessFlags(signature.access_flags));
        general_features.insert(GeneralFeatureKey::Parameters(signature.parameters.clone()));

        method_index_by_id.insert(method_id.clone(), index);
        class_method_indices
            .entry(class_type.clone())
            .or_default()
            .push(index_u32);
        indexed_methods.push(MethodEntry {
            method_id,
            class_type,
            signature,
            instructions: features.instructions,
            general_features,
            instruction_token_set: positions_map.keys().cloned().collect(),
            instruction_token_positions: positions_map,
        });
    }

    let mut general_posting: HashMap<GeneralFeatureKey, Vec<u32>> = HashMap::new();
    let mut instruction_posting: HashMap<InstructionToken, Vec<u32>> = HashMap::new();

    for (idx, method) in indexed_methods.iter().enumerate() {
        let idx = u32::try_from(idx).expect("method index exceeds u32 range");
        for key in &method.general_features {
            general_posting.entry(key.clone()).or_default().push(idx);
        }
        for token in &method.instruction_token_set {
            instruction_posting
                .entry(token.clone())
                .or_default()
                .push(idx);
        }
    }

    FingerprintIndex {
        methods: indexed_methods,
        method_index_by_id,
        class_method_indices,
        general_feature_posting: general_posting,
        instruction_token_posting: instruction_posting,
    }
}
