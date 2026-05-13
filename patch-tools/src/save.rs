use crate::cli::OutputFormat;
use crate::output::style;
use crate::types::{DaemonResponse, EngineEvent, daemon_response, engine_event};
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

pub fn preflight_save_target(requested: &Path) -> Result<SaveTarget> {
    let looks_like_dir = is_directory_like(requested);
    if looks_like_dir {
        std::fs::create_dir_all(requested).with_context(|| {
            format!(
                "failed to create --save directory `{}`",
                requested.display()
            )
        })?;
        return Ok(SaveTarget::Directory(requested.to_path_buf()));
    }
    if let Some(parent) = requested.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create parent directory for --save target `{}`",
                requested.display()
            )
        })?;
    }
    Ok(SaveTarget::File(requested.to_path_buf()))
}

pub fn run_post_execute_save(
    format: OutputFormat,
    target: &SaveTarget,
    execute_response: &DaemonResponse,
) -> Result<()> {
    let mut summary = SaveSummary {
        target: target.clone(),
        ..SaveSummary::default()
    };
    let saved_apks = match collect_saved_apks(execute_response) {
        Ok(saved_apks) => saved_apks,
        Err(error) => {
            return Err(fail_save(format, &mut summary, error.to_string()));
        }
    };
    summary.apks = saved_apks;

    if summary.apks.is_empty() {
        return Err(fail_save(
            format,
            &mut summary,
            "No patched APKs were produced by the execute response. Load at least one APK and return at least one patch before using --save.".to_string(),
        ));
    }

    if let SaveTarget::File(path) = target {
        let successful = summary.apks.iter().filter(|a| a.source_path.is_some()).count();
        if successful > 1 {
            return Err(fail_save(
                format,
                &mut summary,
                format!(
                    "--save={} expects a single output APK, but {successful} were produced. Pass a directory (e.g. `--save=dir/`) instead.",
                    path.display()
                ),
            ));
        }
    }

    copy_saved_apks(target, &mut summary);
    print_save_summary(format, &summary);

    if summary.has_failures() {
        bail!("run --save failed; see save summary above");
    }
    Ok(())
}

fn is_directory_like(path: &Path) -> bool {
    if path.is_dir() {
        return true;
    }
    let as_str = path.to_string_lossy();
    as_str.ends_with('/') || as_str.ends_with(std::path::MAIN_SEPARATOR)
}

fn collect_saved_apks(response: &DaemonResponse) -> Result<Vec<ApkSaveResult>> {
    match response.kind_ref()? {
        daemon_response::Kind::ExecutionResult(payload) => Ok(payload
            .events
            .iter()
            .filter_map(ApkSaveResult::from_save_event)
            .collect()),
        daemon_response::Kind::Error(payload) => bail!(
            "failed to read execute response for save: {}",
            payload.message
        ),
        _ => bail!("unexpected daemon response while reading execute result for save"),
    }
}

fn copy_saved_apks(target: &SaveTarget, summary: &mut SaveSummary) {
    for result in &mut summary.apks {
        let Some(source) = result.source_path.as_deref() else {
            continue;
        };
        if result.save_error.is_some() {
            continue;
        }

        let target_path = match target {
            SaveTarget::Directory(dir) => {
                let Some(file_name) = Path::new(source).file_name() else {
                    result.copy_result = CopyResult::Failed(format!(
                        "source path `{source}` has no file name component"
                    ));
                    continue;
                };
                dir.join(file_name)
            }
            SaveTarget::File(path) => path.clone(),
        };

        result.copy_result = match std::fs::copy(source, &target_path) {
            Ok(_) => CopyResult::Succeeded(target_path.display().to_string()),
            Err(error) => CopyResult::Failed(format!("{error:#}")),
        };
    }
}

fn fail_save(format: OutputFormat, summary: &mut SaveSummary, message: String) -> anyhow::Error {
    if !message.is_empty() {
        summary.workflow_error = Some(message);
    }
    print_save_summary(format, summary);
    anyhow::anyhow!("run --save failed; see save summary above")
}

#[derive(Clone)]
pub enum SaveTarget {
    Directory(PathBuf),
    File(PathBuf),
}

impl SaveTarget {
    fn display(&self) -> String {
        match self {
            Self::Directory(path) | Self::File(path) => path.display().to_string(),
        }
    }

    fn kind_label(&self) -> &'static str {
        match self {
            Self::Directory(_) => "directory",
            Self::File(_) => "file",
        }
    }
}

impl Default for SaveTarget {
    fn default() -> Self {
        Self::Directory(PathBuf::from("."))
    }
}

#[derive(Default)]
struct SaveSummary {
    target: SaveTarget,
    apks: Vec<ApkSaveResult>,
    workflow_error: Option<String>,
}

impl SaveSummary {
    fn has_failures(&self) -> bool {
        self.workflow_error.is_some() || self.apks.iter().any(ApkSaveResult::has_failure)
    }

