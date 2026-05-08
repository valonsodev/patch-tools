use crate::diff;
use crate::types::ClassFingerprintResultResponse;
use std::fmt::Write as _;

use super::formatters::Formatter;
use super::style;

pub(super) fn render_class_fingerprints(
    payload: &ClassFingerprintResultResponse,
    fmt: &dyn Formatter,
) -> String {
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let mut output = String::new();

    if is_md {
        output.push_str(&fmt.heading(2, "Class Fingerprints"));
        output.push_str(&fmt.bullet(&format!(
            "{}: {}\n",
            fmt.bold("Class"),
            fmt.code(&payload.class_id)
        )));
        write!(
            output,
            "Generated {} class fingerprint(s), ranked from best to worst.\n\n",
            payload.fingerprints.len()
        )
        .expect("writing to a String cannot fail");
    } else {
        write!(
            output,
            "{}\n  {}\n\n",
            style::bold("Class fingerprints"),
            style::magenta(&payload.class_id),
        )
        .expect("writing to a String cannot fail");
    }

    for (index, candidate) in payload.fingerprints.iter().enumerate() {
        if is_md {
            output.push_str(&fmt.heading(3, &format!("Class Fingerprint {}", index + 1)));
            if let Some(method) = candidate.source_method.as_ref() {
                output.push_str(
                    &fmt.bullet(&format!("Source method: {}\n", fmt.code(&method.unique_id))),
                );
            }
            if let Some(fingerprint) = candidate.fingerprint.as_ref() {
                output.push_str(&fmt.code_block("kotlin", &fingerprint.morphe_code));
            }
        } else {
            writeln!(output, "{}", style::cyan(&format!("#{}", index + 1)))
                .expect("writing to a String cannot fail");
            if let Some(method) = candidate.source_method.as_ref() {
                writeln!(
                    output,
                    "  {} {}",
                    style::dimmed("source:"),
                    fmt.matched_method_id(method)
                )
                .expect("writing to a String cannot fail");
            }
            if let Some(fingerprint) = candidate.fingerprint.as_ref() {
                output.push_str(&fmt.code_block("", &fingerprint.morphe_code));
            }
        }
    }

    output
}
