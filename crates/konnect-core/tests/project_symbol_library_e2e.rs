mod harness;

use harness::Harness;
use konnect_sexp::schematic::{
    extract_lib_pins_for_unit, extract_symbol_instances, find_lib_symbol, read_schematic,
};
use serde_json::json;

#[tokio::test]
async fn project_registered_symbol_is_placed_embedded_and_persists() {
    let h = Harness::new();
    let project_dir = h.dir.path().join("Test");
    let project = project_dir.join("Test.kicad_pro");
    let schematic = project_dir.join("Test.kicad_sch");
    let library = project_dir.join("TestLocal.kicad_sym");

    h.json(
        "create_project",
        json!({"path": project_dir, "name": "Test"}),
    )
    .await;
    h.json(
        "create_symbol",
        json!({
            "library_path": library,
            "name": "TEST_IC",
            "reference_prefix": "U",
            "pins": [
                {"number":"1", "name":"IN", "type":"input", "x":-7.62, "y":2.54, "angle":0, "length":2.54},
                {"number":"2", "name":"GND", "type":"power_in", "x":-7.62, "y":-2.54, "angle":0, "length":2.54},
                {"number":"3", "name":"OUT", "type":"output", "x":7.62, "y":0.0, "angle":180, "length":2.54}
            ]
        }),
    )
    .await;
    h.json(
        "register_symbol_library",
        json!({
            "library_path": library,
            "nickname": "TestLocal",
            "scope": "project",
            "project": project
        }),
    )
    .await;

    let listed = h
        .json(
            "list_symbol_libraries",
            json!({"scope": "project", "project": project}),
        )
        .await;
    assert_eq!(listed["count"], 1, "project sym-lib-table must parse");
    assert_eq!(listed["libraries"][0]["nickname"], "TestLocal");

    h.json(
        "add_schematic_component",
        json!({
            "schematic": schematic,
            "lib_id": "TestLocal:TEST_IC",
            "x": 100.0,
            "y": 80.0,
            "reference": "U1"
        }),
    )
    .await;
    let placed = h
        .json(
            "get_schematic_component",
            json!({"schematic": schematic, "reference": "U1"}),
        )
        .await;
    assert_eq!(placed["reference"], "U1");
    assert_eq!(placed["lib_id"], "TestLocal:TEST_IC");

    // Re-open the persisted document and resolve its embedded definition/pins
    // through the same S-expression readers used by production analysis tools.
    let (_, reopened) = read_schematic(&schematic).expect("saved schematic reopens");
    let instances = extract_symbol_instances(&reopened);
    let instance = instances
        .iter()
        .find(|instance| instance.reference == "U1")
        .expect("U1 persists");
    assert_eq!(instance.lib_id, "TestLocal:TEST_IC");
    let lib_symbols = reopened
        .find("lib_symbols")
        .expect("embedded lib_symbols")
        .find_all("symbol");
    let embedded = find_lib_symbol(&lib_symbols, instance).expect("embedded TEST_IC definition");
    let pins = extract_lib_pins_for_unit(embedded, instance.unit);
    assert_eq!(pins.len(), 3);
    assert_eq!(
        pins.iter()
            .map(|pin| pin.number.as_str())
            .collect::<Vec<_>>(),
        vec!["1", "2", "3"]
    );
}
