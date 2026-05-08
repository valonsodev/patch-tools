use crate::types::{MethodDiffDto, class_diff_dto, method_diff_dto, resource_change_dto};

use super::style;

pub(super) fn format_human_method_id(
    defining_class: &str,
    method_name: &str,
    parameters: &[String],
    return_type: &str,
) -> String {
    let parameters = parameters
        .iter()
        .map(|parameter| style::green(parameter))
        .collect::<String>();

    format!(
        "{}{}{}{}{}{}{}",
        style::magenta(defining_class),
        style::yellow("->"),
        style::cyan(method_name),
        style::yellow("("),
        parameters,
        style::yellow(")"),
        style::red(return_type),
    )
}

pub(super) fn method_change_kind_label(diff: &MethodDiffDto) -> &'static str {
    match diff.change_kind_enum() {
        method_diff_dto::ChangeKind::Unspecified => "UNSPECIFIED",
        method_diff_dto::ChangeKind::Modified => "MODIFIED",
        method_diff_dto::ChangeKind::Added => "ADDED",
        method_diff_dto::ChangeKind::Deleted => "DELETED",
    }
}

pub(super) fn class_change_kind_label(diff: &crate::types::ClassDiffDto) -> &'static str {
    match diff.change_kind_enum() {
        class_diff_dto::ChangeKind::Unspecified => "UNSPECIFIED",
        class_diff_dto::ChangeKind::Added => "ADDED",
        class_diff_dto::ChangeKind::Modified => "MODIFIED",
    }
}

pub(super) fn resource_change_kind_label(change: &crate::types::ResourceChangeDto) -> &'static str {
    match change.kind_enum() {
        resource_change_dto::Kind::Unspecified => "UNSPECIFIED",
        resource_change_dto::Kind::Added => "ADDED",
        resource_change_dto::Kind::Modified => "MODIFIED",
        resource_change_dto::Kind::Deleted => "DELETED",
    }
}

pub(super) fn format_duration(secs: u64) -> String {
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
    }
}

pub(super) fn indented_block(text: &str, prefix: &str) -> String {
    let mut output = String::new();
    for line in text.lines() {
        if line.is_empty() {
            output.push('\n');
        } else {
            output.push_str(prefix);
            output.push_str(line);
            output.push('\n');
        }
    }
    output
}
