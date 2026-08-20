//! D.7.3 — the delta path is pull, and the handshake says so.
//!
//! `changes_since` exists because KiCad has no pub/sub and this server does not
//! invent one. That is a promise a client reads out of `initialize`, so it is
//! pinned here rather than left to nobody having added a subscription: a
//! `subscribe: true` that no code backs would tell a client to wait for a
//! notification that never arrives, and waiting is indistinguishable from a
//! design that has not changed.
//!
//! `tools.listChanged` is deliberately *not* included in the prohibition. It is
//! true — `load_toolset` really does emit `notifications/tools/list_changed`,
//! and `crates/konnect/tests/protocol_stdio.rs` proves it — and what it
//! describes is session state, which is not a disk mutation (D60). The rule
//! this file enforces is about the *design*: nothing here promises to tell a
//! client that a document moved.

use konnect_core::mcp::server::McpServerState;
use konnect_core::router::meta_tools;

/// Words that, in a tool description, promise the server will speak first.
const PUSH_WORDS: &[&str] = &[
    "subscribe",
    "subscription",
    "push notification",
    "notify you",
];

#[test]
fn the_handshake_promises_no_resource_subscription() {
    let result = McpServerState::build_initialize_result();
    let resources = result
        .capabilities
        .resources
        .expect("evidence handles are served as resources");

    assert_eq!(
        resources["subscribe"],
        serde_json::json!(false),
        "there is no per-resource subscription; claiming one would have clients \
         waiting on a notification nothing sends"
    );
    assert_eq!(
        resources["listChanged"],
        serde_json::json!(false),
        "the resource list changes silently as batches run; a client re-lists \
         when it follows a handle it was given"
    );
}

/// The one notification this server does send is about which tools are
/// exposed, not about what is on disk. Asserted rather than assumed, so that a
/// future change that stopped emitting it fails here instead of leaving a
/// capability advertising something that no longer happens.
#[test]
fn the_only_advertised_notification_is_the_tool_list() {
    let result = McpServerState::build_initialize_result();
    let tools = result.capabilities.tools.expect("tools are advertised");
    assert_eq!(tools.list_changed, Some(true));
    assert!(
        result.capabilities.prompts.is_none() && result.capabilities.logging.is_none(),
        "a capability this server does not implement must not appear in the handshake"
    );
}

#[test]
fn no_meta_tool_offers_to_watch_a_document() {
    for tool in meta_tools::meta_tool_descriptions() {
        let text = tool.description.to_lowercase();
        for word in PUSH_WORDS {
            assert!(
                !text.contains(word),
                "{} promises '{word}': the delta path is pull — a client asks \
                 changes_since(document, rev), and the server never speaks first",
                tool.name
            );
        }
    }
}
