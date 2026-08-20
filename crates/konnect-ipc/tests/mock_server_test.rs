//! IPC client tests against a mock KiCAD NNG server — no KiCAD required.
//!
//! A rep0 socket on inproc://<unique-name> plays KiCAD: it decodes the
//! ApiRequest envelope and returns canned ApiResponse messages. This lets CI
//! exercise the full encode → transport → decode → error-mapping path that
//! previously only ran against a live KiCAD session.

use konnect_ipc::builders;
use konnect_ipc::gen::kiapi;
use konnect_ipc::KiCadIpcClient;
use nng::options::Options;
use prost::Message;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A rep0 server answering each request via `respond`.
/// Returns the inproc:// URL to dial. The server thread exits when the socket
/// errors (i.e. when `_socket_keepalive` is dropped by the returned guard).
struct MockKicad {
    url: String,
    _thread: std::thread::JoinHandle<()>,
}

fn spawn_mock<F>(respond: F) -> MockKicad
where
    F: Fn(kiapi::common::ApiRequest) -> Option<kiapi::common::ApiResponse> + Send + 'static,
{
    // inproc:// needs no port, so there is no bind-a-TcpListener-then-relisten
    // TOCTOU window (that pattern intermittently died with AddressInUse on CI
    // when another process grabbed the probed port). The name only has to be
    // unique within this test process; a counter suffices.
    static NEXT_MOCK: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let url = format!(
        "inproc://mock-kicad-{}",
        NEXT_MOCK.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );

    // Listen BEFORE returning (and before the receive thread spawns): the
    // client dials the moment spawn_mock returns, and NNG's dial fails
    // immediately if nothing is bound yet. Doing the listen inside the
    // thread raced the caller — flaky on slow CI runners.
    let socket = nng::Socket::new(nng::Protocol::Rep0).expect("mock rep socket");
    socket
        .set_opt::<nng::options::RecvTimeout>(Some(Duration::from_secs(20)))
        .unwrap();
    socket.listen(&url).expect("mock listen");

    let thread = std::thread::spawn(move || {
        while let Ok(msg) = socket.recv() {
            let request = match kiapi::common::ApiRequest::decode(msg.as_slice()) {
                Ok(r) => r,
                Err(_) => break,
            };
            match respond(request) {
                Some(resp) => {
                    let out = nng::Message::from(resp.encode_to_vec().as_slice());
                    if socket.send(out).is_err() {
                        break;
                    }
                }
                None => {
                    // Simulate a wedged KiCAD: never reply. The rep socket
                    // can't take another request until it replies, so just
                    // park until the test ends.
                    std::thread::sleep(Duration::from_secs(20));
                    break;
                }
            }
        }
    });

    MockKicad {
        url,
        _thread: thread,
    }
}

fn ok_response() -> kiapi::common::ApiResponse {
    kiapi::common::ApiResponse {
        status: Some(kiapi::common::ApiResponseStatus {
            status: kiapi::common::ApiStatusCode::AsOk as i32,
            error_message: String::new(),
        }),
        header: None,
        message: None,
    }
}

fn reply_with(inner: prost_types::Any) -> kiapi::common::ApiResponse {
    kiapi::common::ApiResponse {
        message: Some(inner),
        ..ok_response()
    }
}

fn open_board_response() -> kiapi::common::ApiResponse {
    let response = kiapi::common::commands::GetOpenDocumentsResponse {
        documents: vec![kiapi::common::types::DocumentSpecifier {
            r#type: kiapi::common::types::DocumentType::DoctypePcb as i32,
            project: None,
            identifier: Some(
                kiapi::common::types::document_specifier::Identifier::BoardFilename(
                    "test.kicad_pcb".to_string(),
                ),
            ),
        }],
    };
    reply_with(builders::pack_any(
        &response,
        "kiapi.common.commands.GetOpenDocumentsResponse",
    ))
}

#[test]
fn ping_roundtrips_through_mock() {
    let mock = spawn_mock(|req| {
        // The envelope must carry a client name and a packed command.
        assert!(req.header.is_some());
        let header = req.header.unwrap();
        assert!(header.client_name.starts_with("konnect-"));
        let msg = req.message.expect("request must pack a command");
        assert!(
            msg.type_url.ends_with("kiapi.common.commands.Ping"),
            "unexpected type_url: {}",
            msg.type_url
        );
        Some(ok_response())
    });

    let client = KiCadIpcClient::new(&mock.url);
    assert!(client.ping().unwrap());
}

