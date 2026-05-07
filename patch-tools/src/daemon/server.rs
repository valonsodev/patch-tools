use crate::daemon;
use crate::engine_jni::EngineJni;
use crate::fingerprint::{ClassFingerprintCandidate, FingerprintIndex};
use crate::search;
use crate::types::{
    ApkIdentity, ApkLoadedResponse, ApkStatus, ApkUnloadedResponse, ClassFingerprintCandidateDto,
    ClassFingerprintResultResponse, DaemonRequest, DaemonResponse, ExecutionResultResponse,
    FingerprintResultResponse, MatchedMethodDto, MethodData, MethodFingerprintDto, MethodInfoDto,
    MethodInfoList, MethodMapCandidateDto, MethodMapResponse, MethodSmaliResponse,
    SearchResultResponse, StatusInfoResponse, daemon_request, daemon_response,
};
use anyhow::{Context, Result};
use nucleo_matcher::Utf32String;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UnixListener;
use tokio::sync::{RwLock, mpsc, oneshot};
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Per-APK data stored on the Rust side after JNI bulk export.
pub struct ApkData {
    pub identity: ApkIdentity,
    pub status: ApkStatus,
    pub method_infos: Vec<MethodInfoDto>,
    pub search_haystacks: Vec<Utf32String>,
    pub fingerprint_index: FingerprintIndex,
}

/// Read-mostly state shared across request handlers.
struct SharedState {
    apks: HashMap<String, Arc<ApkData>>,
    start_time: Instant,
    daemon_pid: u32,
}

// =============================================================================
// Engine worker — serializes all JNI access onto a dedicated OS thread
// =============================================================================

struct LoadedApk {
    identity: ApkIdentity,
    apk: ApkData,
}

type LoadApkReply = Result<Option<LoadedApk>>;

enum EngineCommand {
    LoadApk {
        path: String,
        reply: oneshot::Sender<LoadApkReply>,
    },
    UnloadApk {
        apk_id: String,
        reply: oneshot::Sender<Result<()>>,
    },
    EvaluateScript {
        script_path: String,
        cap: i32,
        save_patched_apks: bool,
        reply: oneshot::Sender<Result<ExecutionResultResponse>>,
    },
    GetMethodSmali {
        apk_id: String,
        method_id: String,
        reply: oneshot::Sender<Result<Option<String>>>,
    },
    Close {
        reply: oneshot::Sender<Result<()>>,
    },
}

#[derive(Clone)]
struct EngineHandle {
    tx: mpsc::Sender<EngineCommand>,
}

impl EngineHandle {
    async fn load_apk(&self, path: String) -> Result<Option<LoadedApk>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::LoadApk {
                path,
                reply: reply_tx,
            })
            .await
            .context("engine worker gone")?;
        reply_rx.await.context("engine worker gone")?
    }

    async fn unload_apk(&self, apk_id: String) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::UnloadApk {
                apk_id,
                reply: reply_tx,
            })
            .await
            .context("engine worker gone")?;
        reply_rx.await.context("engine worker gone")?
    }

    async fn evaluate_script(
        &self,
        script_path: String,
        cap: i32,
        save_patched_apks: bool,
    ) -> Result<ExecutionResultResponse> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::EvaluateScript {
                script_path,
                cap,
                save_patched_apks,
                reply: reply_tx,
            })
            .await
            .context("engine worker gone")?;
        reply_rx.await.context("engine worker gone")?
    }

    async fn get_method_smali(&self, apk_id: String, method_id: String) -> Result<Option<String>> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::GetMethodSmali {
                apk_id,
                method_id,
                reply: reply_tx,
            })
            .await
            .context("engine worker gone")?;
        reply_rx.await.context("engine worker gone")?
    }

    async fn close(&self) -> Result<()> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.tx
            .send(EngineCommand::Close { reply: reply_tx })
            .await
            .context("engine worker gone")?;
        reply_rx.await.context("engine worker gone")?
    }
}

