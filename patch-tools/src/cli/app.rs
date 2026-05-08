use super::{Cli, Commands, DaemonAction, OutputFormat};
use crate::daemon::{client::DaemonClient, runtime};
use crate::install as run_install;
use crate::output;
use crate::types::CommonFingerprintTargetSelector;
use anyhow::{Context, Result, bail};
use clap::CommandFactory;
use clap_complete::generate;
use std::{
    io,
    path::{Path, PathBuf},
};

pub async fn run(cli: Cli) -> Result<()> {
    let sock = runtime::socket_path()?;
    let format = cli.format;

    match cli.command {
        Commands::InternalDaemon => runtime::run(&sock).await?,
        Commands::Daemon { action } => handle_daemon_action(&sock, action, format).await?,
        Commands::Load { apk_path } => handle_load(&sock, apk_path, format).await?,
        Commands::Unload { apk } => handle_unload(&sock, apk, format).await?,
        Commands::Run {
            script_path,
            install: should_install,
            device,
        } => handle_run(&sock, script_path, should_install, device, format).await?,
        Commands::Scaffold => runtime::scaffold_scripts()?,
        Commands::Fingerprint { args, limit } => {
            let (apk, method_id) = split_optional_apk_selector(args, "method_id")?;
            handle_fingerprint(&sock, apk, method_id, limit, format).await?;
        }
        Commands::ClassFingerprint { args, limit } => {
            let (apk, class_id) = split_optional_apk_selector(args, "class_id")?;
            handle_class_fingerprint(&sock, apk, class_id, limit, format).await?;
        }
        Commands::CommonFingerprint { args, limit } => {
            let targets = split_common_fingerprint_targets(&args)?;
            handle_common_fingerprint(&sock, targets, limit, format).await?;
        }
        Commands::Search { query, limit } => {
            handle_search(&sock, query, limit, format).await?;
        }
        Commands::Map {
            old_apk,
            method_id,
            new_apk,
            limit,
        } => {
            handle_map(&sock, old_apk, method_id, new_apk, limit, format).await?;
        }
        Commands::Smali { args } => {
            let (apk, method_id) = split_optional_apk_selector(args, "method_id")?;
            handle_smali(&sock, apk, method_id, format).await?;
        }
        Commands::Completion { shell } => render_completion(shell),
    }

    Ok(())
}

async fn handle_daemon_action(
    sock: &Path,
    action: DaemonAction,
    format: OutputFormat,
) -> Result<()> {
    match action {
        DaemonAction::Start { apk } => runtime::start(sock, &apk).await,
        DaemonAction::Stop => {
            runtime::stop(sock).await?;
            println!("Daemon stopped");
            Ok(())
        }
        DaemonAction::Status => {
            let mut client = connect_daemon(sock).await?;
            let resp = client.status().await?;
            output::print_response_checked(&resp, format)?;
            Ok(())
        }
    }
}

async fn handle_load(sock: &Path, apk_path: PathBuf, format: OutputFormat) -> Result<()> {
    let path = apk_path
        .canonicalize()
        .context("Android package file not found")?;
    let mut client = connect_daemon(sock).await?;
    let resp = client.load_apk(path.to_string_lossy().as_ref()).await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_unload(sock: &Path, apk: Option<String>, format: OutputFormat) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let resp = client.unload_apk(apk).await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_run(
    sock: &Path,
    script_path: PathBuf,
    should_install: bool,
    device: Option<String>,
    format: OutputFormat,
) -> Result<()> {
    let path = script_path
        .canonicalize()
        .context("Script file not found")?;
    let install_device = if should_install {
        Some(
            run_install::preflight_install_device(device.as_deref())
                .context("run --install preflight failed")?,
        )
    } else {
        None
    };

    let mut client = connect_daemon(sock).await?;
    let resp = client
        .execute(path.to_string_lossy().as_ref(), None, should_install)
        .await?;
    output::print_response_checked(&resp, format)?;
    if let Some(resolved_device) = install_device.as_deref() {
        run_install::run_post_execute_install(format, resolved_device, &resp)?;
    }
    Ok(())
}

async fn handle_fingerprint(
    sock: &Path,
    apk: Option<String>,
    method_id: String,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let resp = client
        .generate_fingerprint(apk, &method_id, Some(limit))
        .await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_class_fingerprint(
    sock: &Path,
    apk: Option<String>,
    class_id: String,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let resp = client
        .generate_class_fingerprint(apk, &class_id, Some(limit))
        .await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_common_fingerprint(
    sock: &Path,
    targets: Vec<CommonFingerprintTargetSelector>,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let resp = client
        .generate_common_fingerprint(targets, Some(limit))
        .await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_search(
    sock: &Path,
    query: Vec<String>,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let query = query.join(" ");
    let resp = client.search_methods(&query, Some(limit)).await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_map(
    sock: &Path,
    old_apk: String,
    method_id: String,
    new_apk: String,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let resp = client
        .map_method(&old_apk, &method_id, &new_apk, Some(limit))
        .await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

async fn handle_smali(
    sock: &Path,
    apk: Option<String>,
    method_id: String,
    format: OutputFormat,
) -> Result<()> {
    let mut client = connect_daemon(sock).await?;
    let resp = client.get_method_smali(apk, &method_id).await?;
    output::print_response_checked(&resp, format)?;
    Ok(())
}

fn render_completion(shell: clap_complete::Shell) {
    let mut command = Cli::command();
    let binary_name = command.get_name().to_string();
    generate(shell, &mut command, binary_name, &mut io::stdout());
}

fn split_optional_apk_selector(
    args: Vec<String>,
    value_name: &str,
) -> Result<(Option<String>, String)> {
    let mut args = args.into_iter();
    let first = args
        .next()
        .with_context(|| format!("{value_name} argument missing"))?;
    let Some(second) = args.next() else {
        return Ok((None, first));
    };

    if let Some(extra) = args.next() {
        bail!("unexpected extra argument: {extra}");
    }

    Ok((Some(first), second))
}

fn split_common_fingerprint_targets(
    args: &[String],
) -> Result<Vec<CommonFingerprintTargetSelector>> {
    if args.len() < 4 || !args.len().is_multiple_of(2) {
        bail!(
            "common-fingerprint expects at least 2 APK/method pairs: <APK> <METHOD_ID> <APK> <METHOD_ID>..."
        );
    }

    Ok(args
        .chunks_exact(2)
        .map(|pair| CommonFingerprintTargetSelector {
            apk_id: pair[0].clone(),
            method_id: pair[1].clone(),
        })
        .collect())
}

async fn connect_daemon(sock: &Path) -> Result<DaemonClient> {
    DaemonClient::connect(sock)
        .await
        .context("daemon is not running")
}