#[test]
fn explicit_kicad_token_is_sent_in_request_header() {
    let mock = spawn_mock(|req| {
        let header = req.header.expect("request header");
        assert_eq!(header.kicad_token, "linux-instance-token");
        Some(ok_response())
    });

    let client = KiCadIpcClient::new_with_token(&mock.url, "linux-instance-token");
    assert!(client.ping().unwrap());
}

#[test]
fn kicad_error_status_maps_to_err() {
    let mock = spawn_mock(|_req| {
        Some(kiapi::common::ApiResponse {
            status: Some(kiapi::common::ApiResponseStatus {
                status: kiapi::common::ApiStatusCode::AsBadRequest as i32,
                error_message: "no board open".to_string(),
            }),
            header: None,
            message: None,
        })
    });

    let client = KiCadIpcClient::new(&mock.url);
    // ping() swallows errors into Ok(false) by design — that's the
    // "KiCAD unreachable" UX. It must not be Ok(true) and must not hang.
    assert!(!client.ping().unwrap());

    // A typed call surfaces the error text.
    let err = client.get_open_documents().unwrap_err().to_string();
    assert!(err.contains("no board open"), "unexpected error: {err}");
}

#[test]
fn unreachable_endpoint_errors_fast() {
    // Nothing listens here; dial must fail with an error, not hang.
    let client = KiCadIpcClient::new("tcp://127.0.0.1:1");
    let start = std::time::Instant::now();
    let result = client.get_open_documents();
    assert!(result.is_err());
    assert!(
        start.elapsed() < Duration::from_secs(10),
        "dial to dead endpoint took {:?}",
        start.elapsed()
    );
}

#[test]
fn empty_socket_path_is_configuration_error() {
    // Clear KICAD_API_SOCKET influence by passing explicit empty and hoping
    // the env var isn't set in CI; if it is, skip.
    if std::env::var("KICAD_API_SOCKET").is_ok() {
        eprintln!("SKIP: KICAD_API_SOCKET set in environment");
        return;
    }
    let client = KiCadIpcClient::new("");
    let err = client.get_open_documents().unwrap_err().to_string();
    assert!(
        err.contains("socket path not configured"),
        "unexpected error: {err}"
    );
    assert!(
        err.contains("TROUBLESHOOTING"),
        "error should link the troubleshooting guide: {err}"
    );
}

// ─── CreateItems outcome handling ─────────────────────────────────────────────
//
// Ported from emolitor's PR #66 (which exercised these against the
// ParseAndCreateItemsFromString path) and adapted to the typed create_items
// API: the per-item accounting bugs they guard are identical.

/// A mock KiCAD with one board open that answers every CreateItems with
/// `results`.
fn spawn_mock_creating(results: Vec<kiapi::common::commands::ItemCreationResult>) -> MockKicad {
    spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else {
            assert!(message.type_url.ends_with("CreateItems"));
            let response = kiapi::common::commands::CreateItemsResponse {
                header: None,
                // IRS_OK even though nothing may have been created — the
                // response shape the proto explicitly warns about.
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                created_items: results.clone(),
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.CreateItemsResponse",
            )))
        }
    })
}

fn creation_result(
    code: kiapi::common::commands::ItemStatusCode,
    message: &str,
) -> kiapi::common::commands::ItemCreationResult {
    kiapi::common::commands::ItemCreationResult {
        status: Some(kiapi::common::commands::ItemStatus {
            code: code as i32,
            error_message: message.to_string(),
        }),
        item: None,
    }
}

fn any_item() -> prost_types::Any {
    builders::pack_any(&kiapi::common::commands::Ping {}, "test.Item")
}

#[test]
fn a_rejected_item_is_not_counted_as_created() {
    // The regression this guards: created_items is documented as "status of
    // each item TO BE created", so a rejection still occupies a slot. Counting
    // the vector's length would call this a success and put the phantom back.
    let mock = spawn_mock_creating(vec![creation_result(
        kiapi::common::commands::ItemStatusCode::IscInvalidData,
        "footprint has no pads",
    )]);

    let client = KiCadIpcClient::new(&mock.url);
    let err = client
        .create_items(vec![any_item()])
        .unwrap_err()
        .to_string();

    assert!(err.contains("created 0 of 1"), "must report failure: {err}");
    // The per-item reason is what makes this diagnosable.
    assert!(
        err.contains("ISC_INVALID_DATA") && err.contains("footprint has no pads"),
        "must surface KiCAD's own reason: {err}"
    );
}

