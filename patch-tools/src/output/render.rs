use crate::diff;
use crate::syndiff::SmaliSnippetKind;
use crate::types::{
    DaemonResponse, EngineEvent, EngineResult, daemon_response, engine_event, engine_result,
};
use itertools::Itertools;
use std::fmt::Write as _;

use super::formatters::Formatter;
use super::helpers::{
    class_change_kind_label, format_duration, indented_block, method_change_kind_label,
    resource_change_kind_label,
};
use super::inspect::{render_class_fingerprints, render_inspect};
use super::style;

#[derive(Default)]
pub(crate) struct RenderOutput {
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

impl RenderOutput {
    fn push_stdout(&mut self, text: impl AsRef<str>) {
        self.stdout.push_str(text.as_ref());
    }

    fn push_stderr(&mut self, text: impl AsRef<str>) {
        self.stderr.push_str(text.as_ref());
    }

    fn extend(&mut self, other: Self) {
        let Self { stdout, stderr } = other;
        self.stdout.push_str(&stdout);
        self.stderr.push_str(&stderr);
    }
}

pub(super) fn render_with(response: &DaemonResponse, fmt: &dyn Formatter) -> RenderOutput {
    match response
        .kind_ref()
        .expect("validated daemon response missing kind")
    {
        daemon_response::Kind::ApkLoaded(payload) => render_apk_loaded(payload, fmt),
        daemon_response::Kind::ApkUnloaded(_) => stdout_only(fmt.success("APK unloaded.")),
        daemon_response::Kind::StatusInfo(payload) => render_status_info(payload, fmt),
        daemon_response::Kind::ExecutionResult(payload) => {
            let mut output = RenderOutput::default();
            if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
                output.push_stdout(fmt.heading(1, "Execution results"));
            }
            output.extend(render_execution_result(&payload.events, fmt));
            output
        }
        daemon_response::Kind::FingerprintResult(payload) => {
            render_fingerprint_result(payload, fmt)
        }
        daemon_response::Kind::ClassFingerprintResult(payload) => {
            stdout_only(render_class_fingerprints(payload, fmt))
        }
        daemon_response::Kind::SearchResult(payload) => render_search_result(payload, fmt),
        daemon_response::Kind::MethodSmali(payload) => render_method_smali(payload, fmt),
        daemon_response::Kind::Ok(_) => stdout_only(fmt.success("OK")),
        daemon_response::Kind::Error(payload) => stderr_only(fmt.error(&payload.message)),
        daemon_response::Kind::InspectMethod(payload) => stdout_only(render_inspect(payload, fmt)),
    }
}

fn stdout_only(text: String) -> RenderOutput {
    RenderOutput {
        stdout: text,
        stderr: String::new(),
    }
}

fn stderr_only(text: String) -> RenderOutput {
    RenderOutput {
        stdout: String::new(),
        stderr: text,
    }
}

fn render_apk_loaded(
    payload: &crate::types::ApkLoadedResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();

    if let Some(identity) = payload.identity.as_ref() {
        if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
            output.push_stdout(fmt.heading(2, "APK loaded"));
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.bold("Package"),
                identity.package_name
            )));
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.bold("Version"),
                identity.package_version
            )));
            output.push_stdout(fmt.bullet(&format!(
                "{}: {}",
                fmt.bold("Path"),
                identity.source_file_path
            )));
        } else {
            output.push_stdout(format!(
                "{} {} {}\n",
                style::success("APK loaded:"),
                identity.package_name,
                identity.package_version,
            ));
        }
    } else {
        output.push_stdout(fmt.warning("APK already loaded or invalid."));
    }

    output
}

fn render_status_info(
    payload: &crate::types::StatusInfoResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();

    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        output.push_stdout(fmt.heading(2, "Daemon status"));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}",
            fmt.bold("Uptime"),
            format_duration(payload.uptime_secs)
        )));
        output.push_stdout(fmt.bullet(&format!(
            "{}: {}\n",
            fmt.bold("Loaded APKs"),
            payload.apks.len()
        )));
        for apk in &payload.apks {
            let identity = apk
                .identity
                .as_ref()
                .expect("validated apk status missing identity");
            output.push_stdout(format!(
                "  - {} {}\n    - Classes: {}\n    - Methods: {}\n    - Path: {}\n",
                identity.package_name,
                identity.package_version,
                apk.class_count,
                apk.method_count,
                identity.source_file_path,
            ));
        }
    } else {
        output.push_stdout(format!(
            "{} — uptime: {}, {} APK(s) loaded\n",
            style::bold("Daemon status"),
            format_duration(payload.uptime_secs),
            payload.apks.len(),
        ));
        for apk in &payload.apks {
            let identity = apk
                .identity
                .as_ref()
                .expect("validated apk status missing identity");
            output.push_stdout(format!(
                "  {} {} ({}, {})\n",
                identity.package_name,
                identity.package_version,
                style::cyan(&format!("{} classes", apk.class_count)),
                style::green(&format!("{} methods", apk.method_count)),
            ));
        }
    }

    output
}

