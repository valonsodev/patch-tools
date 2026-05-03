mod adb;
mod render;
mod summary;

use self::adb::{AdbClient, RealAdbClient, resolve_device};
use self::render::print_install_summary;
use self::summary::{InstallResult, InstallSummary, execution_saved_apks};
use crate::cli::OutputFormat;
use crate::types::{DaemonResponse, EngineEvent, daemon_response, engine_event};
use anyhow::{Result, bail};
use std::path::Path;

pub fn run_post_execute_install(
    format: &OutputFormat,
    resolved_device: &str,
    execute_response: &DaemonResponse,
) -> Result<()> {
    let saved_apks = execution_saved_apks(execute_response)?;
    let mut summary = InstallSummary::from_saved_apks(saved_apks);

    if let Err(error) = assess_execute_response_for_install(execute_response) {
        return Err(fail_install(format, &mut summary, error.to_string()));
    }

    if summary.apks.is_empty() {
        return Err(fail_install(
            format,
            &mut summary,
            "No patched APKs were produced by the execute response. Load at least one APK and return at least one patch before using --install.".to_string(),
        ));
    }

    if !summary.has_saved_apks() {
        return Err(fail_install(format, &mut summary, String::new()));
    }

    let mut adb = RealAdbClient::default();
    summary.resolved_device = Some(resolved_device.to_string());

    let current_user = match adb.current_user(resolved_device) {
        Ok(user_id) => user_id,
        Err(error) => {
            return Err(fail_install(format, &mut summary, error.to_string()));
        }
    };
    summary.current_user = Some(current_user);

    install_saved_apks(&mut adb, resolved_device, current_user, &mut summary);
    print_install_summary(format, &summary);

    if summary.has_failures() {
        bail!("run --install failed; see install summary above");
    }

    Ok(())
}

fn fail_install(
    format: &OutputFormat,
    summary: &mut InstallSummary,
    message: String,
) -> anyhow::Error {
    if !message.is_empty() {
        summary.workflow_error = Some(message);
    }
    print_install_summary(format, summary);
    anyhow::anyhow!("run --install failed; see install summary above")
}

pub fn preflight_install_device(requested_device: Option<&str>) -> Result<String> {
    let mut adb = RealAdbClient::default();
    resolve_device(&mut adb, requested_device)
}

fn install_saved_apks<C: AdbClient>(
    adb: &mut C,
    device: &str,
    current_user: u32,
    summary: &mut InstallSummary,
) {
    for result in &mut summary.apks {
        let Some(saved_apk_path) = result.saved_apk_path.as_deref() else {
            continue;
        };
        if result.save_error.is_some() {
            continue;
        }

        result.install_result = match adb.install(device, Path::new(saved_apk_path), current_user) {
            Ok(()) => InstallResult::Succeeded(format!("installed for user {current_user}")),
            Err(error) => InstallResult::Failed(format!("{error:#}")),
        };
    }
}

fn assess_execute_response_for_install(response: &DaemonResponse) -> Result<()> {
    match response.kind_ref()? {
        daemon_response::Kind::ExecutionResult(payload) => assess_execution_events(&payload.events),
        daemon_response::Kind::Error(payload) => bail!(
            "Skipping save/install because execution failed: {}",
            payload.message
        ),
        _ => bail!("Skipping save/install because execute returned an unexpected daemon response"),
    }
}

fn assess_execution_events(events: &[EngineEvent]) -> Result<()> {
    let mut run_failed = None;
    let mut run_completed = None;

    for event in events {
        match event.kind_ref() {
            engine_event::Kind::RunFailed(payload) => run_failed = Some(payload.error.clone()),
            engine_event::Kind::RunCompleted(payload) => {
                run_completed = Some((payload.total_items, payload.errors.clone()));
            }
            engine_event::Kind::ItemResult(_)
            | engine_event::Kind::ScriptOutput(_)
            | engine_event::Kind::PatchedApkSaved(_)
            | engine_event::Kind::PatchedApkSaveFailed(_) => {}
        }
    }

    if let Some(error) = run_failed {
        bail!("Skipping save/install because execution failed: {error}");
    }

    let Some((total_items, errors)) = run_completed else {
        bail!("Skipping save/install because execution did not report completion");
    };

    if total_items == 0 {
        bail!("Skipping save/install because the script produced no items to save");
    }

    if !errors.is_empty() {
        bail!(
            "Skipping save/install because the run completed with errors:\n- {}",
            errors.join("\n- ")
        );
    }

    Ok(())
}
