use konnect_schematic_editor::{
    sexp::{parser, writer},
    Error, Schematic,
};

// ---- S-expression parser round-trip ----------------------------------------

#[test]
fn parse_simple_list() {
    let node = parser::parse("(kicad_sch (version 20231120))").unwrap();
    assert_eq!(node.tag(), Some("kicad_sch"));
    let ver = node.find("version").unwrap();
    assert_eq!(ver.value(), Some("20231120"));
}

#[test]
fn parse_quoted_string() {
    let node = parser::parse(r#"(property "Reference" "C1")"#).unwrap();
    assert_eq!(node.tag(), Some("property"));
    let args = node.scalar_args();
    assert_eq!(args, vec!["Reference", "C1"]);
}

#[test]
fn parse_escaped_string() {
    let node = parser::parse(r#"(text "hello \"world\"")"#).unwrap();
    assert_eq!(node.value(), Some("hello \"world\""));
}

#[test]
fn writer_round_trip() {
    let input = r#"(kicad_sch (version 20231120) (generator "kicad"))"#;
    let node = parser::parse(input).unwrap();
    let out = writer::write(&node);
    // Re-parse and check structure is preserved
    let node2 = parser::parse(out.trim()).unwrap();
    assert_eq!(node2.tag(), Some("kicad_sch"));
    assert_eq!(node2.find("version").unwrap().value(), Some("20231120"));
    assert_eq!(node2.find("generator").unwrap().value(), Some("kicad"));
}

#[test]
fn parse_nested() {
    let src = r#"(symbol (lib_id "Device:R") (at 100.33 88.9 90) (unit 1))"#;
    let node = parser::parse(src).unwrap();
    assert_eq!(node.tag(), Some("symbol"));
    assert_eq!(node.get_value("lib_id"), Some("Device:R"));
    let at = node.find("at").unwrap();
    let scalars = at.scalar_args();
    assert_eq!(scalars, vec!["100.33", "88.9", "90"]);
}

// ---- Schematic from string --------------------------------------------------

fn minimal_sch() -> &'static str {
    r#"(kicad_sch
  (version 20231120)
  (generator "test")
  (uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
  (paper "A4")

  (symbol
    (lib_id "Device:R")
    (at 100 100 0)
    (unit 1)
    (in_bom yes)
    (on_board yes)
    (dnp no)
    (uuid "11111111-0000-0000-0000-000000000001")
    (property "Reference" "R1"
      (at 100 95 0)
    )
    (property "Value" "10k"
      (at 100 105 0)
    )
    (property "Footprint" "Resistor_SMD:R_0402"
      (at 100 110 0)
    )
    (property "Datasheet" ""
      (at 100 115 0)
    )
  )

  (symbol
    (lib_id "Device:C")
    (at 150 100 0)
    (unit 1)
    (in_bom yes)
    (on_board yes)
    (dnp no)
    (uuid "22222222-0000-0000-0000-000000000002")
    (property "Reference" "C1"
      (at 150 95 0)
    )
    (property "Value" "100nF"
      (at 150 105 0)
    )
    (property "Footprint" "Capacitor_SMD:C_0402"
      (at 150 110 0)
    )
    (property "Datasheet" ""
      (at 150 115 0)
    )
  )

  (wire
    (pts
      (xy 90 100)
      (xy 100 100)
    )
    (stroke (width 0) (type default))
    (uuid "33333333-0000-0000-0000-000000000003")
  )

  (label "VCC"
    (at 90 100 180)
    (uuid "44444444-0000-0000-0000-000000000004")
  )

  (junction
    (at 100 100)
    (diameter 0)
    (uuid "55555555-0000-0000-0000-000000000005")
  )
)"#
}

fn load_minimal() -> Schematic {
    // Each call gets its own tempfile. The file is deleted when this function
    // returns — `Schematic::load` has already pulled the content into memory.
    // Previously this used a fixed shared path, which raced under parallel
    // test execution and caused spurious "Unexpected end of input" parse
    // errors.
    let tmp = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(tmp.path(), minimal_sch()).unwrap();
    Schematic::load(tmp.path()).unwrap()
}

/// Create a persistent tempfile seeded with the minimal schematic.
/// Callers mutate + overwrite + re-load, so they must keep the returned
/// `NamedTempFile` alive for the duration of the test.
fn fresh_minimal_file() -> tempfile::NamedTempFile {
    let tmp = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(tmp.path(), minimal_sch()).unwrap();
    tmp
}

#[test]
fn load_symbol_count() {
    let sch = load_minimal();
    assert_eq!(sch.symbols.len(), 2);
}

#[test]
fn load_wire_count() {
    let sch = load_minimal();
    assert_eq!(sch.wires.len(), 1);
}

#[test]
fn load_label_count() {
    let sch = load_minimal();
    assert_eq!(sch.labels.len(), 1);
}

#[test]
fn load_junction_count() {
    let sch = load_minimal();
    assert_eq!(sch.junctions.len(), 1);
}

#[test]
fn symbol_by_reference() {
    let sch = load_minimal();
    let r1 = sch.symbols.by_reference("R1").expect("R1 not found");
    assert_eq!(r1.lib_id, "Device:R");
    assert_eq!(r1.value_str(), Some("10k"));
    assert_eq!(r1.footprint(), Some("Resistor_SMD:R_0402"));
}

#[test]
fn symbol_properties() {
    let sch = load_minimal();
    let c1 = sch.symbols.by_reference("C1").expect("C1 not found");
    assert_eq!(c1.property("Value"), Some("100nF"));
    assert_eq!(c1.property("Footprint"), Some("Capacitor_SMD:C_0402"));
}

#[test]
fn symbol_position() {
    let sch = load_minimal();
    let r1 = sch.symbols.by_reference("R1").expect("R1 not found");
    assert_eq!(r1.position(), (100.0, 100.0));
}

#[test]
fn symbol_booleans() {
    let sch = load_minimal();
    let r1 = sch.symbols.by_reference("R1").unwrap();
    assert!(r1.in_bom);
    assert!(r1.on_board);
    assert!(!r1.dnp);
}

#[test]
fn mutate_property_round_trips() {
    let tmp = fresh_minimal_file();

    {
        let mut sch = Schematic::load(tmp.path()).unwrap();
        let r1 = sch.symbols.by_reference_mut("R1").unwrap();
        r1.set_value_str("4.7k");
        r1.dnp = true;
        sch.overwrite().unwrap();
    }

    let sch2 = Schematic::load(tmp.path()).unwrap();
    let r1 = sch2.symbols.by_reference("R1").unwrap();
    assert_eq!(r1.value_str(), Some("4.7k"));
    assert!(r1.dnp);
}

#[test]
fn set_all_dnp() {
    let tmp = fresh_minimal_file();

    let mut sch = Schematic::load(tmp.path()).unwrap();
    sch.symbols.set_all_dnp(true);
    for sym in &sch.symbols {
        assert!(sym.dnp);
    }
}

#[test]
fn reference_startswith_filter() {
    let sch = load_minimal();
    let caps = sch.symbols.reference_startswith("C");
    assert_eq!(caps.len(), 1);
    assert_eq!(caps[0].reference(), Some("C1"));

    let resistors = sch.symbols.reference_startswith("R");
    assert_eq!(resistors.len(), 1);
}

#[test]
fn wire_properties() {
    let sch = load_minimal();
    let w = sch.wires.get(0).unwrap();
    assert_eq!(w.start, (90.0, 100.0));
    assert_eq!(w.end, (100.0, 100.0));
    assert!(w.is_horizontal());
    assert!(!w.is_vertical());
    assert!((w.length() - 10.0).abs() < 1e-9);
}

#[test]
fn wire_touches() {
    let sch = load_minimal();
    let w = sch.wires.get(0).unwrap();
    assert!(w.touches(90.0, 100.0));
    assert!(w.touches(100.0, 100.0));
    assert!(!w.touches(95.0, 100.0));
}

#[test]
fn spatial_within_circle() {
    let sch = load_minimal();
    // R1 is at (100,100), C1 at (150,100) — radius 10 from R1 should find R1 only
    let found = sch.within_circle(100.0, 100.0, 10.0);
    let sym_count = found
        .iter()
        .filter(|e| matches!(e, konnect_schematic_editor::LocatedElement::Symbol(_)))
        .count();
    assert_eq!(sym_count, 1);
}

#[test]
fn spatial_within_rectangle() {
    let sch = load_minimal();
    // Box that covers both symbols
    let found = sch.within_rectangle(80.0, 80.0, 200.0, 120.0);
    let sym_count = found
        .iter()
        .filter(|e| matches!(e, konnect_schematic_editor::LocatedElement::Symbol(_)))
        .count();
    assert_eq!(sym_count, 2);
}

#[test]
fn add_wire_and_save() {
    let tmp = fresh_minimal_file();

    let wire_count_before;
    {
        let sch = Schematic::load(tmp.path()).unwrap();
        wire_count_before = sch.wires.len();
    }

    {
        let mut sch = Schematic::load(tmp.path()).unwrap();
        sch.add_wire(100.0, 100.0, 150.0, 100.0);
        sch.overwrite().unwrap();
    }

    let sch2 = Schematic::load(tmp.path()).unwrap();
    assert_eq!(sch2.wires.len(), wire_count_before + 1);
}

#[test]
fn add_label_and_save() {
    let tmp = fresh_minimal_file();

    {
        let mut sch = Schematic::load(tmp.path()).unwrap();
        sch.add_label("GND", 100.0, 110.0);
        sch.overwrite().unwrap();
    }

    let sch2 = Schematic::load(tmp.path()).unwrap();
    assert!(!sch2.labels.value_contains("GND").is_empty());
}

#[test]
fn overwrite_rejects_a_stale_loaded_revision() {
    let tmp = fresh_minimal_file();
    let mut schematic = Schematic::load(tmp.path()).unwrap();
    let external_revision = minimal_sch().replace("10k", "22k");
    std::fs::write(tmp.path(), &external_revision).unwrap();
    schematic
        .symbols
        .by_reference_mut("R1")
        .unwrap()
        .set_value_str("4.7k");

    let error = schematic.overwrite().unwrap_err();

    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        std::fs::read_to_string(tmp.path()).unwrap(),
        external_revision
    );
}