fn render_fingerprint_result(
    payload: &crate::types::FingerprintResultResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();

    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        output.push_stdout(fmt.heading(2, "Fingerprints"));
        output.push_stdout(format!(
            "Generated {} fingerprint(s). Use the results below as-is; they already target unique matches.\n\n",
            payload.fingerprints.len()
        ));
        for (index, fingerprint) in payload.fingerprints.iter().enumerate() {
            output.push_stdout(fmt.heading(3, &format!("Fingerprint {}", index + 1)));
            output.push_stdout(fmt.code_block("kotlin", &fingerprint.morphe_code));
        }
    } else {
        output.push_stdout(format!(
            "{}\n\n",
            style::bold(&format!(
                "{} fingerprint(s). Use the results below as-is; they already target unique matches",
                payload.fingerprints.len()
            ))
        ));
        for (index, fingerprint) in payload.fingerprints.iter().enumerate() {
            output.push_stdout(format!("{}\n", style::cyan(&format!("#{}", index + 1))));
            output.push_stdout(fmt.code_block("", &fingerprint.morphe_code));
        }
    }

    output
}

fn render_search_result(
    payload: &crate::types::SearchResultResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);

    if is_md {
        output.push_stdout(fmt.heading(2, "Search results"));
    }
    for (apk_label, methods) in &payload.results {
        if is_md {
            output.push_stdout(fmt.heading(3, &format!("APK {apk_label}")));
            for method in &methods.items {
                output.push_stdout(fmt.bullet(&fmt.smali_method_id(method)));
            }
            output.push_stdout("\n");
        } else {
            output.push_stdout(format!("{}\n", style::bold(&format!("APK {apk_label}"))));
            for method in &methods.items {
                output.push_stdout(format!("  {}\n", fmt.smali_method_id(method)));
            }
        }
    }

    output
}

fn render_method_smali(
    payload: &crate::types::MethodSmaliResponse,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let Some(smali) = payload.smali.as_ref() else {
        return stdout_only(fmt.warning("Method not found."));
    };

    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        stdout_only(fmt.code_block("smali", smali))
    } else {
        stdout_only(format!("{smali}\n"))
    }
}

fn render_event(event: &EngineEvent, fmt: &dyn Formatter) -> RenderOutput {
    let mut output = RenderOutput::default();

    match event.kind_ref() {
        engine_event::Kind::RunCompleted(payload) => {
            if payload.total_items > 0 || payload.total_apks > 0 {
                if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
                    output.push_stdout(format!(
                        "---\n**Run complete**: {} item(s) across {} APK(s)\n",
                        payload.total_items, payload.total_apks
                    ));
                } else {
                    output.push_stdout(format!(
                        "\n{} {} item(s), {} APK(s)\n",
                        style::success("Done:"),
                        payload.total_items,
                        payload.total_apks,
                    ));
                }
            }
            if !payload.errors.is_empty() {
                if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
                    output.push_stdout("\n**Errors**:\n");
                    for error in &payload.errors {
                        output.push_stdout(format!("- {error}\n"));
                    }
                } else {
                    for error in &payload.errors {
                        output.push_stderr(format!(
                            "  {}\n",
                            style::error(&format!("Error: {error}"))
                        ));
                    }
                }
            }
        }
        engine_event::Kind::RunFailed(payload) => {
            if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
                output.push_stderr(format!("**Run failed**: {}\n", payload.error));
            } else {
                output.push_stderr(format!(
                    "{} {}\n",
                    style::error("Run failed:"),
                    payload.error
                ));
            }
        }
        engine_event::Kind::ItemResult(_) => {
            unreachable!("item results are handled by grouped execution formatting")
        }
        engine_event::Kind::ScriptOutput(_)
        | engine_event::Kind::PatchedApkSaved(_)
        | engine_event::Kind::PatchedApkSaveFailed(_) => {}
    }

    output
}