fn spawn_engine_worker(engine: EngineJni) -> EngineHandle {
    let (tx, mut rx) = mpsc::channel::<EngineCommand>(32);

    std::thread::spawn(move || {
        while let Some(cmd) = rx.blocking_recv() {
            match cmd {
                EngineCommand::LoadApk { path, reply } => {
                    let result = engine.load_apk(&path).and_then(|identity| match identity {
                        Some(identity) => {
                            let methods = engine.get_apk_method_data(&identity.id)?;
                            let status = engine.get_apk_status(&identity.id)?;
                            let apk = build_apk_data(identity.clone(), status, methods)?;
                            Ok(Some(LoadedApk { identity, apk }))
                        }
                        None => Ok(None),
                    });
                    let _ = reply.send(result);
                }
                EngineCommand::UnloadApk { apk_id, reply } => {
                    let _ = reply.send(engine.unload_apk(&apk_id));
                }
                EngineCommand::EvaluateScript {
                    script_path,
                    cap,
                    save_patched_apks,
                    reply,
                } => {
                    let _ =
                        reply.send(engine.evaluate_script(&script_path, cap, save_patched_apks));
                }
                EngineCommand::GetMethodSmali {
                    apk_id,
                    method_id,
                    reply,
                } => {
                    let _ = reply.send(engine.get_method_smali(&apk_id, &method_id));
                }
                EngineCommand::Close { reply } => {
                    let result = engine.close();
                    let _ = reply.send(result);
                    return;
                }
            }
        }

        let _ = engine.close();
    });

    EngineHandle { tx }
}

// =============================================================================
// Server
// =============================================================================

/// Run the daemon Unix socket server, dispatching protobuf requests.
pub async fn run(engine: EngineJni, socket_path: &Path) -> Result<()> {
    let _ = std::fs::remove_file(socket_path);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let engine_tx = spawn_engine_worker(engine);
    let state = Arc::new(RwLock::new(SharedState {
        apks: HashMap::new(),
        start_time: Instant::now(),
        daemon_pid: std::process::id(),
    }));

    let listener = UnixListener::bind(socket_path).context("failed to bind unix socket")?;

    tracing::info!("Listening on {}", socket_path.display());

    let shutdown = CancellationToken::new();
    let tracker = TaskTracker::new();
    let shutdown_signal = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        tracing::info!("Received shutdown signal");
        shutdown_signal.cancel();
    });

    loop {
        tokio::select! {
            () = shutdown.cancelled() => break,
            accept_result = listener.accept() => {
                let (stream, _addr) = accept_result?;
                tracing::debug!("New connection");

                let state = Arc::clone(&state);
                let engine_tx = engine_tx.clone();
                let shutdown = shutdown.clone();
                tracker.spawn(async move {
                    if let Err(error) = handle_connection(stream, &state, &engine_tx, shutdown).await {
                        tracing::error!("Connection error: {error:#}");
                    }
                });
            }
        }
    }

    tracker.close();
    tracker.wait().await;
    engine_tx.close().await?;
    let _ = std::fs::remove_file(socket_path);
    tracing::info!("Daemon shut down");
    Ok(())
}

async fn handle_connection(
    stream: tokio::net::UnixStream,
    state: &Arc<RwLock<SharedState>>,
    engine_tx: &EngineHandle,
    shutdown: CancellationToken,
) -> Result<()> {
    let (mut reader, mut writer) = stream.into_split();

    loop {
        let request = tokio::select! {
            () = shutdown.cancelled() => break,
            request = daemon::read_request(&mut reader) => request?,
        };
        let Some(request) = request else {
            break;
        };
        let is_stop = matches!(request.kind_ref()?, daemon_request::Kind::Stop(_));
        let response = dispatch(state, engine_tx, request).await;

        daemon::write_response(&mut writer, &response).await?;

        if is_stop {
            tracing::info!("Stop command processed, shutting down");
            shutdown.cancel();
            break;
        }
    }

    Ok(())
}