#[test]
fn repeated_overwrites_advance_the_owned_revision_baseline() {
    let tmp = fresh_minimal_file();
    let mut schematic = Schematic::load(tmp.path()).unwrap();
    schematic
        .symbols
        .by_reference_mut("R1")
        .unwrap()
        .set_value_str("4.7k");
    schematic.overwrite().unwrap();

    schematic
        .symbols
        .by_reference_mut("R1")
        .unwrap()
        .set_value_str("1k");
    schematic.overwrite().unwrap();

    let reloaded = Schematic::load(tmp.path()).unwrap();
    assert_eq!(
        reloaded.symbols.by_reference("R1").unwrap().value_str(),
        Some("1k")
    );
}

#[test]
fn overwrite_still_rejects_an_external_change_after_a_successful_save() {
    let tmp = fresh_minimal_file();
    let mut schematic = Schematic::load(tmp.path()).unwrap();
    schematic
        .symbols
        .by_reference_mut("R1")
        .unwrap()
        .set_value_str("4.7k");
    schematic.overwrite().unwrap();

    let external_revision = std::fs::read_to_string(tmp.path())
        .unwrap()
        .replace("4.7k", "22k");
    std::fs::write(tmp.path(), &external_revision).unwrap();
    schematic
        .symbols
        .by_reference_mut("R1")
        .unwrap()
        .set_value_str("1k");

    let error = schematic.overwrite().unwrap_err();

    assert!(matches!(error, Error::Conflict(_)));
    assert_eq!(
        std::fs::read_to_string(tmp.path()).unwrap(),
        external_revision
    );
}

