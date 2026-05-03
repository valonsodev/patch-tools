use adb_client::{
    ADBDeviceExt,
    server::{ADBServer, DeviceState},
};
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

pub(super) trait AdbClient {
    fn eligible_devices(&mut self) -> Result<Vec<String>>;
    fn current_user(&mut self, serial: &str) -> Result<u32>;
    fn install(&mut self, serial: &str, apk_path: &Path, user: u32) -> Result<()>;
}

#[derive(Debug, Default)]
pub(super) struct RealAdbClient {
    server: ADBServer,
}

impl AdbClient for RealAdbClient {
    fn eligible_devices(&mut self) -> Result<Vec<String>> {
        let devices = self
            .server
            .devices()
            .context("failed to query adb devices")?;
        Ok(devices
            .into_iter()
            .filter(|device| device.state == DeviceState::Device)
            .map(|device| device.identifier)
            .collect())
    }

    fn current_user(&mut self, serial: &str) -> Result<u32> {
        let mut device = self
            .server
            .get_device_by_name(serial)
            .with_context(|| format!("failed to connect to adb device `{serial}`"))?;
        let mut stdout = Vec::new();
        let mut stderr = Vec::new();
        let exit_status = device
            .shell_command(&"am get-current-user", Some(&mut stdout), Some(&mut stderr))
            .with_context(|| format!("failed to run `am get-current-user` on `{serial}`"))?;

        let stdout = String::from_utf8_lossy(&stdout).into_owned();
        let stderr = String::from_utf8_lossy(&stderr).into_owned();

        if let Some(status) = exit_status
            && status != 0
        {
            bail!(format_shell_failure(
                serial,
                "am get-current-user",
                Some(status),
                &stdout,
                &stderr,
            ));
        }

        parse_current_user(&stdout).with_context(|| {
            format!(
                "unexpected output from `am get-current-user` on `{serial}`\nstdout:\n{}\nstderr:\n{}",
                normalize_output(&stdout),
                normalize_output(&stderr),
            )
        })
    }

    fn install(&mut self, serial: &str, apk_path: &Path, user: u32) -> Result<()> {
        let user = user.to_string();
        let output = Command::new("adb")
            .arg("-s")
            .arg(serial)
            .arg("install")
            .arg("--user")
            .arg(user.as_str())
            .arg(apk_path)
            .output()
            .with_context(|| {
                format!(
                    "failed to invoke `adb install` for device `{serial}`, user `{user}`, apk `{}`",
                    apk_path.display(),
                )
            })?;

        if output.status.success() {
            return Ok(());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let exit = output.status.code().map_or_else(
            || "terminated by signal".to_string(),
            |code| code.to_string(),
        );
        bail!(
            "adb install failed for device `{serial}`, user `{user}`, apk `{}`\nexit status: {exit}\nstdout:\n{}\nstderr:\n{}",
            apk_path.display(),
            normalize_output(&stdout),
            normalize_output(&stderr),
        )
    }
}

pub(super) fn resolve_device<C: AdbClient>(
    adb: &mut C,
    requested_device: Option<&str>,
) -> Result<String> {
    let eligible_devices = adb.eligible_devices()?;

    if let Some(serial) = requested_device {
        if eligible_devices.iter().any(|device| device == serial) {
            return Ok(serial.to_string());
        }

        let eligible = if eligible_devices.is_empty() {
            "none".to_string()
        } else {
            eligible_devices.join(", ")
        };
        bail!(
            "ADB device '{serial}' is not connected or not in device state. Eligible devices: {eligible}."
        )
    }

    match eligible_devices.as_slice() {
        [device] => Ok(device.clone()),
        [] => bail!(
            "No eligible adb devices are connected. Connect exactly one device or pass --device SERIAL."
        ),
        devices => bail!(
            "Multiple eligible adb devices are connected: {}. Pass --device SERIAL.",
            devices.join(", ")
        ),
    }
}

fn parse_current_user(output: &str) -> Result<u32> {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        bail!("expected a single integer user id, got empty output");
    }
    if trimmed.lines().count() != 1 || !trimmed.chars().all(|ch| ch.is_ascii_digit()) {
        bail!("expected a single integer user id, got `{trimmed}`");
    }

    trimmed
        .parse::<u32>()
        .with_context(|| format!("failed to parse current user id from `{trimmed}`"))
}

fn format_shell_failure(
    serial: &str,
    command: &str,
    exit_status: Option<u8>,
    stdout: &str,
    stderr: &str,
) -> String {
    let exit = exit_status.map_or_else(|| "unknown".to_string(), |status| status.to_string());
    format!(
        "Command failed on `{serial}`: `{command}`\nexit status: {exit}\nstdout:\n{}\nstderr:\n{}",
        normalize_output(stdout),
        normalize_output(stderr),
    )
}

fn normalize_output(output: &str) -> &str {
    let trimmed = output.trim();
    if trimmed.is_empty() {
        "<empty>"
    } else {
        trimmed
    }
}
