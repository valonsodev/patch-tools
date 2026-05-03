use crate::types::{
    InstructionFeature, InstructionFieldAccess, InstructionMethodCall, instruction_feature,
};
use itertools::Itertools;

pub(crate) fn expand_variants(
    instructions: &[&InstructionFeature],
) -> Vec<Vec<InstructionFeature>> {
    if instructions.is_empty() {
        return vec![Vec::new()];
    }

    let variants_per: Vec<Vec<InstructionFeature>> = instructions
        .iter()
        .map(|instr| match instr.kind_ref() {
            instruction_feature::Kind::MethodCall(kind)
                if kind.same_defining_class && !kind.use_this_defining_class =>
            {
                vec![
                    (*instr).clone(),
                    method_call_feature(
                        instr.index,
                        kind.defining_class.clone(),
                        true,
                        true,
                        kind.name.clone(),
                        kind.parameters.clone(),
                        kind.return_type.clone(),
                    ),
                ]
            }
            instruction_feature::Kind::FieldAccess(kind)
                if kind.same_defining_class && !kind.use_this_defining_class =>
            {
                vec![
                    (*instr).clone(),
                    field_access_feature(
                        instr.index,
                        kind.defining_class.clone(),
                        true,
                        true,
                        kind.name.clone(),
                        kind.field_type.clone(),
                    ),
                ]
            }
            _ => vec![(*instr).clone()],
        })
        .collect();

    variants_per.into_iter().multi_cartesian_product().collect()
}

fn method_call_feature(
    index: u32,
    defining_class: String,
    same_defining_class: bool,
    use_this_defining_class: bool,
    name: String,
    parameters: Vec<String>,
    return_type: String,
) -> InstructionFeature {
    InstructionFeature {
        index,
        kind: Some(instruction_feature::Kind::MethodCall(
            InstructionMethodCall {
                defining_class,
                same_defining_class,
                use_this_defining_class,
                name,
                parameters,
                return_type,
            },
        )),
    }
}

fn field_access_feature(
    index: u32,
    defining_class: String,
    same_defining_class: bool,
    use_this_defining_class: bool,
    name: String,
    field_type: String,
) -> InstructionFeature {
    InstructionFeature {
        index,
        kind: Some(instruction_feature::Kind::FieldAccess(
            InstructionFieldAccess {
                defining_class,
                same_defining_class,
                use_this_defining_class,
                name,
                field_type,
            },
        )),
    }
}