#[test]
fn save_as_refuses_to_replace_an_existing_document() {
    let source = fresh_minimal_file();
    let destination = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create destination");
    std::fs::write(destination.path(), "keep this document").unwrap();
    let schematic = Schematic::load(source.path()).unwrap();

    let error = schematic.save(destination.path()).unwrap_err();

    assert!(matches!(error, Error::Io(_)));
    assert_eq!(
        std::fs::read_to_string(destination.path()).unwrap(),
        "keep this document"
    );
}

#[test]
fn diff_detects_value_change() {
    let tmp = fresh_minimal_file();

    let mut sch = Schematic::load(tmp.path()).unwrap();
    sch.symbols
        .by_reference_mut("R1")
        .unwrap()
        .set_value_str("1k");

    let cs = sch.diff_against_disk().unwrap();
    assert!(!cs.is_empty());
    let summary = cs.summary();
    assert!(summary.contains("R1"));
    assert!(summary.contains("Value"));
}

#[test]
fn changeset_display() {
    use konnect_schematic_editor::ChangeSet;
    let mut cs = ChangeSet::new();
    cs.record("R1.Value: \"10k\" → \"4.7k\"");
    cs.record("R1: dnp false → true");
    assert_eq!(cs.len(), 2);
    assert!(cs.summary().contains("R1.Value"));
}