async fn dispatch(
    state: &Arc<RwLock<SharedState>>,
    engine_tx: &EngineHandle,
    request: DaemonRequest,
) -> DaemonResponse {
    let request = match request.into_kind() {
        Ok(request) => request,
        Err(error) => return DaemonResponse::error(format!("{error:#}")),
    };

    match request {
        daemon_request::Kind::LoadApk(load_apk) => {
            handle_load_apk(state, engine_tx, load_apk).await
        }
        daemon_request::Kind::UnloadApk(unload_apk) => {
            handle_unload_apk(state, engine_tx, unload_apk).await
        }
        daemon_request::Kind::Execute(execute) => handle_execute(engine_tx, execute).await,
        daemon_request::Kind::GenerateFingerprint(generate) => {
            handle_generate_fingerprint(state, generate).await
        }
        daemon_request::Kind::GenerateClassFingerprint(generate) => {
            handle_generate_class_fingerprint(state, generate).await
        }
        daemon_request::Kind::SearchMethods(search_methods) => {
            handle_search_methods(state, search_methods).await
        }
        daemon_request::Kind::MapMethod(map_method) => handle_map_method(state, map_method).await,
        daemon_request::Kind::GetMethodSmali(get_method_smali) => {
            handle_get_method_smali(state, engine_tx, get_method_smali).await
        }
        daemon_request::Kind::Status(_) => handle_status(state).await,
        daemon_request::Kind::InspectMethod(inspect) => handle_inspect_method(state, inspect).await,

        daemon_request::Kind::Stop(_) => DaemonResponse::ok(),
    }
}

async fn resolve_loaded_apk(
    state: &Arc<RwLock<SharedState>>,
    apk_selector: &str,
) -> std::result::Result<Arc<ApkData>, String> {
    let state = state.read().await;
    let resolved_apk_id = resolve_apk_selector(&state, apk_selector)?;
    state
        .apks
        .get(&resolved_apk_id)
        .cloned()
        .ok_or_else(|| format_apk_not_found_error(&state, apk_selector))
}

async fn handle_load_apk(
    state: &Arc<RwLock<SharedState>>,
    engine_tx: &EngineHandle,
    load_apk: crate::types::LoadApkRequest,
) -> DaemonResponse {
    match engine_tx.load_apk(load_apk.path).await {
        Ok(Some(loaded_apk)) => {
            let identity = loaded_apk.identity;
            tracing::info!("Loaded APK {}", identity.package_name);
            let mut state = state.write().await;
            state
                .apks
                .insert(identity.id.clone(), Arc::new(loaded_apk.apk));
            DaemonResponse {
                kind: Some(daemon_response::Kind::ApkLoaded(ApkLoadedResponse {
                    identity: Some(identity),
                })),
            }
        }
        Ok(None) => DaemonResponse {
            kind: Some(daemon_response::Kind::ApkLoaded(ApkLoadedResponse {
                identity: None,
            })),
        },
        Err(error) => DaemonResponse::error(format!("{error:#}")),
    }
}

async fn handle_unload_apk(
    state: &Arc<RwLock<SharedState>>,
    engine_tx: &EngineHandle,
    unload_apk: crate::types::UnloadApkRequest,
) -> DaemonResponse {
    let resolved_apk_id = {
        let state = state.read().await;
        match resolve_apk_selector(&state, &unload_apk.apk_id) {
            Ok(apk_id) => apk_id,
            Err(error) => return DaemonResponse::error(error),
        }
    };

    match engine_tx.unload_apk(resolved_apk_id.clone()).await {
        Ok(()) => {
            let mut state = state.write().await;
            state.apks.remove(&resolved_apk_id);
            DaemonResponse {
                kind: Some(daemon_response::Kind::ApkUnloaded(ApkUnloadedResponse {})),
            }
        }
        Err(error) => DaemonResponse::error(format!("{error:#}")),
    }
}

