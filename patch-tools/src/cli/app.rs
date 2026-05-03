use super::{Cli, Commands, DaemonAction, OutputFormat};
use crate::daemon::{client::DaemonClient, runtime};
use crate::install as run_install;
use crate::output;
use anyhow::{Context, Result};
use clap::CommandFactory;
use clap_complete::generate;
use std::{
    future::Future,
    io,
    path::{Path, PathBuf},
    pin::Pin,
};

pub async fn run(cli: Cli) -> Result<()> {
    let sock = runtime::socket_path()?;
    let format = cli.format;

    match cli.command {
        Commands::InternalDaemon => runtime::run(&sock).await?,
        Commands::Daemon { action } => handle_daemon_action(&sock, action, format.clone()).await?,
        Commands::Load { apk_path } => handle_load(&sock, apk_path, format.clone()).await?,
        Commands::Unload { apk } => handle_unload(&sock, apk, format.clone()).await?,
        Commands::Run {
            script_path,
            install: should_install,
            device,
        } => handle_run(&sock, script_path, should_install, device, format.clone()).await?,
        Commands::Scaffold => runtime::scaffold_scripts()?,
        Commands::Fingerprint {
            apk,
            method_id,
            limit,
        } => handle_fingerprint(&sock, apk, method_id, limit, format.clone()).await?,
        Commands::ClassFingerprint {
            apk,
            class_id,
            limit,
        } => handle_class_fingerprint(&sock, apk, class_id, limit, format.clone()).await?,
        Commands::Search { query, limit } => {
            handle_search(&sock, query, limit, format.clone()).await?;
        }
        Commands::Smali { apk, method_id } => {
            handle_smali(&sock, apk, method_id, format.clone()).await?;
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
            with_client(sock, |client| {
                Box::pin(async move {
                    let resp = client.status().await?;
                    output::print_response_checked(&resp, &format)?;
                    Ok(())
                })
            })
            .await
        }
    }
}

async fn handle_load(sock: &Path, apk_path: PathBuf, format: OutputFormat) -> Result<()> {
    let path = apk_path
        .canonicalize()
        .context("Android package file not found")?;
    with_client(sock, |client| {
        Box::pin(async move {
            let resp = client.load_apk(path.to_string_lossy().as_ref()).await?;
            output::print_response_checked(&resp, &format)?;
            Ok(())
        })
    })
    .await
}

async fn handle_unload(sock: &Path, apk: String, format: OutputFormat) -> Result<()> {
    with_client(sock, |client| {
        Box::pin(async move {
            let resp = client.unload_apk(&apk).await?;
            output::print_response_checked(&resp, &format)?;
            Ok(())
        })
    })
    .await
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

    with_client(sock, |client| {
        Box::pin(async move {
            let resp = client
                .execute(path.to_string_lossy().as_ref(), None, should_install)
                .await?;
            output::print_response_checked(&resp, &format)?;
            if let Some(resolved_device) = install_device.as_deref() {
                run_install::run_post_execute_install(&format, resolved_device, &resp)?;
            }
            Ok(())
        })
    })
    .await
}

async fn handle_fingerprint(
    sock: &Path,
    apk: String,
    method_id: String,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    with_client(sock, |client| {
        Box::pin(async move {
            let resp = client
                .generate_fingerprint(&apk, &method_id, Some(limit))
                .await?;
            output::print_response_checked(&resp, &format)?;
            Ok(())
        })
    })
    .await
}

async fn handle_class_fingerprint(
    sock: &Path,
    apk: String,
    class_id: String,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    with_client(sock, |client| {
        Box::pin(async move {
            let resp = client
                .generate_class_fingerprint(&apk, &class_id, Some(limit))
                .await?;
            output::print_response_checked(&resp, &format)?;
            Ok(())
        })
    })
    .await
}

async fn handle_search(
    sock: &Path,
    query: Vec<String>,
    limit: u32,
    format: OutputFormat,
) -> Result<()> {
    with_client(sock, |client| {
        Box::pin(async move {
            let query = query.join(" ");
            let resp = client.search_methods(&query, Some(limit)).await?;
            output::print_response_checked(&resp, &format)?;
            Ok(())
        })
    })
    .await
}

async fn handle_smali(
    sock: &Path,
    apk: String,
    method_id: String,
    format: OutputFormat,
) -> Result<()> {
    with_client(sock, |client| {
        Box::pin(async move {
            let resp = client.get_method_smali(&apk, &method_id).await?;
            output::print_response_checked(&resp, &format)?;
            Ok(())
        })
    })
    .await
}

fn render_completion(shell: clap_complete::Shell) {
    let mut command = Cli::command();
    let binary_name = command.get_name().to_string();
    generate(shell, &mut command, binary_name, &mut io::stdout());
}

type ClientFuture<'a> = Pin<Box<dyn Future<Output = Result<()>> + 'a>>;

async fn with_client<F>(sock: &Path, handler: F) -> Result<()>
where
    F: for<'a> FnOnce(&'a mut DaemonClient) -> ClientFuture<'a>,
{
    let mut client = connect_daemon(sock).await?;
    handler(&mut client).await
}

async fn connect_daemon(sock: &Path) -> Result<DaemonClient> {
    DaemonClient::connect(sock)
        .await
        .context("daemon is not running")
}
