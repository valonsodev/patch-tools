use crate::diff;
use crate::types::{ClassFingerprintResultResponse, InspectMethodResponse};
use std::fmt::Write as _;

use super::formatters::{Formatter, ScoreRow};
use super::helpers::score_badge;
use super::style;

pub(super) fn render_inspect(payload: &InspectMethodResponse, fmt: &dyn Formatter) -> String {
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let mut output = String::new();

    if is_md {
        output.push_str(&fmt.heading(2, "Stability inspection"));
        write!(
            output,
            "{}: {}\n\n",
            fmt.bold("Method"),
            fmt.code(&payload.method_id)
        )
        .expect("writing to a String cannot fail");
    } else {
        write!(
            output,
            "{} {}\n\n",
            style::bold("Stability inspection:"),
            payload.method_id,
        )
        .expect("writing to a String cannot fail");
    }

    if let Some(ref sig) = payload.signature {
        if is_md {
            write!(
                output,
                "{}: return={}, access=`{:#06x}`, params=[{}]\n\n",
                fmt.bold("Signature"),
                fmt.code(&sig.return_type),
                sig.access_flags,
                sig.parameters
                    .iter()
                    .map(|parameter| fmt.code(parameter))
                    .collect::<Vec<_>>()
                    .join(", "),
            )
            .expect("writing to a String cannot fail");
        } else {
            writeln!(
                output,
                "  Signature: return={} access={:#06x} params=[{}]",
                sig.return_type,
                sig.access_flags,
                sig.parameters.join(", "),
            )
            .expect("writing to a String cannot fail");
        }
    }

    if is_md {
        write!(
            output,
            "{}: {} ({})\n\n",
            fmt.bold("Total stability score"),
            payload.total_stability_score,
            score_badge(payload.total_stability_score),
        )
        .expect("writing to a String cannot fail");
    } else {
        write!(
            output,
            "  Total score: {} ({})\n\n",
            payload.total_stability_score,
            score_badge(payload.total_stability_score),
        )
        .expect("writing to a String cannot fail");
    }

    if payload.scored_instructions.is_empty() {
        if is_md {
            output.push_str("_No instruction features._\n");
        }
        return output;
    }

    let rows = payload
        .scored_instructions
        .iter()
        .enumerate()
        .map(|(index, scored)| ScoreRow {
            index: index + 1,
            score: scored.stability_score,
            label: scored.label.clone(),
        })
        .collect::<Vec<_>>();
    output.push_str(&fmt.score_table(&rows));
    output
}

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