fn render_execution_result(events: &[EngineEvent], fmt: &dyn Formatter) -> RenderOutput {
    let mut output = RenderOutput::default();
    for block in build_execution_render_plan(events) {
        match block {
            ExecutionRenderBlock::ItemResults(entries) => {
                output.extend(render_item_result_groups(&entries, fmt));
            }
            ExecutionRenderBlock::ScriptOutput(entries) => {
                output.push_stdout(render_script_output(&entries, fmt));
            }
            ExecutionRenderBlock::Event(event) => {
                output.extend(render_event(event, fmt));
            }
        }
    }
    output
}

#[derive(Clone)]
struct ItemResultEntry {
    apk_id: String,
    item_id: String,
    result: EngineResult,
}

struct ItemApkGroup {
    apk_id: String,
    items: Vec<ItemGroup>,
}

struct ItemGroup {
    item_id: String,
    results: Vec<EngineResult>,
}

#[derive(Clone)]
struct ScriptOutputEntry {
    item_id: Option<String>,
    apk_label: Option<String>,
    text: String,
}

struct ScriptOutputGroup {
    item_id: Option<String>,
    apk_label: Option<String>,
    lines: Vec<String>,
}

enum ExecutionRenderBlock<'a> {
    ItemResults(Vec<ItemResultEntry>),
    ScriptOutput(Vec<ScriptOutputEntry>),
    Event(&'a EngineEvent),
}

fn build_execution_render_plan(events: &[EngineEvent]) -> Vec<ExecutionRenderBlock<'_>> {
    let mut blocks = Vec::new();
    let mut pending_item_results = Vec::new();
    let mut pending_script_output = Vec::new();
    let mut tail_events = Vec::new();

    for event in events {
        match event.kind_ref() {
            engine_event::Kind::ItemResult(item_result) => {
                if let Some(result) = item_result.result.as_ref() {
                    pending_item_results.push(ItemResultEntry {
                        apk_id: item_result.apk_id.clone(),
                        item_id: item_result.item_id.clone(),
                        result: result.clone(),
                    });
                }
            }
            engine_event::Kind::ScriptOutput(script_output) => {
                pending_script_output.push(ScriptOutputEntry {
                    item_id: script_output.item_id.clone(),
                    apk_label: script_output
                        .apk_label
                        .clone()
                        .or_else(|| script_output.apk_id.clone()),
                    text: script_output.text.clone(),
                });
            }
            engine_event::Kind::RunCompleted(_) => tail_events.push(event),
            _ => {
                flush_execution_render_buffers(
                    &mut blocks,
                    &mut pending_item_results,
                    &mut pending_script_output,
                );
                blocks.push(ExecutionRenderBlock::Event(event));
            }
        }
    }

    flush_execution_render_buffers(
        &mut blocks,
        &mut pending_item_results,
        &mut pending_script_output,
    );

    for event in tail_events {
        blocks.push(ExecutionRenderBlock::Event(event));
    }

    blocks
}

fn flush_execution_render_buffers(
    blocks: &mut Vec<ExecutionRenderBlock<'_>>,
    pending_item_results: &mut Vec<ItemResultEntry>,
    pending_script_output: &mut Vec<ScriptOutputEntry>,
) {
    if !pending_item_results.is_empty() {
        blocks.push(ExecutionRenderBlock::ItemResults(std::mem::take(
            pending_item_results,
        )));
    }

    if !pending_script_output.is_empty() {
        blocks.push(ExecutionRenderBlock::ScriptOutput(std::mem::take(
            pending_script_output,
        )));
    }
}

fn render_item_result_groups(entries: &[ItemResultEntry], fmt: &dyn Formatter) -> RenderOutput {
    let mut output = RenderOutput::default();
    if entries.is_empty() {
        return output;
    }

    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let mut first_apk = true;

    for apk_group in group_item_results(entries) {
        let ItemApkGroup { apk_id, items } = apk_group;
        if is_md {
            if !first_apk {
                output.push_stdout(fmt.separator());
            }
            first_apk = false;
            output.push_stdout(fmt.heading(2, &format!("APK `{apk_id}`")));
        } else {
            output.push_stdout(format!("{}\n", style::bold(&apk_id)));
        }
        let mut first_item = true;
        for ItemGroup { item_id, results } in items {
            if is_md {
                if !first_item {
                    output.push_stdout(fmt.separator());
                }
                first_item = false;
                output.push_stdout(fmt.heading(3, &item_id));
            } else {
                output.push_stdout(format!("  {}\n", style::bold(&item_id)));
            }
            for result in &results {
                output.extend(render_grouped_result(result, fmt));
            }
            if is_md {
                output.push_stdout("\n");
            }
        }
    }

    output
}