async fn handle_execute(
    engine_tx: &EngineHandle,
    execute: crate::types::ExecuteRequest,
) -> DaemonResponse {
    let raw_cap = execute.fingerprint_result_cap.unwrap_or(15);
    let Ok(cap) = i32::try_from(raw_cap) else {
        return DaemonResponse::error(format!(
            "fingerprint_result_cap exceeds i32::MAX: {raw_cap}"
        ));
    };

    match engine_tx
        .evaluate_script(execute.script_path, cap, execute.save_patched_apks)
        .await
    {
        Ok(payload) => DaemonResponse {
            kind: Some(daemon_response::Kind::ExecutionResult(payload)),
        },
        Err(error) => DaemonResponse::error(format!("{error:#}")),
    }
}

async fn handle_generate_fingerprint(
    state: &Arc<RwLock<SharedState>>,
    generate: crate::types::GenerateFingerprintRequest,
) -> DaemonResponse {
    let limit = generate.limit.unwrap_or(8) as usize;
    let apk = match resolve_loaded_apk(state, &generate.apk_id).await {
        Ok(apk) => apk,
        Err(error) => return DaemonResponse::error(error),
    };

    match resolve_method_id(&apk, &generate.method_id) {
        Some(unique_id) => match tokio::task::spawn_blocking(move || {
            generate_fingerprint_response(&apk, &unique_id, limit)
        })
        .await
        {
            Ok(response) => response,
            Err(error) => blocking_task_error("fingerprint generation", &error),
        },
        None => DaemonResponse::error(format!("Method not found: {}", generate.method_id)),
    }
}

fn generate_fingerprint_response(
    apk: &Arc<ApkData>,
    unique_id: &str,
    limit: usize,
) -> DaemonResponse {
    let target_method = apk
        .method_infos
        .iter()
        .find(|method| method.unique_id == unique_id);
    match crate::fingerprint::generate_all(&apk.fingerprint_index, unique_id, limit) {
        Ok(fingerprints) => DaemonResponse {
            kind: Some(daemon_response::Kind::FingerprintResult(
                FingerprintResultResponse { fingerprints },
            )),
        },
        Err(error) => {
            let message = format!("{error:#}");
            if message.contains("Could not distinguish target method") {
                match crate::fingerprint::best_class_fingerprint_for_method(
                    &apk.fingerprint_index,
                    unique_id,
                ) {
                    Ok(class_fingerprint) => match crate::fingerprint::generate_class_scoped(
                        &apk.fingerprint_index,
                        unique_id,
                        limit,
                    ) {
                        Ok(mut fingerprints) => {
                            crate::fingerprint::apply_class_fingerprint_to_results(
                                &mut fingerprints,
                                &class_fingerprint.fingerprint,
                            );
                            DaemonResponse {
                                kind: Some(daemon_response::Kind::FingerprintResult(
                                    FingerprintResultResponse { fingerprints },
                                )),
                            }
                        }
                        Err(fallback_error) => {
                            DaemonResponse::error(format_fingerprint_generation_error(
                                &message,
                                Some(&format!("{fallback_error:#}")),
                                target_method,
                                Some(&class_fingerprint.fingerprint),
                            ))
                        }
                    },
                    Err(fallback_error) => {
                        DaemonResponse::error(format_fingerprint_generation_error(
                            &message,
                            Some(&format!("{fallback_error:#}")),
                            target_method,
                            None,
                        ))
                    }
                }
            } else {
                DaemonResponse::error(message)
            }
        }
    }
}