#[test]
fn an_empty_result_list_still_reports_failure() {
    // KiCAD 10.0's actual behaviour: an empty CreateItemsResponse with IRS_OK.
    let mock = spawn_mock_creating(vec![]);
    let client = KiCadIpcClient::new(&mock.url);
    let err = client
        .create_items(vec![any_item()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("created no items"), "unexpected: {err}");
    assert!(err.contains("no items at all"), "unexpected: {err}");
}

#[test]
fn a_created_item_counts() {
    let mock = spawn_mock_creating(vec![creation_result(
        kiapi::common::commands::ItemStatusCode::IscOk,
        "",
    )]);
    let client = KiCadIpcClient::new(&mock.url);
    client
        .create_items(vec![any_item()])
        .expect("an ISC_OK result is a created item");
}

#[test]
fn a_defaulted_status_counts_only_when_an_item_came_back() {
    // Protobuf cannot distinguish "unset" from "explicitly zero", so an
    // ISC_UNKNOWN status is only evidence of success if an item is attached.
    let with_item = kiapi::common::commands::ItemCreationResult {
        status: None,
        item: Some(prost_types::Any::default()),
    };
    let mock = spawn_mock_creating(vec![with_item]);
    let client = KiCadIpcClient::new(&mock.url);
    client
        .create_items(vec![any_item()])
        .expect("an item with a defaulted status was still created");

    // The same defaulted status with nothing attached created nothing.
    let without_item = kiapi::common::commands::ItemCreationResult {
        status: Some(kiapi::common::commands::ItemStatus {
            code: kiapi::common::commands::ItemStatusCode::IscUnknown as i32,
            error_message: String::new(),
        }),
        item: None,
    };
    let mock = spawn_mock_creating(vec![without_item]);
    let client = KiCadIpcClient::new(&mock.url);
    assert!(
        client.create_items(vec![any_item()]).is_err(),
        "a bare ISC_UNKNOWN with no item must not count as created"
    );
}

#[test]
fn a_mixed_response_counts_only_the_successes() {
    let mock = spawn_mock_creating(vec![
        creation_result(kiapi::common::commands::ItemStatusCode::IscOk, ""),
        creation_result(
            kiapi::common::commands::ItemStatusCode::IscInvalidData,
            "bad",
        ),
    ]);
    let client = KiCadIpcClient::new(&mock.url);
    let err = client
        .create_items(vec![any_item(), any_item()])
        .unwrap_err()
        .to_string();
    assert!(err.contains("created 1 of 2"), "unexpected: {err}");
    assert!(
        err.contains("item 1") && err.contains("ISC_INVALID_DATA") && err.contains("bad"),
        "the rejected slot must be identified: {err}"
    );
}

/// End-to-end placement through the mock: the FootprintInstance sent over the
/// wire must carry the footprint's graphics (courtyard/silk/fab) as children
/// alongside its pads — a pads-only instance trips lib_footprint_mismatch and
/// makes courtyard DRC meaningless.
#[test]
fn place_footprint_sends_graphics_children() {
    use konnect_ipc::{IpcGraphicDefinition, IpcPadDefinition};

    let captured: Arc<Mutex<Option<kiapi::common::commands::CreateItems>>> =
        Arc::new(Mutex::new(None));
    let captured_in_mock = captured.clone();
    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else if message.type_url.ends_with("GetItems") {
            // Before creation the board is empty; afterwards it holds exactly
            // what CreateItems carried, so the client's verification pass sees
            // its own footprint.
            let items = captured_in_mock
                .lock()
                .unwrap()
                .as_ref()
                .map(|create| create.items.clone())
                .unwrap_or_default();
            Some(reply_with(builders::pack_any(
                &kiapi::common::commands::GetItemsResponse {
                    header: None,
                    status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                    items,
                },
                "kiapi.common.commands.GetItemsResponse",
            )))
        } else {
            assert!(message.type_url.ends_with("CreateItems"));
            let create =
                kiapi::common::commands::CreateItems::decode(message.value.as_slice()).unwrap();
            let created_items = create
                .items
                .iter()
                .cloned()
                .map(|item| kiapi::common::commands::ItemCreationResult {
                    status: Some(kiapi::common::commands::ItemStatus {
                        code: kiapi::common::commands::ItemStatusCode::IscOk as i32,
                        error_message: String::new(),
                    }),
                    item: Some(item),
                })
                .collect();
            *captured_in_mock.lock().unwrap() = Some(create);
            Some(reply_with(builders::pack_any(
                &kiapi::common::commands::CreateItemsResponse {
                    header: None,
                    status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                    created_items,
                },
                "kiapi.common.commands.CreateItemsResponse",
            )))
        }
    });

    let pads = vec![IpcPadDefinition {
        number: "1".to_string(),
        pad_type: "smd".to_string(),
        shape: "roundrect".to_string(),
        x: -0.5,
        y: 0.0,
        rotation: 0.0,
        size_x: 0.5,
        size_y: 0.5,
        drill_x: None,
        drill_y: None,
        drill_oval: false,
        layers: vec!["F.Cu".to_string()],
        roundrect_ratio: 0.25,
    }];
    let graphics = vec![
        IpcGraphicDefinition::Rect {
            start: (-0.8, -0.7),
            end: (0.8, 0.7),
            layer: "F.CrtYd".to_string(),
            width: 0.05,
            filled: false,
        },
        IpcGraphicDefinition::Line {
            start: (-0.6, -0.5),
            end: (0.6, -0.5),
            layer: "F.SilkS".to_string(),
            width: 0.12,
        },
        IpcGraphicDefinition::Text {
            text: "R_0402".to_string(),
            position: (0.0, 1.17),
            rotation: 0.0,
            layer: "F.Fab".to_string(),
            size: 0.26,
        },
    ];

    let client = KiCadIpcClient::new(&mock.url);
    let placed = client
        .place_footprint(
            std::path::Path::new("test.kicad_pcb"),
            "Resistor_SMD:R_0402",
            "R1",
            "R_0402",
            &pads,
            &graphics,
            &konnect_ipc::IpcFieldPlacement::default(),
            10.0,
            20.0,
            0.0,
            "F.Cu",
        )
        .expect("placement through the mock should succeed");
    assert_eq!(placed.reference, "R1");

    let create = captured.lock().unwrap().take().expect("CreateItems sent");
    assert_eq!(create.items.len(), 1);
    let footprint =
        kiapi::board::types::FootprintInstance::decode(create.items[0].value.as_slice())
            .expect("sent item must be a FootprintInstance");
    let children = footprint.definition.expect("definition").items;
    let pads_sent = children
        .iter()
        .filter(|any| any.type_url.ends_with("kiapi.board.types.Pad"))
        .count();
    let shapes_sent = children
        .iter()
        .filter(|any| any.type_url.ends_with("BoardGraphicShape"))
        .count();
    let texts_sent = children
        .iter()
        .filter(|any| any.type_url.ends_with("BoardText"))
        .count();
    assert_eq!(pads_sent, 1, "pad child missing");
    assert_eq!(shapes_sent, 2, "courtyard rect + silk line must be sent");
    assert_eq!(texts_sent, 1, "fab text must be sent");
}

