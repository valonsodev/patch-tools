use crate::types::{DaemonResponse, EngineEvent, daemon_response, engine_event};
use anyhow::{Result, bail};

pub(super) fn execution_saved_apks(response: &DaemonResponse) -> Result<Vec<ApkInstallResult>> {
    match response.kind_ref()? {
        daemon_response::Kind::ExecutionResult(payload) => Ok(payload
            .events
            .iter()
            .filter_map(ApkInstallResult::from_save_event)
            .collect()),
        daemon_response::Kind::Error(payload) => {
            bail!(
                "failed to read execute response for install: {}",
                payload.message
            )
        }
        _ => bail!("unexpected daemon response while reading execute result for install"),
    }
}

#[derive(Clone, Default)]
pub(super) struct InstallSummary {
    pub(super) apks: Vec<ApkInstallResult>,
    pub(super) resolved_device: Option<String>,
    pub(super) current_user: Option<u32>,
    pub(super) workflow_error: Option<String>,
}

impl InstallSummary {
    pub(super) fn from_saved_apks(saved_apks: Vec<ApkInstallResult>) -> Self {
        Self {
            apks: saved_apks,
            ..Self::default()
        }
    }

    pub(super) fn has_saved_apks(&self) -> bool {
        self.apks.iter().any(ApkInstallResult::save_succeeded)
    }

    pub(super) fn has_failures(&self) -> bool {
        self.workflow_error.is_some() || self.apks.iter().any(ApkInstallResult::has_failure)
    }

    pub(super) fn success_count(&self) -> usize {
        self.apks
            .iter()
            .filter(|apk| matches!(apk.install_result, InstallResult::Succeeded(_)))
            .count()
    }

    pub(super) fn failure_count(&self) -> usize {
        self.apks.iter().filter(|apk| apk.has_failure()).count()
    }
}

#[derive(Clone)]
pub(super) struct ApkInstallResult {
    pub(super) apk_id: String,
    pub(super) apk_label: String,
    pub(super) item_id: Option<String>,
    pub(super) saved_apk_path: Option<String>,
    pub(super) save_error: Option<String>,
    pub(super) install_result: InstallResult,
}

impl ApkInstallResult {
    fn from_save_event(event: &EngineEvent) -> Option<Self> {
        match event.kind_ref() {
            engine_event::Kind::PatchedApkSaved(payload) => Some(Self {
                apk_label: payload
                    .apk_label
                    .clone()
                    .unwrap_or_else(|| payload.apk_id.clone()),
                apk_id: payload.apk_id.clone(),
                item_id: payload.item_id.clone(),
                saved_apk_path: Some(payload.apk_path.clone()),
                save_error: None,
                install_result: InstallResult::NotAttempted,
            }),
            engine_event::Kind::PatchedApkSaveFailed(payload) => Some(Self {
                apk_label: payload
                    .apk_label
                    .clone()
                    .unwrap_or_else(|| payload.apk_id.clone()),
                apk_id: payload.apk_id.clone(),
                item_id: payload.item_id.clone(),
                saved_apk_path: None,
                save_error: Some(payload.error.clone()),
                install_result: InstallResult::NotAttempted,
            }),
            _ => None,
        }
    }

    pub(super) fn save_succeeded(&self) -> bool {
        self.saved_apk_path.is_some() && self.save_error.is_none()
    }

    pub(super) fn has_failure(&self) -> bool {
        self.save_error.is_some()
            || (self.save_succeeded()
                && !matches!(self.install_result, InstallResult::Succeeded(_)))
    }
}

#[derive(Clone)]
pub(super) enum InstallResult {
    NotAttempted,
    Succeeded(String),
    Failed(String),
}