async fn handle_generate_class_fingerprint(
    state: &Arc<RwLock<SharedState>>,
    generate: crate::types::GenerateClassFingerprintRequest,
) -> DaemonResponse {
    let limit = generate.limit.unwrap_or(8) as usize;
    let apk = match resolve_loaded_apk(state, &generate.apk_id).await {
        Ok(apk) => apk,
        Err(error) => return DaemonResponse::error(error),
    };
    let resolved_class_id = match resolve_class_id(&apk, &generate.class_id) {
        Ok(class_id) => class_id,
        Err(error) => return DaemonResponse::error(error),
    };

    match tokio::task::spawn_blocking(move || {
        generate_class_fingerprint_response(&apk, resolved_class_id, limit)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => blocking_task_error("class fingerprint generation", &error),
    }
}

fn generate_class_fingerprint_response(
    apk: &Arc<ApkData>,
    resolved_class_id: String,
    limit: usize,
) -> DaemonResponse {
    match crate::fingerprint::generate_class_fingerprints(
        &apk.fingerprint_index,
        &resolved_class_id,
        limit,
    ) {
        Ok(fingerprints) => DaemonResponse {
            kind: Some(daemon_response::Kind::ClassFingerprintResult(
                ClassFingerprintResultResponse {
                    class_id: resolved_class_id,
                    fingerprints: fingerprints
                        .into_iter()
                        .map(|candidate| {
                            to_class_fingerprint_candidate(candidate, &apk.method_infos)
                        })
                        .collect(),
                },
            )),
        },
        Err(error) => DaemonResponse::error(format!("{error:#}")),
    }
}

async fn handle_inspect_method(
    state: &Arc<RwLock<SharedState>>,
    inspect: crate::types::InspectMethodRequest,
) -> DaemonResponse {
    let apk = match resolve_loaded_apk(state, &inspect.apk_id).await {
        Ok(apk) => apk,
        Err(error) => return DaemonResponse::error(error),
    };

    match resolve_method_id(&apk, &inspect.method_id) {
        Some(unique_id) => {
            match crate::fingerprint::inspect_stability(&apk.fingerprint_index, &unique_id) {
                Ok(result) => DaemonResponse {
                    kind: Some(daemon_response::Kind::InspectMethod(result)),
                },
                Err(error) => DaemonResponse::error(format!("{error:#}")),
            }
        }
        None => DaemonResponse::error(format!("Method not found: {}", inspect.method_id)),
    }
}

async fn handle_search_methods(
    state: &Arc<RwLock<SharedState>>,
    search_methods: crate::types::SearchMethodsRequest,
) -> DaemonResponse {
    let apks: Vec<Arc<ApkData>> = {
        let state = state.read().await;
        state.apks.values().cloned().collect()
    };
    let limit = search_methods.limit.unwrap_or(8) as usize;
    let query = search_methods.query;

    match tokio::task::spawn_blocking(move || search_methods_response(&apks, &query, limit)).await {
        Ok(response) => response,
        Err(error) => blocking_task_error("method search", &error),
    }
}

fn search_methods_response(apks: &[Arc<ApkData>], query: &str, limit: usize) -> DaemonResponse {
    let results = search::search_all_apks(
        apks.iter().map(|apk| {
            (
                &apk.identity,
                apk.method_infos.as_slice(),
                apk.search_haystacks.as_slice(),
            )
        }),
        query,
        limit,
    )
    .into_iter()
    .map(|(apk_id, methods)| (apk_id, MethodInfoList { items: methods }))
    .collect();

    DaemonResponse {
        kind: Some(daemon_response::Kind::SearchResult(SearchResultResponse {
            results,
        })),
    }
}

async fn handle_map_method(
    state: &Arc<RwLock<SharedState>>,
    map_method: crate::types::MapMethodRequest,
) -> DaemonResponse {
    let limit = map_method.limit.unwrap_or(8) as usize;
    let (source_apk, target_apk) = {
        let state = state.read().await;
        if state.apks.len() < 2 {
            return DaemonResponse::error(format!(
                "map requires at least 2 loaded APKs. Currently loaded: {}",
                state.apks.len()
            ));
        }

        let source_apk_id = match resolve_apk_selector(&state, &map_method.old_apk_id) {
            Ok(apk_id) => apk_id,
            Err(error) => return DaemonResponse::error(error),
        };
        let target_apk_id = match resolve_apk_selector(&state, &map_method.new_apk_id) {
            Ok(apk_id) => apk_id,
            Err(error) => return DaemonResponse::error(error),
        };

        if source_apk_id == target_apk_id {
            return DaemonResponse::error(
                "map requires two different APKs; source and target resolved to the same APK",
            );
        }

        let Some(source_apk) = state.apks.get(&source_apk_id).cloned() else {
            return DaemonResponse::error(format_apk_not_found_error(
                &state,
                &map_method.old_apk_id,
            ));
        };
        let Some(target_apk) = state.apks.get(&target_apk_id).cloned() else {
            return DaemonResponse::error(format_apk_not_found_error(
                &state,
                &map_method.new_apk_id,
            ));
        };

        (source_apk, target_apk)
    };

    let source_method_id = match resolve_method_id(&source_apk, &map_method.method_id) {
        Some(unique_id) => unique_id,
        None => {
            return DaemonResponse::error(format!("Method not found: {}", map_method.method_id));
        }
    };

    match tokio::task::spawn_blocking(move || {
        map_method_response(&source_apk, &source_method_id, &target_apk, limit)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => blocking_task_error("method map", &error),
    }
}

fn map_method_response(
    source_apk: &Arc<ApkData>,
    source_method_id: &str,
    target_apk: &Arc<ApkData>,
    limit: usize,
) -> DaemonResponse {
    let Some(source_method) = source_apk
        .method_infos
        .iter()
        .find(|method| method.unique_id == source_method_id)
        .cloned()
    else {
        return DaemonResponse::error(format!("Method not found: {source_method_id}"));
    };

    let candidates = match crate::fingerprint::map_methods(
        &source_apk.fingerprint_index,
        source_method_id,
        &target_apk.fingerprint_index,
        limit,
    ) {
        Ok(candidates) => candidates,
        Err(error) => return DaemonResponse::error(format!("{error:#}")),
    };

    let method_infos_by_id = target_apk
        .method_infos
        .iter()
        .map(|method| (method.unique_id.as_str(), method))
        .collect::<HashMap<_, _>>();
    let candidates = candidates
        .into_iter()
        .filter_map(|candidate| {
            method_infos_by_id
                .get(candidate.method_id.as_str())
                .map(|method| MethodMapCandidateDto {
                    method: Some((*method).clone()),
                    similarity: candidate.similarity,
                })
        })
        .collect();

    DaemonResponse {
        kind: Some(daemon_response::Kind::MethodMap(MethodMapResponse {
            source_apk: Some(source_apk.identity.clone()),
            target_apk: Some(target_apk.identity.clone()),
            source_method: Some(source_method),
            candidates,
        })),
    }
}

fn blocking_task_error(action: &str, error: &tokio::task::JoinError) -> DaemonResponse {
    DaemonResponse::error(format!("{action} task failed: {error}"))
}

async fn handle_get_method_smali(
    state: &Arc<RwLock<SharedState>>,
    engine_tx: &EngineHandle,
    get_method_smali: crate::types::GetMethodSmaliRequest,
) -> DaemonResponse {
    let resolved_apk_id = {
        let state = state.read().await;
        match resolve_apk_selector(&state, &get_method_smali.apk_id) {
            Ok(apk_id) => apk_id,
            Err(error) => return DaemonResponse::error(error),
        }
    };

    match engine_tx
        .get_method_smali(resolved_apk_id, get_method_smali.method_id)
        .await
    {
        Ok(smali) => DaemonResponse {
            kind: Some(daemon_response::Kind::MethodSmali(MethodSmaliResponse {
                smali,
            })),
        },
        Err(error) => DaemonResponse::error(format!("{error:#}")),
    }
}

async fn handle_status(state: &Arc<RwLock<SharedState>>) -> DaemonResponse {
    let state = state.read().await;
    let uptime_secs = state.start_time.elapsed().as_secs();
    let apks: Vec<ApkStatus> = state.apks.values().map(|apk| apk.status.clone()).collect();

    DaemonResponse {
        kind: Some(daemon_response::Kind::StatusInfo(StatusInfoResponse {
            apks,
            uptime_secs,
            daemon_pid: state.daemon_pid,
        })),
    }
}

fn build_apk_data(
    identity: ApkIdentity,
    status: ApkStatus,
    methods: Vec<MethodData>,
) -> Result<ApkData> {
    let status_identity = status
        .identity
        .as_ref()
        .context("loaded apk status missing identity")?;
    if status_identity.id != identity.id {
        anyhow::bail!(
            "loaded apk status id mismatch: expected {}, got {}",
            identity.id,
            status_identity.id
        );
    }

    let method_infos = methods
        .iter()
        .map(|method| method.info_ref().clone())
        .collect::<Vec<_>>();
    let search_haystacks = search::build_search_index(&method_infos);
    let fingerprint_index = crate::fingerprint::build_index(methods);

    Ok(ApkData {
        identity,
        status,
        method_infos,
        search_haystacks,
        fingerprint_index,
    })
}

fn resolve_apk_selector(state: &SharedState, raw: &str) -> std::result::Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return resolve_implicit_apk_selector(state);
    }

    if state.apks.contains_key(normalized) {
        return Ok(normalized.to_string());
    }

    let mut exact_matches = Vec::new();
    let mut package_matches = Vec::new();

    for (apk_id, apk) in &state.apks {
        let identity = &apk.identity;
        let package = identity.package_name.as_str();
        let version = identity.package_version.as_str();
        let selectors = [
            format!("{package} / {version}"),
            format!("{package}/{version}"),
            format!("{package}@{version}"),
            format!("{package}:{version}"),
            format!("{package} {version}"),
        ];

        if selectors.iter().any(|selector| selector == normalized) {
            exact_matches.push((apk_id.clone(), apk_display_name(identity)));
        } else if package == normalized {
            package_matches.push((apk_id.clone(), apk_display_name(identity)));
        }
    }

    match exact_matches.len() {
        1 => return Ok(exact_matches.remove(0).0),
        n if n > 1 => {
            return Err(format!(
                "APK selector '{}' is ambiguous. Matches: {}",
                normalized,
                exact_matches
                    .into_iter()
                    .map(|(_, label)| label)
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        _ => {}
    }

    match package_matches.len() {
        1 => Ok(package_matches.remove(0).0),
        n if n > 1 => Err(format!(
            "APK selector '{}' matches multiple versions. Use 'package / version'. Matches: {}",
            normalized,
            package_matches
                .into_iter()
                .map(|(_, label)| label)
                .collect::<Vec<_>>()
                .join(", ")
        )),
        _ => Err(format_apk_not_found_error(state, normalized)),
    }
}

fn resolve_implicit_apk_selector(state: &SharedState) -> std::result::Result<String, String> {
    match state.apks.len() {
        0 => Err("APK selector omitted, but no APKs are loaded".to_string()),
        1 => Ok(state
            .apks
            .keys()
            .next()
            .expect("one APK exists")
            .to_string()),
        _ => Err(format!(
            "APK selector is required when multiple APKs are loaded. Available apks: {}",
            available_apk_labels(state)
        )),
    }
}

fn format_apk_not_found_error(state: &SharedState, raw: &str) -> String {
    format!(
        "APK not found: {raw}. Available apks: {}",
        available_apk_labels(state)
    )
}

fn available_apk_labels(state: &SharedState) -> String {
    let mut labels = state
        .apks
        .values()
        .map(|apk| apk_display_name(&apk.identity))
        .collect::<Vec<_>>();
    labels.sort();

    if labels.is_empty() {
        "none loaded".to_string()
    } else {
        labels.join(", ")
    }
}

fn apk_display_name(identity: &ApkIdentity) -> String {
    format!("{} / {}", identity.package_name, identity.package_version)
}

/// Resolve a `method_id` which may be a `unique_id` or `java_signature`.
fn resolve_method_id(apk: &ApkData, raw: &str) -> Option<String> {
    if apk
        .method_infos
        .iter()
        .any(|method| method.unique_id == raw)
    {
        return Some(raw.to_string());
    }

    apk.method_infos
        .iter()
        .find(|method| method.java_signature == raw)
        .map(|method| method.unique_id.clone())
}

fn resolve_class_id(apk: &ApkData, raw: &str) -> std::result::Result<String, String> {
    let normalized = raw.trim();
    if normalized.is_empty() {
        return Err("Class selector cannot be empty".to_string());
    }

    let mut matches = Vec::new();
    for method in &apk.method_infos {
        if method.defining_class == normalized || method.class_name == normalized {
            matches.push(method.defining_class.clone());
        }
    }

    matches.sort();
    matches.dedup();

    match matches.len() {
        1 => Ok(matches.remove(0)),
        0 => Err(format!("Class not found: {normalized}")),
        _ => Err(format!(
            "Class selector '{}' is ambiguous. Matches: {}",
            normalized,
            matches.join(", ")
        )),
    }
}

fn to_class_fingerprint_candidate(
    candidate: ClassFingerprintCandidate,
    method_infos: &[MethodInfoDto],
) -> ClassFingerprintCandidateDto {
    let source_method = method_infos
        .iter()
        .find(|method| method.unique_id == candidate.source_method_id)
        .map(|method| MatchedMethodDto {
            unique_id: method.unique_id.clone(),
            defining_class: method.defining_class.clone(),
            method_name: method.name.clone(),
            return_type: method.return_type.clone(),
            parameters: method.parameters.clone(),
        });

    ClassFingerprintCandidateDto {
        fingerprint: Some(candidate.fingerprint),
        source_method,
    }
}

fn format_fingerprint_generation_error(
    message: &str,
    fallback_error: Option<&str>,
    method: Option<&MethodInfoDto>,
    class_fingerprint: Option<&MethodFingerprintDto>,
) -> String {
    if !message.contains("Could not distinguish target method") {
        return message.to_string();
    }

    let class_fallback = class_fingerprint.map(crate::morphe_render::to_morphe_code_string);
    let name_only_fallback = method.map(|method| {
        crate::morphe_render::to_morphe_name_only_fingerprint_string(
            &method.defining_class,
            &method.name,
        )
    });

    match (fallback_error, class_fallback, name_only_fallback) {
        (Some(error), Some(class_fallback), Some(name_only_fallback)) => format!(
            "{message}\n\nA class-scoped fallback was also attempted and failed:\n{error}\n\nThis class fingerprint does uniquely identify the class:\n{class_fallback}\n\nThis fingerprint can still be used but is not resilient:\n{name_only_fallback}"
        ),
        (Some(error), Some(class_fallback), None) => format!(
            "{message}\n\nA class-scoped fallback was also attempted and failed:\n{error}\n\nThis class fingerprint does uniquely identify the class:\n{class_fallback}"
        ),
        (Some(error), None, Some(name_only_fallback)) => format!(
            "{message}\n\nA class-scoped fallback was also attempted and failed:\n{error}\n\nThis fingerprint can still be used but is not resilient:\n{name_only_fallback}"
        ),
        (Some(error), None, None) => {
            format!("{message}\n\nA class-scoped fallback was also attempted and failed:\n{error}")
        }
        (None, Some(class_fallback), Some(name_only_fallback)) => format!(
            "{message}\n\nThis class fingerprint does uniquely identify the class:\n{class_fallback}\n\nThis fingerprint can still be used but is not resilient:\n{name_only_fallback}"
        ),
        (None, Some(class_fallback), None) => {
            format!(
                "{message}\n\nThis class fingerprint does uniquely identify the class:\n{class_fallback}"
            )
        }
        (None, None, Some(name_only_fallback)) => {
            format!(
                "{message}\n\nThis fingerprint can still be used but is not resilient:\n{name_only_fallback}"
            )
        }
        (None, None, None) => message.to_string(),
    }
}
