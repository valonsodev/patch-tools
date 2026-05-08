use super::{client::DaemonClient, server};
use crate::cli::OutputFormat;
use crate::output;
use crate::types::daemon_response;
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const MAIN_KTS_TEMPLATE: &str = include_str!(concat!(env!("OUT_DIR"), "/main.kts"));
const AGENTS_MD_TEMPLATE: &str = include_str!(concat!(env!("OUT_DIR"), "/AGENTS.md"));
const DAEMON_SOCKET_FILE: &str = ".patch-tools.sock";
const DAEMON_START_LOG_FILE: &str = "patch-tools.log";

/// Start daemon as a child process by re-executing self with internal-daemon subcommand.
pub async fn start(sock: &Path, apks: &[PathBuf]) -> Result<()> {
    let exe = std::env::current_exe().context("cannot find current executable")?;
    let log_path = start_log_path()?;
    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .open(&log_path)
        .with_context(|| format!("failed to open daemon log file {}", log_path.display()))?;

    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("internal-daemon");
    configure_detached_daemon(&mut cmd);

    cmd.stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::from(
            log_file
                .try_clone()
                .with_context(|| format!("failed to clone {}", log_path.display()))?,
        ))
        .stderr(std::process::Stdio::from(log_file));

    let mut child = cmd.spawn().context("failed to start daemon")?;
    println!("Daemon starting (pid: {})", child.id());

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        if std::time::Instant::now() > deadline {
            anyhow::bail!("Daemon did not start within 30 seconds");
        }
        if let Some(status) = child
            .try_wait()
            .context("failed to poll daemon process state")?
        {
            let startup_log = std::fs::read_to_string(&log_path)
                .ok()
                .map(|contents| contents.trim().to_string())
                .filter(|contents| !contents.is_empty());
            let log_message = startup_log.as_deref().map_or_else(
                || format!("\nDaemon startup log file: {}", log_path.display()),
                |contents| format!("\nDaemon startup log ({}):\n{contents}", log_path.display()),
            );
            anyhow::bail!(
                "Daemon exited early with status: {status}{log_message}\nThe daemon may be running inside a sandbox that blocks Unix socket binding or detached child processes."
            );
        }
        if is_expected_daemon_ready(sock, child.id()).await? {
            println!("Daemon ready at {}", sock.display());
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    if !apks.is_empty() {
        println!("Preloading {} APK(s)...", apks.len());
        for apk in apks {
            let path = apk
                .canonicalize()
                .with_context(|| format!("APK not found: {}", apk.display()))?;
            let mut client = DaemonClient::connect(sock).await?;
            let resp = client.load_apk(path.to_string_lossy().as_ref()).await?;
            output::print_response_checked(&resp, OutputFormat::Human)?;
        }
    }

    Ok(())
}

/// Ask the daemon to stop and wait until its socket is removed.
pub async fn stop(sock: &Path) -> Result<()> {
    let mut client = DaemonClient::connect(sock).await?;
    let response = client.stop().await?;
    if let daemon_response::Kind::Error(payload) = response.kind_ref()? {
        anyhow::bail!("daemon returned stop error: {}", payload.message);
    }

    wait_for_socket_removal(sock, std::time::Duration::from_secs(30)).await
}

/// Run the daemon in-process (called by internal-daemon subcommand).
pub async fn run(sock: &Path) -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env().add_directive("morphe=info".parse()?),
        )
        .init();

    tracing::info!("Starting daemon at {}", sock.display());
    server::run(sock).await?;
    Ok(())
}

pub fn socket_path() -> Result<PathBuf> {
    runtime_dir().map(|dir| dir.join(DAEMON_SOCKET_FILE))
}

pub fn scaffold_scripts() -> Result<()> {
    let cwd = std::env::current_dir().context("cannot determine current directory")?;
    let main_path = cwd.join("main.kts");
    let agents_path = cwd.join("AGENTS.md");

    let existing = [&main_path, &agents_path]
        .into_iter()
        .filter(|path| path.exists())
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();

    if !existing.is_empty() {
        anyhow::bail!(
            "refusing to overwrite existing file(s): {}",
            existing.join(", ")
        );
    }

    std::fs::write(&main_path, MAIN_KTS_TEMPLATE)
        .with_context(|| format!("failed to write {}", main_path.display()))?;
    std::fs::write(&agents_path, AGENTS_MD_TEMPLATE)
        .with_context(|| format!("failed to write {}", agents_path.display()))?;

    println!("Created {}", main_path.display());
    println!("Created {}", agents_path.display());
    println!(
        "Edit main.kts and run `patch-tools run main.kts` when the daemon has an APK loaded."
    );

    Ok(())
}

fn start_log_path() -> Result<PathBuf> {
    runtime_dir().map(|dir| dir.join(DAEMON_START_LOG_FILE))
}

fn runtime_dir() -> Result<PathBuf> {
    std::env::current_dir().context("cannot determine current directory for daemon runtime files")
}

async fn is_expected_daemon_ready(sock: &Path, expected_pid: u32) -> Result<bool> {
    let Ok(mut client) = DaemonClient::connect(sock).await else {
        return Ok(false);
    };

    let Ok(response) = client.status().await else {
        return Ok(false);
    };

    match response.kind_ref()? {
        daemon_response::Kind::StatusInfo(status) => Ok(status.daemon_pid == expected_pid),
        daemon_response::Kind::Error(payload) => {
            anyhow::bail!("daemon returned startup error: {}", payload.message)
        }
        other => anyhow::bail!("unexpected daemon startup response: {other:?}"),
    }
}

async fn wait_for_socket_removal(sock: &Path, timeout: std::time::Duration) -> Result<()> {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        if !sock.exists() {
            return Ok(());
        }
        if std::time::Instant::now() > deadline {
            anyhow::bail!(
                "daemon did not finish shutting down within {} seconds",
                timeout.as_secs()
            );
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Detach the daemon from the launching shell so it survives the parent exiting.
///
/// `setsid()` creates a new session and process group with no controlling
/// terminal. `Command::process_group(0)` (stable in std) only sets the pgid —
/// the child would still be associated with the parent's session and would
/// receive SIGHUP when the shell exits. We need the full session split, which
/// requires `unsafe { setsid() }` from `pre_exec` (after fork, before exec).
#[cfg(unix)]
fn configure_detached_daemon(cmd: &mut std::process::Command) {
    use std::os::unix::process::CommandExt;

    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
}

#[cfg(not(unix))]
fn configure_detached_daemon(_cmd: &mut std::process::Command) {}
