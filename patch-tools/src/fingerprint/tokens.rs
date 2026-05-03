use crate::types::{InstructionFeature, instruction_feature};
use std::collections::HashMap;

/// Index-friendly instruction token stripped of bytecode index.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum InstructionToken {
    Literal {
        value: i64,
    },
    StringValue {
        value: String,
    },
    MethodCall {
        defining_class: Option<String>,
        same_defining_class: bool,
        name: String,
        parameters: Vec<String>,
        return_type: String,
    },
    FieldAccess {
        defining_class: Option<String>,
        same_defining_class: bool,
        name: String,
        field_type: String,
    },
    NewInstance {
        instance_type: String,
    },
    InstanceOf {
        instance_type: String,
    },
    CheckCast {
        cast_type: String,
    },
}

pub(crate) fn feature_to_token(feat: &InstructionFeature) -> InstructionToken {
    match feat.kind_ref() {
        instruction_feature::Kind::Literal(kind) => InstructionToken::Literal { value: kind.value },
        instruction_feature::Kind::StringConst(kind) => InstructionToken::StringValue {
            value: kind.string.clone(),
        },
        instruction_feature::Kind::MethodCall(kind) => InstructionToken::MethodCall {
            defining_class: if kind.use_this_defining_class {
                None
            } else {
                Some(kind.defining_class.clone())
            },
            same_defining_class: kind.use_this_defining_class,
            name: kind.name.clone(),
            parameters: kind.parameters.clone(),
            return_type: kind.return_type.clone(),
        },
        instruction_feature::Kind::FieldAccess(kind) => InstructionToken::FieldAccess {
            defining_class: if kind.use_this_defining_class {
                None
            } else {
                Some(kind.defining_class.clone())
            },
            same_defining_class: kind.use_this_defining_class,
            name: kind.name.clone(),
            field_type: kind.field_type.clone(),
        },
        instruction_feature::Kind::NewInstance(kind) => InstructionToken::NewInstance {
            instance_type: kind.instance_type.clone(),
        },
        instruction_feature::Kind::InstanceOf(kind) => InstructionToken::InstanceOf {
            instance_type: kind.instance_type.clone(),
        },
        instruction_feature::Kind::CheckCast(kind) => InstructionToken::CheckCast {
            cast_type: kind.cast_type.clone(),
        },
    }
}

pub(crate) fn feature_to_index_tokens(feat: &InstructionFeature) -> Vec<InstructionToken> {
    let base = feature_to_token(feat);
    match feat.kind_ref() {
        instruction_feature::Kind::MethodCall(kind)
            if kind.same_defining_class && !kind.use_this_defining_class =>
        {
            vec![
                base,
                InstructionToken::MethodCall {
                    defining_class: None,
                    same_defining_class: true,
                    name: kind.name.clone(),
                    parameters: kind.parameters.clone(),
                    return_type: kind.return_type.clone(),
                },
            ]
        }
        instruction_feature::Kind::FieldAccess(kind)
            if kind.same_defining_class && !kind.use_this_defining_class =>
        {
            vec![
                base,
                InstructionToken::FieldAccess {
                    defining_class: None,
                    same_defining_class: true,
                    name: kind.name.clone(),
                    field_type: kind.field_type.clone(),
                },
            ]
        }
        _ => vec![base],
    }
}

pub(crate) fn intersect_sorted(a: &[u32], b: &[u32]) -> Vec<u32> {
    let mut result = Vec::with_capacity(a.len().min(b.len()));
    let (mut i, mut j) = (0, 0);
    while i < a.len() && j < b.len() {
        match a[i].cmp(&b[j]) {
            std::cmp::Ordering::Equal => {
                result.push(a[i]);
                i += 1;
                j += 1;
            }
            std::cmp::Ordering::Less => i += 1,
            std::cmp::Ordering::Greater => j += 1,
        }
    }
    result
}

pub(crate) fn is_subsequence(
    tokens: &[InstructionToken],
    positions: &HashMap<InstructionToken, Vec<u32>>,
) -> bool {
    if tokens.is_empty() {
        return true;
    }
    let mut next_target = 0_u32;
    for token in tokens {
        let Some(pos_array) = positions.get(token) else {
            return false;
        };
        match pos_array.binary_search(&next_target) {
            Ok(idx) => next_target = pos_array[idx].saturating_add(1),
            Err(idx) => {
                if idx < pos_array.len() {
                    next_target = pos_array[idx].saturating_add(1);
                } else {
                    return false;
                }
            }
        }
    }
    true
}
