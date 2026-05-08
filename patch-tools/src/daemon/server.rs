use super::engine_worker::{ApkData, EngineHandle, rayon_spawn, spawn_engine_worker};
use super::resolver::{
    apk_display_name, format_apk_not_found_error, resolve_apk_selector, resolve_class_id,
    resolve_method_id, StateView,
};
use crate::daemon;
use crate::fingerprint::ClassFingerprintCandidate;
use crate::types::{
    ApkLoadedResponse, ApkStatus, ApkUnloadedResponse, ClassFingerprintCandidateDto,
    ClassFingerprintResultResponse, CommonFingerprintResultResponse, CommonFingerprintTargetDto,
    DaemonRequest, DaemonResponse, FingerprintResultResponse, MatchedMethodDto, MethodFingerprintDto,
    MethodInfoDto, MethodInfoList, MethodMapCandidateDto, MethodMapResponse, MethodSmaliResponse,
    SearchResultResponse, StatusInfoResponse, daemon_request, daemon_response,
};
use crate::search;
use anyhow::{Context, Result};
use std::collections::{HashMap, HashSet};
use std::convert::TryFrom;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;
use tokio::net::UnixListener;
use tokio::sync::RwLock;
use tokio_util::sync::CancellationToken;
use tokio_util::task::TaskTracker;

/// Read-mostly state shared across request handlers.
struct SharedState {
    apks: HashMap<String, Arc<ApkData>>,
    start_time: Instant,
    daemon_pid: u32,
}

impl SharedState {
    fn view(&self) -> StateView<'_> {
        StateView { apks: &self.apks }
    }
}

// =============================================================================
// Server
// =============================================================================

/// Run the daemon Unix socket server, dispatching protobuf requests.
pub async fn run(socket_path: &Path) -> Result<()> {
    let _ = std::fs::remove_file(socket_path);
    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let engine_tx = spawn_engine_worker()?;
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
    drop(engine_tx); // closes the engine job channel; the worker exits and Drop-cleans the JVM.
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
        daemon_request::Kind::GenerateCommonFingerprint(generate) => {
            handle_generate_common_fingerprint(state, generate).await
        }
        daemon_request::Kind::SearchMethods(search_methods) => {
            handle_search_methods(state, search_methods).await
        }
        daemon_request::Kind::MapMethod(map_method) => handle_map_method(state, map_method).await,
        daemon_request::Kind::GetMethodSmali(get_method_smali) => {
            handle_get_method_smali(state, engine_tx, get_method_smali).await
        }
        daemon_request::Kind::Status(_) => handle_status(state).await,

        daemon_request::Kind::Stop(_) => DaemonResponse::ok(),
    }
}