#[test]
fn create_items_requires_a_typed_response_payload() {
    let mock = spawn_mock(|request| {
        let message = request.message.expect("request message");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else {
            assert!(message.type_url.ends_with("CreateItems"));
            Some(ok_response())
        }
    });
    let client = KiCadIpcClient::new(&mock.url);
    let item = builders::pack_any(&kiapi::common::commands::Ping {}, "test.Item");

    let error = client.create_items(vec![item]).unwrap_err().to_string();

    assert!(error.contains("no CreateItems response payload"), "{error}");
}

#[test]
fn update_items_rejects_missing_per_item_results() {
    let mock = spawn_mock(|request| {
        let message = request.message.expect("request message");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else {
            assert!(message.type_url.ends_with("UpdateItems"));
            let response = kiapi::common::commands::UpdateItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                updated_items: vec![],
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.UpdateItemsResponse",
            )))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);
    let item = builders::pack_any(&kiapi::common::commands::Ping {}, "test.Item");

    let error = client.update_items(vec![item]).unwrap_err().to_string();

    assert!(
        error.contains("0 update results for 1 requested"),
        "{error}"
    );
}

#[test]
fn delete_items_surfaces_per_item_failure() {
    let mock = spawn_mock(|request| {
        let message = request.message.expect("request message");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else {
            assert!(message.type_url.ends_with("DeleteItems"));
            let response = kiapi::common::commands::DeleteItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                deleted_items: vec![kiapi::common::commands::ItemDeletionResult {
                    id: Some(kiapi::common::types::Kiid {
                        value: "missing-id".to_string(),
                    }),
                    status: kiapi::common::commands::ItemDeletionStatus::IdsNonexistent as i32,
                }],
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.DeleteItemsResponse",
            )))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    let error = client
        .delete_items(vec!["missing-id".to_string()])
        .unwrap_err()
        .to_string();

    assert!(error.contains("IDS_NONEXISTENT"), "{error}");
    assert!(error.contains("missing-id"), "{error}");
}