    fn success_count(&self) -> usize {
        self.apks
            .iter()
            .filter(|apk| matches!(apk.copy_result, CopyResult::Succeeded(_)))
            .count()
    }

    fn failure_count(&self) -> usize {
        self.apks.iter().filter(|apk| apk.has_failure()).count()
    }
}

struct ApkSaveResult {
    apk_id: String,
    apk_label: String,
    item_id: Option<String>,
    source_path: Option<String>,
    save_error: Option<String>,
    copy_result: CopyResult,
}

impl ApkSaveResult {
    fn from_save_event(event: &EngineEvent) -> Option<Self> {
        match event.kind_ref() {
            engine_event::Kind::PatchedApkSaved(payload) => Some(Self {
                apk_label: payload
                    .apk_label
                    .clone()
                    .unwrap_or_else(|| payload.apk_id.clone()),
                apk_id: payload.apk_id.clone(),
                item_id: payload.item_id.clone(),
                source_path: Some(payload.apk_path.clone()),
                save_error: None,
                copy_result: CopyResult::NotAttempted,
            }),
            engine_event::Kind::PatchedApkSaveFailed(payload) => Some(Self {
                apk_label: payload
                    .apk_label
                    .clone()
                    .unwrap_or_else(|| payload.apk_id.clone()),
                apk_id: payload.apk_id.clone(),
                item_id: payload.item_id.clone(),
                source_path: None,
                save_error: Some(payload.error.clone()),
                copy_result: CopyResult::NotAttempted,
            }),
            _ => None,
        }
    }

    fn has_failure(&self) -> bool {
        self.save_error.is_some() || matches!(self.copy_result, CopyResult::Failed(_))
    }
}

enum CopyResult {
    NotAttempted,
    Succeeded(String),
    Failed(String),
}

fn print_save_summary(format: OutputFormat, summary: &SaveSummary) {
    match format {
        OutputFormat::Markdown => print_save_summary_markdown(summary),
        OutputFormat::Human => print_save_summary_human(summary),
    }
}

fn print_save_summary_markdown(summary: &SaveSummary) {
    println!("\n## Save Summary\n");
    println!(
        "- **Target ({} mode)**: `{}`",
        summary.target.kind_label(),
        summary.target.display()
    );
    println!("- **Succeeded**: {}", summary.success_count());
    println!("- **Failed**: {}", summary.failure_count());

    if let Some(error) = summary.workflow_error.as_deref() {
        println!("\n### Workflow Error\n");
        println!("```text\n{error}\n```");
    }

    for apk in &summary.apks {
        println!("\n### APK `{}`\n", apk.apk_label);
        println!("- **APK ID**: `{}`", apk.apk_id);

        if let Some(error) = apk.save_error.as_deref() {
            println!("```text\nSave failed: {error}\n```");
            continue;
        }

        if let Some(source) = apk.source_path.as_deref() {
            let item = apk.item_id.as_deref().unwrap_or("unknown item");
            println!("- **Source**: `{source}`");
            println!("- **Item**: `{item}`");
        }

        match &apk.copy_result {
            CopyResult::NotAttempted => println!("- **Copy**: not attempted"),
            CopyResult::Succeeded(target) => println!("- **Saved**: `{target}`"),
            CopyResult::Failed(error) => println!("```text\nCopy failed: {error}\n```"),
        }
    }
}

fn print_save_summary_human(summary: &SaveSummary) {
    println!();
    println!("{}", style::bold("Save summary"));
    println!(
        "  {} {} ({})",
        style::dimmed("target:"),
        summary.target.display(),
        summary.target.kind_label(),
    );
    println!(
        "  {} {}, {} {}",
        style::dimmed("succeeded:"),
        summary.success_count(),
        style::dimmed("failed:"),
        summary.failure_count(),
    );

    if let Some(error) = summary.workflow_error.as_deref() {
        println!("  {}", style::error("workflow error:"));
        for line in error.lines() {
            println!("    {line}");
        }
    }

    for apk in &summary.apks {
        println!("{}", style::bold(&apk.apk_label));
        println!("  {} {}", style::dimmed("apk id:"), apk.apk_id);

        if let Some(error) = apk.save_error.as_deref() {
            println!("  {} {}", style::error("save failed:"), error);
            continue;
        }

        if let Some(source) = apk.source_path.as_deref() {
            let item = apk.item_id.as_deref().unwrap_or("unknown item");
            println!("  {} {}", style::dimmed("source:"), source);
            println!("  {} {}", style::dimmed("item:"), item);
        }

        match &apk.copy_result {
            CopyResult::NotAttempted => {
                println!("  {}", style::warning("copy not attempted"));
            }
            CopyResult::Succeeded(target) => {
                println!("  {} {}", style::success("saved:"), target);
            }
            CopyResult::Failed(error) => {
                println!("  {}", style::error("copy failed:"));
                for line in error.lines() {
                    println!("    {line}");
                }
            }
        }
    }
}
