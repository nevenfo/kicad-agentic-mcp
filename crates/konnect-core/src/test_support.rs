//! Test doubles shared by the in-crate tests that need a KiCad IPC endpoint
//! or a stand-in `kicad-cli`. Ported from upstream Konnect v0.13.0 for the
//! `update_pcb_from_schematic` tests; nothing here is compiled outside tests.

use konnect_ipc::gen::kiapi;
use nng::options::Options;
use prost::Message;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// An already-listening in-process KiCad IPC double.
///
/// The guard owns the endpoint for its entire lifetime. Dropping it closes the
/// shared NNG socket, wakes the receive loop, joins the worker, and releases
/// the endpoint name, so tests never leave a detached mock thread behind.
pub(crate) struct MockIpcServer {
    address: String,
    control: nng::Socket,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl MockIpcServer {
    pub(crate) fn spawn(
        purpose: &str,
        respond: impl Fn(kiapi::common::ApiRequest) -> kiapi::common::ApiResponse + Send + 'static,
    ) -> Self {
        static NEXT_SERVER: AtomicU64 = AtomicU64::new(0);
        let sequence = NEXT_SERVER.fetch_add(1, Ordering::Relaxed);
        let address = format!(
            "inproc://konnect-core-{}-{sequence}-{purpose}",
            std::process::id()
        );
        let socket = nng::Socket::new(nng::Protocol::Rep0).expect("mock rep socket");
        socket
            .set_opt::<nng::options::RecvTimeout>(Some(Duration::from_secs(10)))
            .expect("mock receive timeout");
        socket.listen(&address).expect("mock listen");

        let control = socket.clone();
        let worker = std::thread::spawn(move || {
            while let Ok(message) = socket.recv() {
                let Ok(request) = kiapi::common::ApiRequest::decode(message.as_slice()) else {
                    break;
                };
                let response = respond(request);
                let output = nng::Message::from(response.encode_to_vec().as_slice());
                if socket.send(output).is_err() {
                    break;
                }
            }
        });

        Self {
            address,
            control,
            worker: Some(worker),
        }
    }

    pub(crate) fn address(&self) -> &str {
        &self.address
    }
}

impl Drop for MockIpcServer {
    fn drop(&mut self) {
        self.control.close();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// One open PCB document in the form KiCad sends: a `board_filename`.
pub(crate) fn board_document(filename: &str) -> kiapi::common::types::DocumentSpecifier {
    kiapi::common::types::DocumentSpecifier {
        r#type: kiapi::common::types::DocumentType::DoctypePcb as i32,
        project: None,
        identifier: Some(
            kiapi::common::types::document_specifier::Identifier::BoardFilename(
                filename.to_string(),
            ),
        ),
    }
}

/// A KiCad double that holds `board` open: it answers `GetOpenDocuments`
/// itself and hands every other command to `respond`.
pub(crate) fn spawn_kicad_holding_board(
    board: &Path,
    respond: impl Fn(&prost_types::Any) -> Option<prost_types::Any> + Send + 'static,
) -> MockIpcServer {
    let documents = vec![board_document(&board.to_string_lossy())];
    MockIpcServer::spawn("board-documents", move |request| {
        let command = request.message.expect("a command");
        let body = if command.type_url.ends_with("GetOpenDocuments") {
            Some(konnect_ipc::builders::pack_any(
                &kiapi::common::commands::GetOpenDocumentsResponse {
                    documents: documents.clone(),
                },
                "kiapi.common.commands.GetOpenDocumentsResponse",
            ))
        } else {
            respond(&command)
        };
        kiapi::common::ApiResponse {
            status: Some(kiapi::common::ApiResponseStatus {
                status: kiapi::common::ApiStatusCode::AsOk as i32,
                error_message: String::new(),
            }),
            header: None,
            message: body,
        }
    })
}

/// Write an executable stand-in script: `unix_body` on Unix, `windows_body`
/// (as a `.cmd`) on Windows.
pub(crate) fn write_script(dir: &Path, stem: &str, unix_body: &str, windows_body: &str) -> PathBuf {
    #[cfg(windows)]
    let path = dir.join(format!("{stem}.cmd"));
    #[cfg(not(windows))]
    let path = dir.join(stem);

    #[cfg(windows)]
    {
        let _ = unix_body;
        std::fs::write(&path, windows_body).unwrap();
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = windows_body;
        std::fs::write(&path, unix_body).unwrap();
        let mut permissions = std::fs::metadata(&path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&path, permissions).unwrap();
    }
    path
}
