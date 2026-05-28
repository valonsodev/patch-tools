//! Owns the JVM and serializes JNI access onto one dedicated OS thread.

use crate::engine_jni::EngineJni;
use crate::fingerprint::FingerprintIndex;
use crate::search;
use crate::types::{ApkIdentity, ApkStatus, ExecutionResultResponse, MethodData, MethodInfoDto};
use anyhow::{Context, Result};
use nucleo_matcher::Utf32String;
use tokio::sync::{mpsc, oneshot};

pub struct ApkData {
    pub identity: ApkIdentity,
    pub status: ApkStatus,
    pub method_infos: Vec<MethodInfoDto>,
    pub search_haystacks: Vec<Utf32String>,
    pub fingerprint_index: FingerprintIndex,
}

pub struct LoadedApk {
    pub identity: ApkIdentity,
    pub apk: ApkData,
}

/// Each job borrows the `EngineJni` and sends its own reply, typically via a
/// `oneshot` set up by [`EngineHandle::dispatch`].
type EngineJob = Box<dyn FnOnce(&EngineJni) + Send>;

#[derive(Clone)]
pub struct EngineHandle {
    tx: mpsc::Sender<EngineJob>,
}

impl EngineHandle {
    async fn dispatch<R, F>(&self, job: F) -> Result<R>
    where
        F: FnOnce(&EngineJni) -> R + Send + 'static,
        R: Send + 'static,
    {
        let (reply_tx, reply_rx) = oneshot::channel();
        let boxed: EngineJob = Box::new(move |engine| {
            let _ = reply_tx.send(job(engine));
        });
        self.tx
            .send(boxed)
            .await
            .map_err(|_| anyhow::anyhow!("engine worker gone"))?;
        reply_rx.await.context("engine worker gone")
    }

    pub async fn load_apk(&self, path: String) -> Result<Option<LoadedApk>> {
        self.dispatch(move |engine| {
            engine.load_apk(&path).and_then(|identity| match identity {
                Some(identity) => {
                    let methods = engine.get_apk_method_data(&identity.id)?;
                    let status = engine.get_apk_status(&identity.id)?;
                    let apk = build_apk_data(identity.clone(), status, methods)?;
                    Ok(Some(LoadedApk { identity, apk }))
                }
                None => Ok(None),
            })
        })
        .await?
    }

    pub async fn unload_apk(&self, apk_id: String) -> Result<()> {
        self.dispatch(move |engine| engine.unload_apk(&apk_id))
            .await?
    }

    pub async fn evaluate_script(
        &self,
        script_path: String,
        cap: i32,
        save_patched_apks: bool,
    ) -> Result<ExecutionResultResponse> {
        self.dispatch(move |engine| engine.evaluate_script(&script_path, cap, save_patched_apks))
            .await?
    }

    pub async fn get_method_smali(
        &self,
        apk_id: String,
        method_id: String,
    ) -> Result<Option<String>> {
        self.dispatch(move |engine| engine.get_method_smali(&apk_id, &method_id))
            .await?
    }
}

/// Use instead of `tokio::task::spawn_blocking` for handlers that already do
/// rayon-parallel work, so the tokio blocking pool stays free for I/O.
pub async fn rayon_spawn<F, T>(f: F) -> Result<T>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + 'static,
{
    let (tx, rx) = oneshot::channel();
    rayon::spawn(move || {
        let _ = tx.send(f());
    });
    rx.await.context("rayon worker dropped result")
}

/// Builds the JVM on the worker thread itself so the permanent JNI
/// attachment lives on a thread we own end-to-end. Returns only after the
/// JVM is up.
pub fn spawn_engine_worker() -> Result<EngineHandle> {
    let (tx, mut rx) = mpsc::channel::<EngineJob>(32);
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<()>>();

    std::thread::Builder::new()
        .name("engine-jvm".to_string())
        .spawn(move || {
            let engine = match EngineJni::new() {
                Ok(engine) => {
                    let _ = ready_tx.send(Ok(()));
                    engine
                }
                Err(error) => {
                    let _ = ready_tx.send(Err(error));
                    return;
                }
            };

            while let Some(job) = rx.blocking_recv() {
                job(&engine);
            }
        })
        .context("failed to spawn engine worker thread")?;

    ready_rx
        .recv()
        .context("engine worker thread exited before reporting status")??;

    Ok(EngineHandle { tx })
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