fn group_item_results(entries: &[ItemResultEntry]) -> Vec<ItemApkGroup> {
    let mut groups = Vec::<ItemApkGroup>::new();

    for entry in entries {
        let apk_index = groups
            .iter()
            .position(|group| group.apk_id == entry.apk_id)
            .unwrap_or_else(|| {
                groups.push(ItemApkGroup {
                    apk_id: entry.apk_id.clone(),
                    items: Vec::new(),
                });
                groups.len() - 1
            });

        let item_index = groups[apk_index]
            .items
            .iter()
            .position(|group| group.item_id == entry.item_id)
            .unwrap_or_else(|| {
                groups[apk_index].items.push(ItemGroup {
                    item_id: entry.item_id.clone(),
                    results: Vec::new(),
                });
                groups[apk_index].items.len() - 1
            });

        groups[apk_index].items[item_index]
            .results
            .push(entry.result.clone());
    }

    groups
}

fn render_grouped_result(result: &EngineResult, fmt: &dyn Formatter) -> RenderOutput {
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let mut output = match result.kind_ref() {
        engine_result::Kind::FingerprintMatches(payload) => {
            render_fingerprint_matches(payload, fmt)
        }
        engine_result::Kind::BytecodeDiffResult(payload) => render_bytecode_diff(payload, fmt),
        engine_result::Kind::ResourcePatchResult(payload)
            if payload.resource_changes.is_empty() =>
        {
            return render_empty_resource_changes(fmt);
        }
        engine_result::Kind::ResourcePatchResult(payload) => render_resource_patch(payload, fmt),
        engine_result::Kind::GenericResult(payload) => render_generic_result(payload, fmt),
        engine_result::Kind::ItemError(payload) => render_item_error(payload, fmt),
    };

    if !is_md {
        output.push_stdout("\n");
    }

    output
}

fn render_fingerprint_matches(
    payload: &crate::types::EngineResultFingerprintMatches,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);

    if payload.methods.is_empty() {
        if is_md {
            output.push_stdout("(no matches)\n\n");
        } else {
            output.push_stdout(format!("    {}\n", style::dimmed("No matches.")));
        }
        return output;
    }

    if is_md {
        output.push_stdout(fmt.heading(4, "Matches"));
    }
    for method in &payload.methods {
        if is_md {
            output.push_stdout(fmt.bullet(&fmt.code(&method.unique_id)));
        } else {
            output.push_stdout(format!(
                "    {}\n",
                fmt.method_id(
                    &method.defining_class,
                    &method.method_name,
                    &method.parameters,
                    &method.return_type,
                )
            ));
        }
    }
    if is_md {
        output.push_stdout("\n");
    }

    output
}

fn render_bytecode_diff(
    payload: &crate::types::EngineResultBytecodeDiffResult,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let mut rendered_any = false;

    for diff_entry in &payload.method_diffs {
        let diff_text = diff::render_smali_diff(
            &diff_entry.original_smali,
            &diff_entry.modified_smali,
            3,
            SmaliSnippetKind::Method,
            fmt.render_mode(),
        );
        if diff_text.is_empty() {
            continue;
        }
        rendered_any = true;
        output.push_stdout(fmt.labeled_diff_block(
            &diff_entry.method_id,
            method_change_kind_label(diff_entry),
            &diff_text,
        ));
    }

    for diff_entry in &payload.class_diffs {
        let diff_text = diff::render_smali_diff(
            &diff_entry.original_header,
            &diff_entry.modified_header,
            3,
            SmaliSnippetKind::ClassHeader,
            fmt.render_mode(),
        );
        if diff_text.is_empty() {
            continue;
        }
        rendered_any = true;
        output.push_stdout(fmt.labeled_diff_block(
            &diff_entry.class_type,
            class_change_kind_label(diff_entry),
            &diff_text,
        ));
    }

    if !rendered_any {
        output.push_stdout(fmt.no_changes_msg());
    }

    output
}

fn render_empty_resource_changes(fmt: &dyn Formatter) -> RenderOutput {
    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        stdout_only("(no resource changes)\n\n".to_string())
    } else {
        stdout_only(format!("    {}\n", style::dimmed("No resource changes.")))
    }
}

fn render_resource_patch(
    payload: &crate::types::EngineResultResourcePatchResult,
    fmt: &dyn Formatter,
) -> RenderOutput {
    let mut output = RenderOutput::default();
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);

    for change in &payload.resource_changes {
        let kind = resource_change_kind_label(change);
        if is_md {
            output.push_stdout(format!("#### `{}` ({})\n\n", change.relative_path, kind));
        } else {
            output.push_stdout(format!(
                "    {} ({})\n",
                style::cyan(&change.relative_path),
                kind
            ));
        }
        render_resource_change_content(change, fmt, &mut output);
    }

    output
}

