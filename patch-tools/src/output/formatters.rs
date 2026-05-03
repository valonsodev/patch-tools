use crate::diff;
use crate::types::MatchedMethodDto;
use std::fmt::Write as _;

use super::helpers::{format_human_method_id, indented_block, score_badge};
use super::style;
use crate::cli::OutputFormat;

pub(super) struct ScoreRow {
    pub(super) index: usize,
    pub(super) score: i32,
    pub(super) label: String,
}

pub(super) trait Formatter {
    fn heading(&self, level: u8, text: &str) -> String;
    fn separator(&self) -> String;
    fn bold(&self, text: &str) -> String;
    fn code(&self, text: &str) -> String;
    fn bullet(&self, text: &str) -> String;
    fn success(&self, text: &str) -> String;
    fn warning(&self, text: &str) -> String;
    fn error(&self, text: &str) -> String;
    fn code_block(&self, lang: &str, content: &str) -> String;
    fn diff_block(&self, text: &str) -> String;
    fn method_id(&self, class: &str, name: &str, params: &[String], ret: &str) -> String;
    fn matched_method_id(&self, method: &MatchedMethodDto) -> String;
    fn smali_method_id(&self, method: &crate::types::MethodInfoDto) -> String;
    fn render_mode(&self) -> diff::RenderMode;
    fn labeled_diff_block(&self, label: &str, kind: &str, diff_text: &str) -> String;
    fn no_changes_msg(&self) -> String;
    fn score_table(&self, rows: &[ScoreRow]) -> String;
}

pub(super) struct MarkdownFormatter;
pub(super) struct HumanFormatter;

impl Formatter for MarkdownFormatter {
    fn heading(&self, level: u8, text: &str) -> String {
        let hashes = "#".repeat(level as usize);
        format!("{hashes} {text}\n\n")
    }

    fn separator(&self) -> String {
        "---\n\n".to_string()
    }

    fn bold(&self, text: &str) -> String {
        format!("**{text}**")
    }

    fn code(&self, text: &str) -> String {
        format!("`{text}`")
    }

    fn bullet(&self, text: &str) -> String {
        format!("- {text}\n")
    }

    fn success(&self, text: &str) -> String {
        format!("{text}\n")
    }

    fn warning(&self, text: &str) -> String {
        format!("{text}\n")
    }

    fn error(&self, text: &str) -> String {
        format!("**Error**: {text}\n")
    }

    fn code_block(&self, lang: &str, content: &str) -> String {
        format!("```{lang}\n{content}\n```\n\n")
    }

    fn diff_block(&self, text: &str) -> String {
        format!("```diff\n{text}```\n\n")
    }

    fn method_id(&self, _class: &str, _name: &str, _params: &[String], _ret: &str) -> String {
        unreachable!()
    }

    fn matched_method_id(&self, method: &MatchedMethodDto) -> String {
        format!("`{}`", method.unique_id)
    }

    fn smali_method_id(&self, method: &crate::types::MethodInfoDto) -> String {
        format!("`{}`", method.unique_id)
    }

    fn render_mode(&self) -> diff::RenderMode {
        diff::RenderMode::Markdown
    }

    fn labeled_diff_block(&self, label: &str, kind: &str, diff_text: &str) -> String {
        format!("#### `{label}` ({kind})\n\n{}", self.diff_block(diff_text))
    }

    fn no_changes_msg(&self) -> String {
        "(no visible changes)\n\n".to_string()
    }

    fn score_table(&self, rows: &[ScoreRow]) -> String {
        let mut output = String::from(
            "| # | Score | Rating | Instruction |\n|--:|------:|--------|-------------|\n",
        );
        for row in rows {
            writeln!(
                output,
                "| {} | {} | {} | `{}` |",
                row.index,
                row.score,
                score_badge(row.score),
                row.label,
            )
            .expect("writing to a String cannot fail");
        }
        output.push('\n');
        output
    }
}

impl Formatter for HumanFormatter {
    fn heading(&self, _level: u8, text: &str) -> String {
        format!("{}\n", style::bold(text))
    }

    fn separator(&self) -> String {
        String::new()
    }

    fn bold(&self, text: &str) -> String {
        style::bold(text)
    }

    fn code(&self, text: &str) -> String {
        text.to_string()
    }

    fn bullet(&self, text: &str) -> String {
        format!("  {text}\n")
    }

    fn success(&self, text: &str) -> String {
        format!("{}\n", style::success(text))
    }

    fn warning(&self, text: &str) -> String {
        format!("{}\n", style::warning(text))
    }

    fn error(&self, text: &str) -> String {
        format!("{} {text}\n", style::error("Error:"))
    }

    fn code_block(&self, _lang: &str, content: &str) -> String {
        format!("{}\n\n", content.replace('\t', "    "))
    }

    fn diff_block(&self, text: &str) -> String {
        indented_block(text, "    ")
    }

    fn method_id(&self, class: &str, name: &str, params: &[String], ret: &str) -> String {
        format_human_method_id(class, name, params, ret)
    }

    fn matched_method_id(&self, method: &MatchedMethodDto) -> String {
        format_human_method_id(
            &method.defining_class,
            &method.method_name,
            &method.parameters,
            &method.return_type,
        )
    }

    fn smali_method_id(&self, method: &crate::types::MethodInfoDto) -> String {
        format_human_method_id(
            &method.defining_class,
            &method.name,
            &method.parameters,
            &method.return_type,
        )
    }

    fn render_mode(&self) -> diff::RenderMode {
        diff::RenderMode::Human
    }

    fn labeled_diff_block(&self, label: &str, kind: &str, diff_text: &str) -> String {
        format!(
            "    {}\n{}    {}\n",
            style::dimmed(&format!("----- BEGIN {label} ({kind}) -----")),
            indented_block(diff_text, "    "),
            style::dimmed(&format!("----- END {label} -----")),
        )
    }

    fn no_changes_msg(&self) -> String {
        format!("    {}\n", style::dimmed("No visible changes."))
    }

    fn score_table(&self, rows: &[ScoreRow]) -> String {
        let mut output = String::new();
        for row in rows {
            let score_str = format!("{:>3}", row.score);
            let colored_score = match row.score.cmp(&0) {
                std::cmp::Ordering::Less => style::green(&score_str),
                std::cmp::Ordering::Equal => style::dimmed(&score_str),
                std::cmp::Ordering::Greater => style::red(&score_str),
            };
            writeln!(
                output,
                "  {:>3}. [{}] {} {}",
                row.index,
                colored_score,
                score_badge(row.score),
                row.label,
            )
            .expect("writing to a String cannot fail");
        }
        output
    }
}

pub(super) fn formatter_for(format: &OutputFormat) -> &'static dyn Formatter {
    match format {
        OutputFormat::Markdown => &MarkdownFormatter,
        OutputFormat::Human => &HumanFormatter,
    }
}