/// KiCad 10 builds per-item deletion results and never attaches them, so a
/// successful delete comes back with an empty `deleted_items`. Treating that
/// as failure is what made delete_component report "0 deletion results" for
/// deletions that had actually happened (#116).
#[test]
fn an_empty_deletion_result_list_is_not_a_failure() {
    let mock = spawn_mock(|request| {
        let message = request.message.expect("request message");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else {
            assert!(message.type_url.ends_with("DeleteItems"));
            let response = kiapi::common::commands::DeleteItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                deleted_items: vec![],
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.DeleteItemsResponse",
            )))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    client
        .delete_items(vec!["some-id".to_string()])
        .expect("an empty result list means KiCad said nothing, not that it failed");
}

#[test]
fn failed_multi_step_commit_is_dropped() {
    let actions = Arc::new(Mutex::new(Vec::new()));
    let captured_actions = actions.clone();
    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request message");
        if message.type_url.ends_with("BeginCommit") {
            let response = kiapi::common::commands::BeginCommitResponse {
                id: Some(kiapi::common::types::Kiid {
                    value: "commit-1".to_string(),
                }),
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.BeginCommitResponse",
            )))
        } else {
            assert!(message.type_url.ends_with("EndCommit"));
            let command =
                kiapi::common::commands::EndCommit::decode(message.value.as_slice()).unwrap();
            captured_actions.lock().unwrap().push(command.action());
            Some(reply_with(builders::pack_any(
                &kiapi::common::commands::EndCommitResponse {},
                "kiapi.common.commands.EndCommitResponse",
            )))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    let error = client
        .run_commit::<()>("test transaction", |_| anyhow::bail!("second step failed"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("changes dropped"), "{error}");
    assert_eq!(
        *actions.lock().unwrap(),
        vec![kiapi::common::commands::CommitAction::CmaDrop]
    );
}

// ─── Track batch atomicity (D.9.2) ────────────────────────────────────────────

/// Short command name from a `type_url`, e.g. `"kiapi.common.commands.CreateItems"` -> `"CreateItems"`.
fn short_name(type_url: &str) -> String {
    type_url.rsplit('.').next().unwrap_or(type_url).to_string()
}

fn nets_response(nets: &[(&str, i32)]) -> kiapi::common::ApiResponse {
    let response = kiapi::board::commands::NetsResponse {
        nets: nets
            .iter()
            .map(|(name, code)| kiapi::board::types::Net {
                code: Some(kiapi::board::types::NetCode { value: *code }),
                name: name.to_string(),
            })
            .collect(),
    };
    reply_with(builders::pack_any(
        &response,
        "kiapi.board.commands.NetsResponse",
    ))
}

fn create_items_ok_response(
    create: &kiapi::common::commands::CreateItems,
) -> kiapi::common::ApiResponse {
    let created_items = create
        .items
        .iter()
        .cloned()
        .map(|item| kiapi::common::commands::ItemCreationResult {
            status: Some(kiapi::common::commands::ItemStatus {
                code: kiapi::common::commands::ItemStatusCode::IscOk as i32,
                error_message: String::new(),
            }),
            item: Some(item),
        })
        .collect();
    reply_with(builders::pack_any(
        &kiapi::common::commands::CreateItemsResponse {
            header: None,
            status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
            created_items,
        },
        "kiapi.common.commands.CreateItemsResponse",
    ))
}

fn track_spec(net_name: &str) -> konnect_ipc::TrackSpec {
    konnect_ipc::TrackSpec {
        net_name: net_name.to_string(),
        layer: "F.Cu".to_string(),
        width: 0.25,
        x1: 0.0,
        y1: 0.0,
        x2: 1.0,
        y2: 1.0,
    }
}

#[test]
fn add_tracks_sends_one_create_items_for_the_whole_batch() {
    let sequence: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let captured_create: Arc<Mutex<Option<kiapi::common::commands::CreateItems>>> =
        Arc::new(Mutex::new(None));
    let seq = sequence.clone();
    let captured = captured_create.clone();
    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        seq.lock().unwrap().push(short_name(&message.type_url));
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else if message.type_url.ends_with("GetNets") {
            Some(nets_response(&[("GND", 1), ("VCC", 2)]))
        } else {
            assert!(message.type_url.ends_with("CreateItems"));
            let create =
                kiapi::common::commands::CreateItems::decode(message.value.as_slice()).unwrap();
            let response = create_items_ok_response(&create);
            *captured.lock().unwrap() = Some(create);
            Some(response)
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    client
        .add_tracks(&[track_spec("GND"), track_spec("VCC")])
        .expect("batch should succeed");

    let seq = sequence.lock().unwrap();
    assert_eq!(
        seq.iter().filter(|n| *n == "CreateItems").count(),
        1,
        "expected exactly one CreateItems: {seq:?}"
    );
    assert_eq!(
        seq.iter().filter(|n| *n == "GetNets").count(),
        1,
        "expected exactly one GetNets: {seq:?}"
    );
    let create = captured_create
        .lock()
        .unwrap()
        .take()
        .expect("CreateItems sent");
    assert_eq!(create.items.len(), 2);
}

#[test]
fn replace_track_wraps_the_delete_and_the_add_in_one_commit() {
    let sequence: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let seq = sequence.clone();
    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        seq.lock().unwrap().push(short_name(&message.type_url));
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else if message.type_url.ends_with("GetNets") {
            Some(nets_response(&[("GND", 1)]))
        } else if message.type_url.ends_with("BeginCommit") {
            let response = kiapi::common::commands::BeginCommitResponse {
                id: Some(kiapi::common::types::Kiid {
                    value: "commit-1".to_string(),
                }),
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.BeginCommitResponse",
            )))
        } else if message.type_url.ends_with("DeleteItems") {
            let response = kiapi::common::commands::DeleteItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                deleted_items: vec![],
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.DeleteItemsResponse",
            )))
        } else if message.type_url.ends_with("CreateItems") {
            let create =
                kiapi::common::commands::CreateItems::decode(message.value.as_slice()).unwrap();
            Some(create_items_ok_response(&create))
        } else {
            assert!(message.type_url.ends_with("EndCommit"));
            let command =
                kiapi::common::commands::EndCommit::decode(message.value.as_slice()).unwrap();
            assert_eq!(
                command.action(),
                kiapi::common::commands::CommitAction::CmaCommit
            );
            Some(reply_with(builders::pack_any(
                &kiapi::common::commands::EndCommitResponse {},
                "kiapi.common.commands.EndCommitResponse",
            )))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    client
        .replace_track("track-uuid", &track_spec("GND"))
        .expect("replace should succeed");

    let seq = sequence.lock().unwrap();
    let milestones: Vec<&str> = seq
        .iter()
        .map(|s| s.as_str())
        .filter(|s| {
            matches!(
                *s,
                "BeginCommit" | "DeleteItems" | "CreateItems" | "EndCommit"
            )
        })
        .collect();
    assert_eq!(
        milestones,
        vec!["BeginCommit", "DeleteItems", "CreateItems", "EndCommit"],
        "unexpected order: {seq:?}"
    );
}