fn render_resource_change_content(
    change: &crate::types::ResourceChangeDto,
    fmt: &dyn Formatter,
    output: &mut RenderOutput,
) {
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    match (
        change.original_content.as_ref(),
        change.modified_content.as_ref(),
    ) {
        (Some(original), Some(modified)) => {
            render_modified_resource(change, original, modified, fmt, output);
        }
        (None, Some(content)) if is_md => output.push_stdout(fmt.code_block("xml", content)),
        (Some(content), None) if is_md => {
            output.push_stdout("(deleted)\n");
            output.push_stdout(fmt.code_block("xml", content));
        }
        (None, None) if is_md => render_binary_resource_change(change, output),
        _ => {}
    }
}

fn render_modified_resource(
    change: &crate::types::ResourceChangeDto,
    original: &str,
    modified: &str,
    fmt: &dyn Formatter,
    output: &mut RenderOutput,
) {
    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let diff_text = if diff::is_xml_path(&change.relative_path) {
        diff::render_xml_diff(original, modified, 3, fmt.render_mode())
    } else if is_md {
        diff::render_markdown_diff(original, modified, 3)
    } else {
        diff::render_colored_diff(original, modified, 3)
    };

    if diff_text.is_empty() {
        return;
    }
    if is_md {
        output.push_stdout(fmt.diff_block(&diff_text));
    } else {
        output.push_stdout(indented_block(&diff_text, "    "));
    }
}

fn render_binary_resource_change(
    change: &crate::types::ResourceChangeDto,
    output: &mut RenderOutput,
) {
    if let (Some(original_hash), Some(modified_hash)) =
        (change.original_hash.as_ref(), change.modified_hash.as_ref())
    {
        output.push_stdout(format!(
            "Binary change: `{original_hash}` -> `{modified_hash}`\n\n"
        ));
    }
}

fn render_generic_result(
    payload: &crate::types::EngineResultGenericResult,
    fmt: &dyn Formatter,
) -> RenderOutput {
    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        stdout_only(format!(
            "**{}**:\n```\n{}\n```\n\n",
            payload.type_name, payload.text_representation
        ))
    } else {
        stdout_only(format!(
            "    {}: {}\n",
            style::bold(&payload.type_name),
            payload.text_representation,
        ))
    }
}

fn render_item_error(
    payload: &crate::types::EngineResultItemError,
    fmt: &dyn Formatter,
) -> RenderOutput {
    if matches!(fmt.render_mode(), diff::RenderMode::Markdown) {
        stderr_only(format!("**Error**: {}\n\n", payload.message))
    } else {
        stderr_only(format!(
            "    {}\n",
            style::error(&format!("Error: {}", payload.message))
        ))
    }
}

fn group_script_output(entries: &[ScriptOutputEntry]) -> Vec<ScriptOutputGroup> {
    entries
        .iter()
        .chunk_by(|entry| (entry.item_id.as_deref(), entry.apk_label.as_deref()))
        .into_iter()
        .map(|((item_id, apk_label), group)| ScriptOutputGroup {
            item_id: item_id.map(str::to_owned),
            apk_label: apk_label.map(str::to_owned),
            lines: group.map(|entry| entry.text.clone()).collect(),
        })
        .collect()
}

fn render_script_output(entries: &[ScriptOutputEntry], fmt: &dyn Formatter) -> String {
    if entries.is_empty() {
        return String::new();
    }

    let is_md = matches!(fmt.render_mode(), diff::RenderMode::Markdown);
    let mut output = String::new();

    if is_md {
        output.push_str(&fmt.heading(2, "Script output"));
    } else {
        writeln!(output, "\n{}", style::bold("Script output"))
            .expect("writing to a String cannot fail");
    }

    for group in group_script_output(entries) {
        let title = match (&group.item_id, &group.apk_label) {
            (Some(item_id), Some(apk_label)) => format!("{item_id} / {apk_label}"),
            _ => "Initial evaluation".to_string(),
        };
        if is_md {
            output.push_str(&fmt.heading(3, &title));
            output.push_str("```text\n");
            for line in &group.lines {
                output.push_str(line);
                output.push('\n');
            }
            output.push_str("```\n\n");
        } else {
            writeln!(output, "  {}", style::bold(&title)).expect("writing to a String cannot fail");
            for line in &group.lines {
                if line.is_empty() {
                    output.push('\n');
                } else {
                    writeln!(output, "    {line}").expect("writing to a String cannot fail");
                }
            }
            output.push('\n');
        }
    }

    output
}
