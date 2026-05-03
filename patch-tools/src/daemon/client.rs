use crate::daemon;
use crate::types::{DaemonRequest, DaemonResponse};
use anyhow::{Context, Result};
use std::path::Path;
use tokio::io::BufReader;
use tokio::net::UnixStream;

/// Thin Unix socket client that connects to the morphe-daemon.
pub struct DaemonClient {
    reader: BufReader<tokio::net::unix::OwnedReadHalf>,
    writer: tokio::net::unix::OwnedWriteHalf,
}

/// Generate an async client method that forwards to `DaemonRequest::$req(...)` via `self.send`.
macro_rules! client_method {
    ($name:ident($($arg:ident: $ty:ty),*) => $req:ident) => {
        pub async fn $name(&mut self, $($arg: $ty),*) -> Result<DaemonResponse> {
            self.send(DaemonRequest::$req($($arg),*)).await
        }
    };
}

impl DaemonClient {
    pub async fn connect(socket_path: &Path) -> Result<Self> {
        let stream = UnixStream::connect(socket_path).await.with_context(|| {
            format!(
                "failed to connect to daemon at {}. Is it running?",
                socket_path.display(),
            )
        })?;

        let (reader, writer) = stream.into_split();
        Ok(Self {
            reader: BufReader::new(reader),
            writer,
        })
    }

    client_method!(load_apk(path: &str) => load_apk);
    client_method!(unload_apk(apk_selector: &str) => unload_apk);
    client_method!(execute(script_path: &str, cap: Option<u32>, save_patched_apks: bool) => execute);
    client_method!(generate_fingerprint(apk_selector: &str, method_id: &str, limit: Option<u32>) => generate_fingerprint);
    client_method!(generate_class_fingerprint(apk_selector: &str, class_id: &str, limit: Option<u32>) => generate_class_fingerprint);
    client_method!(search_methods(query: &str, limit: Option<u32>) => search_methods);
    client_method!(get_method_smali(apk_selector: &str, method_id: &str) => get_method_smali);
    client_method!(status() => status);
    client_method!(stop() => stop);

    async fn send(&mut self, request: DaemonRequest) -> Result<DaemonResponse> {
        daemon::write_request(&mut self.writer, &request).await?;
        daemon::read_response(&mut self.reader)
            .await?
            .context("daemon closed connection before sending a response")
    }
}