#[test]
fn a_failed_add_after_a_delete_drops_the_commit() {
    let actions: Arc<Mutex<Vec<kiapi::common::commands::CommitAction>>> =
        Arc::new(Mutex::new(Vec::new()));
    let captured_actions = actions.clone();
    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else if message.type_url.ends_with("GetNets") {
            Some(nets_response(&[("GND", 1)]))
        } else if message.type_url.ends_with("BeginCommit") {
            let response = kiapi::common::commands::BeginCommitResponse {
                id: Some(kiapi::common::types::Kiid {
                    value: "commit-1".to_string(),
                }),
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.BeginCommitResponse",
            )))
        } else if message.type_url.ends_with("DeleteItems") {
            let response = kiapi::common::commands::DeleteItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                deleted_items: vec![],
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.DeleteItemsResponse",
            )))
        } else if message.type_url.ends_with("CreateItems") {
            // KiCad refuses the create half of the swap: IRS_OK carrying no
            // per-item results at all, same as the "ignored request" shape
            // `create_items` already rejects.
            let response = kiapi::common::commands::CreateItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                created_items: vec![],
            };
            Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.CreateItemsResponse",
            )))
        } else {
            assert!(message.type_url.ends_with("EndCommit"));
            let command =
                kiapi::common::commands::EndCommit::decode(message.value.as_slice()).unwrap();
            captured_actions.lock().unwrap().push(command.action());
            Some(reply_with(builders::pack_any(
                &kiapi::common::commands::EndCommitResponse {},
                "kiapi.common.commands.EndCommitResponse",
            )))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    let error = client
        .replace_track("track-uuid", &track_spec("GND"))
        .unwrap_err()
        .to_string();

    assert!(error.contains("changes dropped"), "{error}");
    assert_eq!(
        *actions.lock().unwrap(),
        vec![kiapi::common::commands::CommitAction::CmaDrop],
        "the commit must never be pushed after a failed create"
    );
}

