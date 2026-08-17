//! The stackup write is KiCAD's gap, not ours (J.2.2.3).
//!
//! `docs/capability-matrix.md` records "write the board stackup" as
//! `GUI_ONLY_NO_API`, which keeps it out of the coverage denominator. That is a
//! strong claim to make about somebody else's software, so it is checked rather
//! than asserted in prose: KiCAD 10's own board protos, vendored here, say the
//! command is declared and not implemented.
//!
//! When KiCAD implements it, this test fails and the matrix row is wrong — which
//! is the point. Nothing else here needs to notice.

const BOARD_COMMANDS: &str = include_str!("../proto/board/board_commands.proto");

#[test]
fn kicad_10_declares_the_stackup_write_and_does_not_implement_it() {
    let declaration = BOARD_COMMANDS
        .find("message UpdateBoardStackup")
        .expect("KiCAD's board protos declare UpdateBoardStackup");

    // The disclaimer is on the comment block immediately above the message.
    let preamble = &BOARD_COMMANDS[..declaration];
    let comment = preamble
        .rfind("// Changes the stackup")
        .map(|start| &preamble[start..])
        .expect("the message carries its documentation comment");

    assert!(
        comment.contains("not yet implemented"),
        "KiCAD no longer calls UpdateBoardStackup unimplemented — the stackup \
         write may now have an API, so `MISSING`'s GUI_ONLY_NO_API row in \
         konnect_core::capability is stale:\n{comment}"
    );
}