async fn resolve_loaded_apk(
    state: &Arc<RwLock<SharedState>>,
    apk_selector: Option<&str>,
) -> std::result::Result<Arc<ApkData>, String> {
    let state = state.read().await;
    let resolved_apk_id = resolve_apk_selector(&state.view(), apk_selector)?;
    state
        .apks
        .get(&resolved_apk_id)
        .cloned()
        .ok_or_else(|| format_apk_not_found_error(&state.view(), apk_selector.unwrap_or("")))
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
        match resolve_apk_selector(&state.view(), unload_apk.apk_id.as_deref()) {
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
    let apk = match resolve_loaded_apk(state, generate.apk_id.as_deref()).await {
        Ok(apk) => apk,
        Err(error) => return DaemonResponse::error(error),
    };

    match resolve_method_id(&apk, &generate.method_id) {
        Some(unique_id) => match rayon_spawn(move || {
            generate_fingerprint_response(&apk, &unique_id, limit)
        })
        .await
        {
            Ok(response) => response,
            Err(error) => DaemonResponse::error(format!("fingerprint generation failed: {error:#}")),
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
    let apk = match resolve_loaded_apk(state, generate.apk_id.as_deref()).await {
        Ok(apk) => apk,
        Err(error) => return DaemonResponse::error(error),
    };
    let resolved_class_id = match resolve_class_id(&apk, &generate.class_id) {
        Ok(class_id) => class_id,
        Err(error) => return DaemonResponse::error(error),
    };

    match rayon_spawn(move || generate_class_fingerprint_response(&apk, resolved_class_id, limit))
        .await
    {
        Ok(response) => response,
        Err(error) => {
            DaemonResponse::error(format!("class fingerprint generation failed: {error:#}"))
        }
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

struct ResolvedCommonFingerprintTarget {
    apk: Arc<ApkData>,
    method_id: String,
}

async fn handle_generate_common_fingerprint(
    state: &Arc<RwLock<SharedState>>,
    generate: crate::types::GenerateCommonFingerprintRequest,
) -> DaemonResponse {
    let limit = generate.limit.unwrap_or(8) as usize;

    if generate.targets.len() < 2 {
        return DaemonResponse::error("common-fingerprint requires at least 2 APK/method pairs");
    }

    let targets = {
        let state = state.read().await;
        let mut seen_apks = HashSet::new();
        let mut resolved = Vec::with_capacity(generate.targets.len());

        for target in generate.targets {
            let apk_id = match resolve_apk_selector(&state.view(), Some(target.apk_id.as_str())) {
                Ok(apk_id) => apk_id,
                Err(error) => return DaemonResponse::error(error),
            };

            if !seen_apks.insert(apk_id.clone()) {
                return DaemonResponse::error(format!(
                    "common-fingerprint expects one method per APK; '{}' resolves to an APK already used in this request",
                    target.apk_id
                ));
            }

            let Some(apk) = state.apks.get(&apk_id).cloned() else {
                return DaemonResponse::error(format_apk_not_found_error(
                    &state.view(),
                    &target.apk_id,
                ));
            };

            let Some(method_id) = resolve_method_id(&apk, &target.method_id) else {
                return DaemonResponse::error(format!(
                    "Method not found in {}: {}",
                    apk_display_name(&apk.identity),
                    target.method_id
                ));
            };

            resolved.push(ResolvedCommonFingerprintTarget { apk, method_id });
        }

        resolved
    };

    match rayon_spawn(move || common_fingerprint_response(targets, limit)).await {
        Ok(response) => response,
        Err(error) => {
            DaemonResponse::error(format!("common fingerprint generation failed: {error:#}"))
        }
    }
}

fn common_fingerprint_response(
    targets: Vec<ResolvedCommonFingerprintTarget>,
    limit: usize,
) -> DaemonResponse {
    let target_refs = targets
        .iter()
        .map(|target| (&target.apk.fingerprint_index, target.method_id.as_str()))
        .collect::<Vec<_>>();

    let fingerprints = match crate::fingerprint::generate_common(target_refs, limit) {
        Ok(fingerprints) => fingerprints,
        Err(error) => return DaemonResponse::error(format!("{error:#}")),
    };

    let mut target_dtos = Vec::with_capacity(targets.len());
    for target in targets {
        let Some(method) = target
            .apk
            .method_infos
            .iter()
            .find(|method| method.unique_id == target.method_id)
            .cloned()
        else {
            return DaemonResponse::error(format!("Method not found: {}", target.method_id));
        };

        target_dtos.push(CommonFingerprintTargetDto {
            apk: Some(target.apk.identity.clone()),
            method: Some(method),
        });
    }

    DaemonResponse {
        kind: Some(daemon_response::Kind::CommonFingerprintResult(
            CommonFingerprintResultResponse {
                targets: target_dtos,
                fingerprints,
            },
        )),
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

    match rayon_spawn(move || search_methods_response(&apks, &query, limit)).await {
        Ok(response) => response,
        Err(error) => DaemonResponse::error(format!("method search failed: {error:#}")),
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

        let source_apk_id =
            match resolve_apk_selector(&state.view(), Some(map_method.old_apk_id.as_str())) {
                Ok(apk_id) => apk_id,
                Err(error) => return DaemonResponse::error(error),
            };
        let target_apk_id =
            match resolve_apk_selector(&state.view(), Some(map_method.new_apk_id.as_str())) {
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
                &state.view(),
                &map_method.old_apk_id,
            ));
        };
        let Some(target_apk) = state.apks.get(&target_apk_id).cloned() else {
            return DaemonResponse::error(format_apk_not_found_error(
                &state.view(),
                &map_method.new_apk_id,
            ));
        };

        (source_apk, target_apk)
    };

    let Some(source_method_id) = resolve_method_id(&source_apk, &map_method.method_id) else {
        return DaemonResponse::error(format!("Method not found: {}", map_method.method_id));
    };

    match rayon_spawn(move || {
        map_method_response(&source_apk, &source_method_id, &target_apk, limit)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => DaemonResponse::error(format!("method map failed: {error:#}")),
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

async fn handle_get_method_smali(
    state: &Arc<RwLock<SharedState>>,
    engine_tx: &EngineHandle,
    get_method_smali: crate::types::GetMethodSmaliRequest,
) -> DaemonResponse {
    let resolved_apk_id = {
        let state = state.read().await;
        match resolve_apk_selector(&state.view(), get_method_smali.apk_id.as_deref()) {
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

    let mut sections = vec![message.to_string()];
    if let Some(error) = fallback_error {
        sections.push(format!(
            "A class-scoped fallback was also attempted and failed:\n{error}"
        ));
    }
    if let Some(class_fallback) = class_fingerprint.map(crate::morphe_render::to_morphe_code_string)
    {
        sections.push(format!(
            "This class fingerprint does uniquely identify the class:\n{class_fallback}"
        ));
    }
    if let Some(name_only_fallback) = method.map(|m| {
        crate::morphe_render::to_morphe_name_only_fingerprint_string(&m.defining_class, &m.name)
    }) {
        sections.push(format!(
            "This fingerprint can still be used but is not resilient:\n{name_only_fallback}"
        ));
    }

    sections.join("\n\n")
}