// ---- (paper …) round-trip ---------------------------------------------------

/// Load a one-off schematic whose only interesting content is its paper node.
fn sch_with_paper(paper_line: &str) -> Schematic {
    let src = format!(
        "(kicad_sch\n  (version 20250114)\n  (generator \"eeschema\")\n  \
         (generator_version \"10.0\")\n  \
         (uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")\n  {paper_line}\n  \
         (lib_symbols\n  )\n)"
    );
    let tmp = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(tmp.path(), src).unwrap();
    Schematic::load(tmp.path()).unwrap()
}

#[test]
fn custom_paper_keeps_its_dimensions() {
    // `User` is the one page size whose width and height are mandatory. Writing
    // a bare `(paper "User")` produces a file KiCAD refuses to load, with no
    // diagnostic beyond "Failed to load schematic" — and KiCAD's own EasyEDA
    // importer lands every imported sheet on `User`.
    let sch = sch_with_paper(r#"(paper "User" 292.1 205.105)"#);
    assert_eq!(sch.paper.as_deref(), Some("User"));

    let out = sch.to_source();
    assert!(
        out.contains(r#"(paper "User" 292.1 205.105)"#),
        "custom page dimensions must survive a parse -> write cycle:\n{out}"
    );
}

#[test]
fn custom_paper_survives_repeated_round_trips() {
    let mut sch = sch_with_paper(r#"(paper "User" 292.1 205.105)"#);
    for _ in 0..3 {
        let tmp = tempfile::Builder::new()
            .suffix(".kicad_sch")
            .tempfile()
            .expect("create tempfile");
        std::fs::write(tmp.path(), sch.to_source()).unwrap();
        sch = Schematic::load(tmp.path()).unwrap();
    }
    assert!(
        sch.to_source().contains(r#"(paper "User" 292.1 205.105)"#),
        "dimensions must not erode across successive edits"
    );
}

#[test]
fn portrait_paper_keeps_its_orientation() {
    let sch = sch_with_paper(r#"(paper "A4" portrait)"#);
    assert_eq!(sch.paper.as_deref(), Some("A4"));

    let out = sch.to_source();
    assert!(
        out.contains(r#"(paper "A4" portrait)"#),
        "the portrait flag must survive a parse -> write cycle:\n{out}"
    );
}

#[test]
fn named_paper_gains_no_extra_tokens() {
    let out = sch_with_paper(r#"(paper "A4")"#).to_source();
    assert!(
        out.contains(r#"(paper "A4")"#),
        "a plain named page must round-trip unchanged:\n{out}"
    );
}

// ---- unmodelled-token preservation (#143) -----------------------------------

/// A symbol block in the shape eeschema writes for a locally edited library
/// symbol: `lib_name` ahead of `lib_id`, plus tokens the typed model does not
/// reconstruct field-by-field.
fn derived_symbol_sch() -> &'static str {
    r#"(kicad_sch
  (version 20250114)
  (generator "eeschema")
  (uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
  (paper "A4")
  (lib_symbols
    (symbol "Device:R"
      (property "Reference" "R" (at 2.032 0 90))
    )
    (symbol "R_1"
      (property "Reference" "R" (at 2.032 0 90))
    )
  )
  (symbol
    (lib_name "R_1")
    (lib_id "Device:R")
    (at 88.9 63.5 0)
    (unit 1)
    (exclude_from_sim no)
    (in_bom yes)
    (on_board yes)
    (dnp no)
    (uuid "44444444-0002-4111-8111-111111111111")
    (property "Reference" "R2" (at 91.44 62.23 0))
    (property "Value" "22k" (at 91.44 64.77 0))
    (convert 2)
    (default_instance
      (reference "R")
      (unit 1)
    )
    (pin "1" (uuid "55555555-0003-4111-8111-111111111111"))
    (instances
      (project "derived"
        (path "/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee" (reference "R2") (unit 1))
      )
    )
  )
)"#
}

fn load_derived() -> Schematic {
    let tmp = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(tmp.path(), derived_symbol_sch()).unwrap();
    Schematic::load(tmp.path()).unwrap()
}

#[test]
fn lib_name_survives_a_load_and_save_round_trip() {
    // The whole point of #143: dropping this silently re-points the symbol at
    // the base definition and rewires the netlist.
    let out = load_derived().to_source();
    assert!(
        out.contains(r#"(lib_name "R_1")"#),
        "lib_name must survive the round-trip:\n{out}"
    );
}

#[test]
fn unmodelled_symbol_children_survive_a_round_trip() {
    let out = load_derived().to_source();
    for token in ["(exclude_from_sim no)", "(convert 2)", "(default_instance"] {
        assert!(out.contains(token), "{token} was dropped:\n{out}");
    }
}

#[test]
fn lib_name_is_parsed_into_its_own_field() {
    let sch = load_derived();
    let r2 = sch.symbols.by_reference("R2").unwrap();
    assert_eq!(r2.lib_name.as_deref(), Some("R_1"));
    assert_eq!(r2.lib_id, "Device:R");
}

#[test]
fn lib_symbol_name_prefers_lib_name_over_lib_id() {
    let sch = load_derived();
    // The derived entry is what KiCAD resolves through; lib_id is provenance.
    assert_eq!(
        sch.symbols.by_reference("R2").unwrap().lib_symbol_name(),
        "R_1"
    );
    // A symbol with no lib_name falls back to lib_id.
    let plain = load_minimal();
    assert_eq!(
        plain.symbols.by_reference("R1").unwrap().lib_symbol_name(),
        "Device:R"
    );
}

#[test]
fn lib_name_is_written_before_lib_id_like_eeschema() {
    let out = load_derived().to_source();
    let lib_name = out.find(r#"(lib_name "R_1")"#).expect("lib_name emitted");
    let lib_id = out.find(r#"(lib_id "Device:R")"#).expect("lib_id emitted");
    assert!(
        lib_name < lib_id,
        "eeschema writes lib_name ahead of lib_id:\n{out}"
    );
}

#[test]
fn exclude_from_sim_keeps_its_eeschema_position() {
    let out = load_derived().to_source();
    let unit = out.find("(unit 1)").expect("unit emitted");
    let excl = out
        .find("(exclude_from_sim no)")
        .expect("exclude_from_sim emitted");
    let in_bom = out.find("(in_bom yes)").expect("in_bom emitted");
    assert!(unit < excl && excl < in_bom, "wrong ordering:\n{out}");
}

#[test]
fn a_symbol_without_exclude_from_sim_does_not_gain_one() {
    // Older files omit the token; inventing it on save would be a silent
    // format upgrade.
    let out = load_minimal().to_source();
    assert!(!out.contains("exclude_from_sim"), "{out}");
}

#[test]
fn editing_one_symbol_does_not_corrupt_the_others() {
    // The reported failure mode: a single add/edit re-serializes every symbol
    // in the file, so untouched symbols lost their lib_name too.
    let mut sch = load_derived();
    sch.symbols.by_reference_mut("R2").unwrap().dnp = true;
    let out = sch.to_source();
    assert!(out.contains(r#"(lib_name "R_1")"#), "{out}");
    assert!(out.contains("(dnp yes)"), "{out}");
}

/// A symbol whose unmodelled tokens are scattered *between* the modelled ones,
/// which is how KiCad may write them: `pin` ahead of `at`, `convert` before
/// `uuid`, `default_instance` between two properties.
fn interleaved_symbol_sch() -> &'static str {
    r#"(kicad_sch
  (version 20250114)
  (generator "eeschema")
  (uuid "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee")
  (paper "A4")
  (lib_symbols
    (symbol "Device:R"
      (property "Reference" "R" (at 2.032 0 90))
    )
  )
  (symbol
    (pin "1" (uuid "55555555-0003-4111-8111-111111111111"))
    (lib_id "Device:R")
    (at 88.9 63.5 0)
    (convert 2)
    (unit 1)
    (exclude_from_sim no)
    (in_bom yes)
    (on_board yes)
    (dnp no)
    (uuid "44444444-0002-4111-8111-111111111111")
    (property "Reference" "R2" (at 91.44 62.23 0))
    (default_instance
      (reference "R")
      (unit 1)
    )
    (property "Value" "22k" (at 91.44 64.77 0))
    (instances
      (project "interleaved"
        (path "/aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee" (reference "R2") (unit 1))
      )
    )
  )
)"#
}

#[test]
fn raw_and_typed_symbol_children_survive_interleaving() {
    // Contract asserted here is *survival and uniqueness*, not byte-identity:
    // `to_sexp` emits the preserved raw children after the typed fields, so a
    // symbol whose unmodelled tokens were interleaved comes back with them
    // grouped at the end. KiCad's parser is order-insensitive for these, so
    // this is valid — but it does mean a round-trip is not byte-faithful for
    // such files. Re-interleaving to the original positions is deliberately
    // left as follow-up; this test pins today's behaviour so that change is a
    // conscious one.
    let tmp = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(tmp.path(), interleaved_symbol_sch()).unwrap();
    let out = Schematic::load(tmp.path()).unwrap().to_source();

    for token in [
        "(lib_id \"Device:R\")",
        "(convert 2)",
        "(exclude_from_sim no)",
        "(default_instance",
        "(pin \"1\"",
        "(instances",
        "(property \"Reference\" \"R2\"",
        "(property \"Value\" \"22k\"",
    ] {
        assert_eq!(
            out.matches(token).count(),
            1,
            "{token} must appear exactly once — not dropped, not duplicated by \
             both the typed path and raw_sub_nodes:\n{out}"
        );
    }

    // Re-parsing the output must yield the same field values.
    let reparsed = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(reparsed.path(), &out).unwrap();
    let sch = Schematic::load(reparsed.path()).unwrap();
    let r2 = sch.symbols.by_reference("R2").unwrap();
    assert_eq!(r2.lib_id, "Device:R");
    assert_eq!(r2.lib_name, None);
    assert_eq!(r2.exclude_from_sim, Some(false));
    assert_eq!(r2.unit, 1);
    assert_eq!(r2.value_str(), Some("22k"));
}

// ---- P.6.9.4: the writer reproduces the sheet's own formatting -------------
//
// The demo-corpus measurement in `konnect-core`'s conformance suite compares
// with `str::lines()`, which strips a trailing `\r`. It therefore proves the
// indent/paren/blank-line half of P.6.9.4 and says nothing at all about line
// endings — these tests cover the axes that measurement cannot see.

fn load_source(src: &str) -> Schematic {
    let tmp = tempfile::Builder::new()
        .suffix(".kicad_sch")
        .tempfile()
        .expect("create tempfile");
    std::fs::write(tmp.path(), src).unwrap();
    Schematic::load(tmp.path()).unwrap()
}

/// A KiCAD 10 sheet as eeschema writes it: one tab per level, closing paren
/// alone on its own line, no blank lines.
fn kicad_shaped_sch(newline: &str) -> String {
    let lines = [
        "(kicad_sch",
        "\t(version 20250114)",
        "\t(generator \"eeschema\")",
        "\t(generator_version \"10.0\")",
        "\t(uuid \"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\")",
        "\t(paper \"A4\")",
        "\t(lib_symbols",
        "\t)",
        "\t(junction",
        "\t\t(at 50.8 50.8)",
        "\t\t(diameter 0)",
        "\t\t(uuid \"bbbbbbbb-cccc-dddd-eeee-ffffffffffff\")",
        "\t)",
        "\t(embedded_fonts no)",
        ")",
    ];
    let mut out = lines.join(newline);
    out.push_str(newline);
    out
}

#[test]
fn a_crlf_sheet_is_written_back_as_crlf() {
    // Every KiCAD 10 demo sheet shipped by the Windows installer is CRLF.
    // Writing plain LF into one reproduces the exact symptom P.6.9.4 fixes —
    // the whole document in the diff — just via line endings instead of
    // indentation.
    let sch = load_source(&kicad_shaped_sch("\r\n"));
    let out = sch.to_source();
    assert!(out.contains("\r\n"), "CRLF source lost its CRLF:\n{out:?}");
    assert_eq!(
        out.matches('\n').count(),
        out.matches("\r\n").count(),
        "a bare LF leaked into a CRLF document:\n{out:?}"
    );
}

#[test]
fn an_lf_sheet_is_written_back_as_lf() {
    let sch = load_source(&kicad_shaped_sch("\n"));
    let out = sch.to_source();
    assert!(!out.contains('\r'), "LF source gained a CR:\n{out:?}");
}

#[test]
fn the_indent_unit_is_taken_from_the_source() {
    let tabbed = load_source(&kicad_shaped_sch("\n")).to_source();
    assert!(
        tabbed.contains("\n\t(paper \"A4\")"),
        "tab-indented source did not stay tab-indented:\n{tabbed}"
    );
    assert!(
        !tabbed.contains("\n  (paper"),
        "tab-indented source picked up space indentation:\n{tabbed}"
    );

    let spaced_src = kicad_shaped_sch("\n").replace('\t', "    ");
    let spaced = load_source(&spaced_src).to_source();
    assert!(
        spaced.contains("\n    (paper \"A4\")"),
        "four-space source did not stay four-space:\n{spaced}"
    );
    assert!(
        !spaced.contains('\t'),
        "space-indented source picked up a tab:\n{spaced}"
    );
}

#[test]
fn a_multi_line_node_closes_on_its_own_line_and_no_blank_lines_appear() {
    let out = load_source(&kicad_shaped_sch("\n")).to_source();
    assert!(
        out.contains("\t\t(uuid \"bbbbbbbb-cccc-dddd-eeee-ffffffffffff\")\n\t)"),
        "closing paren did not land alone at the parent's depth:\n{out}"
    );
    assert!(
        !out.contains("\n\n"),
        "a blank line was inserted where KiCAD writes none:\n{out}"
    );
    assert!(
        out.ends_with(")\n") && !out.ends_with(")\n\n"),
        "document does not end with a single newline after the root:\n{out:?}"
    );
}