#[test]
fn add_tracks_sends_nothing_when_a_net_name_is_unknown() {
    let create_items_seen = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = create_items_seen.clone();
    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        if message.type_url.ends_with("GetOpenDocuments") {
            Some(open_board_response())
        } else if message.type_url.ends_with("GetNets") {
            // Only GND exists; VCC is unknown.
            Some(nets_response(&[("GND", 1)]))
        } else {
            assert!(message.type_url.ends_with("CreateItems"));
            seen.store(true, std::sync::atomic::Ordering::SeqCst);
            let create =
                kiapi::common::commands::CreateItems::decode(message.value.as_slice()).unwrap();
            Some(create_items_ok_response(&create))
        }
    });
    let client = KiCadIpcClient::new(&mock.url);

    let error = client
        .add_tracks(&[track_spec("GND"), track_spec("VCC")])
        .unwrap_err()
        .to_string();

    assert!(error.contains("VCC"), "{error}");
    assert!(
        !create_items_seen.load(std::sync::atomic::Ordering::SeqCst),
        "an unknown net name must not let any track through"
    );
}

// ─── IpcFailure classification ────────────────────────────────────────────────
//
// The file-editing fallback in konnect-core is gated on this classification:
// only a transport that never delivered the request may fall back, and the
// decision must come from the typed marker, never from matching error text
// (Copilot flagged substring matching three times on PR #66).

#[test]
fn an_unconfigured_socket_classifies_as_unconfigured() {
    if std::env::var("KICAD_API_SOCKET").is_ok() {
        eprintln!("SKIP: KICAD_API_SOCKET set in environment");
        return;
    }
    let client = KiCadIpcClient::new("");
    let failure = konnect_ipc::IpcFailure::from_error(client.get_open_documents().unwrap_err());
    assert!(
        matches!(failure, konnect_ipc::IpcFailure::Unconfigured(_)),
        "unexpected classification: {failure:?}"
    );
    assert!(failure.allows_file_fallback(), "{failure:?}");
}

#[test]
fn a_dead_endpoint_classifies_as_unreachable() {
    let client = KiCadIpcClient::new("tcp://127.0.0.1:1");
    let failure = konnect_ipc::IpcFailure::from_error(client.get_open_documents().unwrap_err());
    assert!(
        matches!(failure, konnect_ipc::IpcFailure::Unreachable(_)),
        "unexpected classification: {failure:?}"
    );
    assert!(failure.allows_file_fallback(), "{failure:?}");
}

#[test]
fn a_live_kicad_that_says_no_classifies_as_rejected() {
    let mock = spawn_mock(|_req| {
        Some(kiapi::common::ApiResponse {
            status: Some(kiapi::common::ApiResponseStatus {
                status: kiapi::common::ApiStatusCode::AsBadRequest as i32,
                error_message: "no board open".to_string(),
            }),
            header: None,
            message: None,
        })
    });
    let client = KiCadIpcClient::new(&mock.url);
    let failure = konnect_ipc::IpcFailure::from_error(client.get_open_documents().unwrap_err());
    assert!(
        matches!(failure, konnect_ipc::IpcFailure::Rejected(_)),
        "a completed round-trip must never classify as unreachable: {failure:?}"
    );
    assert!(failure.message().contains("no board open"), "{failure:?}");
    assert!(!failure.allows_file_fallback(), "{failure:?}");
}

