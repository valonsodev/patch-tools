use super::summary::{InstallResult, InstallSummary};
use crate::cli::OutputFormat;
use crate::output::style;

pub(super) fn print_install_summary(format: &OutputFormat, summary: &InstallSummary) {
    match format {
        OutputFormat::Markdown => print_install_summary_markdown(summary),
        OutputFormat::Human => print_install_summary_human(summary),
    }
}

fn print_install_summary_markdown(summary: &InstallSummary) {
    println!("\n## Install Summary\n");

    if let Some(device) = summary.resolved_device.as_deref() {
        println!("- **Device**: `{device}`");
    }
    if let Some(user_id) = summary.current_user {
        println!("- **Current User**: `{user_id}`");
    }
    println!("- **Succeeded**: {}", summary.success_count());
    println!("- **Failed**: {}", summary.failure_count());

    if let Some(error) = summary.workflow_error.as_deref() {
        println!("\n### Workflow Error\n");
        println!("```text\n{error}\n```");
    }

    for apk in &summary.apks {
        println!("\n### APK `{}`\n", apk.apk_label);
        println!("- **APK ID**: `{}`", apk.apk_id);

        match apk.save_error.as_deref() {
            Some(error) => {
                println!("```text\nSave failed: {error}\n```");
                continue;
            }
            None => {
                if let Some(path) = apk.saved_apk_path.as_deref() {
                    let item = apk.item_id.as_deref().unwrap_or("unknown item");
                    println!("- **Saved**: `{path}`");
                    println!("- **Item**: `{item}`");
                }
            }
        }

        match &apk.install_result {
            InstallResult::NotAttempted => {
                println!("- **Install**: not attempted");
            }
            InstallResult::Succeeded(message) => {
                println!("- **Install**: {message}");
            }
            InstallResult::Failed(error) => {
                println!("```text\nInstall failed: {error}\n```");
            }
        }
    }
}

fn print_install_summary_human(summary: &InstallSummary) {
    println!();
    println!("{}", style::bold("Install summary"));
    if let Some(device) = summary.resolved_device.as_deref() {
        println!("  {} {}", style::dimmed("device:"), device);
    }
    if let Some(user_id) = summary.current_user {
        println!("  {} {}", style::dimmed("current user:"), user_id);
    }
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

        if let Some(path) = apk.saved_apk_path.as_deref() {
            let item = apk.item_id.as_deref().unwrap_or("unknown item");
            println!("  {} {}", style::dimmed("saved:"), path);
            println!("  {} {}", style::dimmed("item:"), item);
        }

        match &apk.install_result {
            InstallResult::NotAttempted => {
                println!("  {}", style::warning("install not attempted"));
            }
            InstallResult::Succeeded(message) => {
                println!("  {} {}", style::success("install:"), message);
            }
            InstallResult::Failed(error) => {
                println!("  {}", style::error("install failed:"));
                for line in error.lines() {
                    println!("    {line}");
                }
            }
        }
    }
}