/// The regression the recv timeout exists for: a server that accepts the
/// request and never replies. The predecessor project hung >600 s here; the
/// client must give up at its recv timeout instead.
///
/// Ignored by default: it necessarily takes the full 30 s recv timeout.
/// Run explicitly with: cargo test -p konnect-ipc -- --ignored
#[test]
#[ignore = "takes ~30s (full recv timeout) by design"]
fn wedged_server_times_out_instead_of_hanging() {
    let mock = spawn_mock(|_req| None); // accept, never respond

    let client = KiCadIpcClient::new(&mock.url);
    let start = std::time::Instant::now();
    let result = client.get_open_documents();
    assert!(result.is_err(), "expected timeout error");
    let elapsed = start.elapsed();
    assert!(
        elapsed >= Duration::from_secs(25) && elapsed < Duration::from_secs(60),
        "expected ~30s recv timeout, got {elapsed:?}"
    );
}

// ─── Multi-board document targeting ──────────────────────────────────────────
//
// Live verification caught this: with the user's own project focused and the
// target board open behind it, first-document targeting either fails or
// mutates the wrong board. place_footprint must address the document whose
// path matches the request.

#[test]
fn placement_targets_the_named_board_among_several_open() {
    use std::sync::{Arc, Mutex};
    let captured: Arc<Mutex<Option<String>>> = Arc::new(Mutex::new(None));
    let captured_in_mock = captured.clone();

    let mock = spawn_mock(move |request| {
        let message = request.message.expect("request must pack a command");
        if message.type_url.ends_with("GetOpenDocuments") {
            let response = kiapi::common::commands::GetOpenDocumentsResponse {
                documents: vec![
                    doc_for("other-project.kicad_pcb"),
                    doc_for("target.kicad_pcb"),
                ],
            };
            return Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.GetOpenDocumentsResponse",
            )));
        }
        if message.type_url.ends_with("GetItems") {
            let request =
                kiapi::common::commands::GetItems::decode(message.value.as_slice()).unwrap();
            record_doc(&captured_in_mock, &request.header);
            let response = kiapi::common::commands::GetItemsResponse {
                header: None,
                status: kiapi::common::types::ItemRequestStatus::IrsOk as i32,
                items: vec![],
            };
            return Some(reply_with(builders::pack_any(
                &response,
                "kiapi.common.commands.GetItemsResponse",
            )));
        }
        if message.type_url.ends_with("CreateItems") {
            let request =
                kiapi::common::commands::CreateItems::decode(message.value.as_slice()).unwrap();
            record_doc(&captured_in_mock, &request.header);
            // Fail fast after capturing the header; the assertion below is
            // about WHICH board the create addressed, not the outcome.
            return Some(kiapi::common::ApiResponse {
                status: Some(kiapi::common::ApiResponseStatus {
                    status: kiapi::common::ApiStatusCode::AsBadRequest as i32,
                    error_message: "stop here".to_string(),
                }),
                header: None,
                message: None,
            });
        }
        Some(ok_response())
    });

    let client = KiCadIpcClient::new(&mock.url);
    let _ = client.place_footprint(
        std::path::Path::new("target.kicad_pcb"),
        "Resistor_SMD:R_0402",
        "R1",
        "R_0402",
        &[],
        &[],
        &konnect_ipc::IpcFieldPlacement::default(),
        10.0,
        20.0,
        0.0,
        "F.Cu",
    );

    let addressed = captured
        .lock()
        .unwrap()
        .take()
        .expect("a command carried a document");
    assert_eq!(
        addressed, "target.kicad_pcb",
        "commands must address the requested board, not the first open one"
    );
}

fn doc_for(filename: &str) -> kiapi::common::types::DocumentSpecifier {
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

fn record_doc(
    slot: &std::sync::Arc<std::sync::Mutex<Option<String>>>,
    header: &Option<kiapi::common::types::ItemHeader>,
) {
    if let Some(kiapi::common::types::document_specifier::Identifier::BoardFilename(name)) = header
        .as_ref()
        .and_then(|h| h.document.as_ref())
        .and_then(|d| d.identifier.as_ref())
    {
        let mut slot = slot.lock().unwrap();
        if slot.is_none() {
            *slot = Some(name.clone());
        }
    }
}
