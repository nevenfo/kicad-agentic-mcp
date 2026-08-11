//! The operations a plan may name, and what each expands to.

use kam_plan::{ExpandError, Op, OpLibrary, StepSpec};
use konnect_sexp::geometry::snap_to_grid;
use serde_json::{json, Map, Value};

/// KiCAD's schematic grid. Every coordinate an operation emits is a multiple
/// of this before it reaches a tool.
const GRID: f64 = 1.27;

/// Vertical offset from a `Device:C` origin to each of its pins. Pin 1 sits
/// above the origin, pin 2 below.
const CAP_PIN_OFFSET: f64 = 3.81;

/// Default spacing between auto-placed symbols, in mm. Ten grid units: close
/// enough to read as one group, far enough not to overlap a two-pin symbol.
const DEFAULT_PITCH: f64 = 12.7;

/// Default spacing between decoupling capacitors. Tighter than [`DEFAULT_PITCH`]
/// because a decoupling bank is meant to read as one block.
const DECOUPLE_PITCH: f64 = 7.62;

/// More steps than this out of one operation and the plan has stopped being
/// reviewable, which is most of what a plan is for.
const MAX_STEPS_PER_OP: usize = 200;

/// The operations this server understands.
///
/// Stateless: expansion is a pure function of the operation's parameters, which
/// is what makes `preview_plan` show exactly what `apply_plan` will run.
pub struct KicadOps;

/// Every operation name, in the order they are documented.
pub const OP_NAMES: &[&str] = &[
    "call", "place", "power", "label", "wire", "connect", "decouple",
];

impl OpLibrary for KicadOps {
    fn expand(&self, op: &Op) -> Result<Vec<StepSpec>, ExpandError> {
        let steps = match op.op.as_str() {
            "call" => expand_call(&op.with)?,
            "place" => expand_place(&op.with)?,
            "power" => expand_power(&op.with)?,
            "label" => expand_label(&op.with)?,
            "wire" => expand_wire(&op.with)?,
            "connect" => expand_connect(&op.with)?,
            "decouple" => expand_decouple(&op.with)?,
            other => return Err(ExpandError::unknown_op(other)),
        };
        if steps.len() > MAX_STEPS_PER_OP {
            return Err(ExpandError::invalid(
                "with",
                format!(
                    "this operation would expand to {} tool calls, over the limit of \
                     {MAX_STEPS_PER_OP}; split it into several operations",
                    steps.len()
                ),
            ));
        }
        Ok(steps)
    }
}

// ─── The operations ──────────────────────────────────────────────────────────

/// `call` — one tool, verbatim.
///
/// The escape hatch, and the reason a plan is never less expressive than the
/// batch it replaces: anything `kicad_invoke` can call, a plan can call. It is
/// also the only operation that does no arithmetic, so a coordinate passed
/// through it is the caller's responsibility.
fn expand_call(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let tool = need_str(with, "tool")?;
    Ok(vec![StepSpec {
        tool: tool.to_string(),
        args: with.get("args").cloned().unwrap_or_else(|| json!({})),
    }])
}

/// `place` — symbols, laid out on the grid.
///
/// A component may give its own `x`/`y`, or omit them and be placed from `at`
/// along `direction` at `pitch`. Either way the result is snapped, and the
/// whole set becomes one `batch_place_components` — one file read/write cycle
/// rather than one per symbol.
fn expand_place(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let components = need_array(with, "components")?;
    let pitch = optional_f64(with, "pitch")?.unwrap_or(DEFAULT_PITCH);
    let (dx, dy) = direction(with)?;
    let anchor = optional_point(with, "at")?;

    let mut placed = Vec::with_capacity(components.len());
    let mut auto_index = 0usize;

    for (index, item) in components.iter().enumerate() {
        let field = |name: &str| format!("components[{index}].{name}");
        let lib_id = item
            .get("lib_id")
            .and_then(Value::as_str)
            .ok_or_else(|| ExpandError::invalid(field("lib_id"), "required"))?;

        let (x, y) = match (item.get("x"), item.get("y")) {
            (Some(x), Some(y)) => (coordinate(x, &field("x"))?, coordinate(y, &field("y"))?),
            (None, None) => {
                let (ax, ay) = anchor.ok_or_else(|| {
                    ExpandError::invalid(
                        "at",
                        format!(
                            "component {index} gives no x/y, so the operation needs an 'at' \
                             anchor to lay out from"
                        ),
                    )
                })?;
                let step = auto_index as f64;
                auto_index += 1;
                (ax + dx * pitch * step, ay + dy * pitch * step)
            }
            _ => {
                return Err(ExpandError::invalid(
                    field("x"),
                    "give both x and y, or neither",
                ))
            }
        };

        let mut entry = Map::new();
        entry.insert("lib_id".to_string(), json!(lib_id));
        entry.insert("x".to_string(), json!(snap(x)));
        entry.insert("y".to_string(), json!(snap(y)));
        for key in ["reference", "value", "rotation", "unit"] {
            if let Some(v) = item.get(key) {
                entry.insert(key.to_string(), v.clone());
            }
        }
        placed.push(Value::Object(entry));
    }

    Ok(vec![StepSpec {
        tool: "batch_place_components".to_string(),
        args: json!({ "schematic": schematic, "components": placed }),
    }])
}

/// `power` — power symbols, on the grid.
///
/// This is the operation E6 exists for. `add_power_symbol` writes the
/// coordinate it is handed, unsnapped, so a power symbol placed at the same
/// nominal position as a resistor lands 0.33 mm off its pin and ERC reports
/// both as unconnected — with no tool error anywhere. Routing power symbols
/// through a plan makes that unreachable.
fn expand_power(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let symbols = need_array(with, "symbols")?;

    symbols
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let field = |name: &str| format!("symbols[{index}].{name}");
            let net = item
                .get("net")
                .or_else(|| item.get("power_net"))
                .and_then(Value::as_str)
                .ok_or_else(|| ExpandError::invalid(field("net"), "required"))?;
            let x = coordinate(
                item.get("x")
                    .ok_or_else(|| ExpandError::invalid(field("x"), "required"))?,
                &field("x"),
            )?;
            let y = coordinate(
                item.get("y")
                    .ok_or_else(|| ExpandError::invalid(field("y"), "required"))?,
                &field("y"),
            )?;
            let mut args = json!({
                "schematic": schematic,
                "power_net": net,
                "x": snap(x),
                "y": snap(y),
            });
            if let Some(rotation) = item.get("rotation") {
                args["rotation"] = rotation.clone();
            }
            Ok(StepSpec {
                tool: "add_power_symbol".to_string(),
                args,
            })
        })
        .collect()
}

/// `label` — net labels, on the grid.
fn expand_label(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let labels = need_array(with, "labels")?;

    labels
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let field = |name: &str| format!("labels[{index}].{name}");
            let net = item
                .get("net")
                .and_then(Value::as_str)
                .ok_or_else(|| ExpandError::invalid(field("net"), "required"))?;
            let x = coordinate(
                item.get("x")
                    .ok_or_else(|| ExpandError::invalid(field("x"), "required"))?,
                &field("x"),
            )?;
            let y = coordinate(
                item.get("y")
                    .ok_or_else(|| ExpandError::invalid(field("y"), "required"))?,
                &field("y"),
            )?;
            let mut args = json!({
                "schematic": schematic,
                "net": net,
                "x": snap(x),
                "y": snap(y),
            });
            for key in ["rotation", "label_type", "shape"] {
                if let Some(v) = item.get(key) {
                    args[key] = v.clone();
                }
            }
            Ok(StepSpec {
                tool: "add_schematic_net_label".to_string(),
                args,
            })
        })
        .collect()
}

/// `wire` — segments and their junctions, on the grid.
fn expand_wire(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let segments = need_array(with, "segments")?;

    let wires: Result<Vec<Value>, ExpandError> = segments
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let mut ends = Map::new();
            for key in ["x1", "y1", "x2", "y2"] {
                let field = format!("segments[{index}].{key}");
                let raw = item
                    .get(key)
                    .ok_or_else(|| ExpandError::invalid(field.clone(), "required"))?;
                ends.insert(key.to_string(), json!(snap(coordinate(raw, &field)?)));
            }
            Ok(Value::Object(ends))
        })
        .collect();

    let mut steps = vec![StepSpec {
        tool: "batch_add_wire".to_string(),
        args: json!({ "schematic": schematic, "wires": wires? }),
    }];

    if let Some(Value::Array(points)) = with.get("junctions") {
        let positions: Result<Vec<Value>, ExpandError> = points
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let mut point = Map::new();
                for key in ["x", "y"] {
                    let field = format!("junctions[{index}].{key}");
                    let raw = item
                        .get(key)
                        .ok_or_else(|| ExpandError::invalid(field.clone(), "required"))?;
                    point.insert(key.to_string(), json!(snap(coordinate(raw, &field)?)));
                }
                Ok(Value::Object(point))
            })
            .collect();
        steps.push(StepSpec {
            tool: "batch_add_junction".to_string(),
            args: json!({ "schematic": schematic, "positions": positions? }),
        });
    }

    Ok(steps)
}

/// `connect` — pin-to-pin wiring.
///
/// Accepts `{"from": "R1.2", "to": "R2.1"}` as well as the four-field form.
/// The compact form is not sugar: a plan is text a model writes, and half the
/// keys of a connection carry no information.
fn expand_connect(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let connections = need_array(with, "connections")?;

    connections
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let field = |name: &str| format!("connections[{index}].{name}");
            let (ref1, pin1, ref2, pin2) = match (item.get("from"), item.get("to")) {
                (Some(from), Some(to)) => {
                    let (r1, p1) = split_pin(from, &field("from"))?;
                    let (r2, p2) = split_pin(to, &field("to"))?;
                    (r1, p1, r2, p2)
                }
                _ => (
                    pin_field(item, "ref1", &field("ref1"))?,
                    pin_field(item, "pin1", &field("pin1"))?,
                    pin_field(item, "ref2", &field("ref2"))?,
                    pin_field(item, "pin2", &field("pin2"))?,
                ),
            };
            Ok(StepSpec {
                tool: "connect_pins".to_string(),
                args: json!({
                    "schematic": schematic,
                    "ref1": ref1, "pin1": pin1,
                    "ref2": ref2, "pin2": pin2,
                }),
            })
        })
        .collect()
}

/// `decouple` — a bank of capacitors placed, wired and grounded.
///
/// The archetype: the caller says which of an IC's pins want decoupling and
/// where the bank goes; the operation works out the positions, the connections
/// and the ground symbols. For four capacitors that is one operation instead of
/// nine tool calls the caller would otherwise have to enumerate — and, since
/// each capacitor's ground symbol sits at a computed offset from the capacitor's
/// own origin, it is nine calls whose geometry cannot drift.
///
/// Each capacitor names either the `pin` it decouples or the `rail` it sits on,
/// never both: the first is wired to the IC, the second gets a power symbol.
///
/// What this does not do is decide whether the decoupling is *right*. Values,
/// counts and placement quality are design questions; this is the mechanical
/// half, and `verify` is what says whether the result holds.
fn expand_decouple(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let caps = need_array(with, "caps")?;
    // Required only when something is wired to it. A bank of capacitors that
    // all sit on a rail names no IC pin, and demanding an `ic` it would never
    // use would be a schema asking for a fact to satisfy the schema.
    let ic = if caps.iter().any(|cap| cap.get("pin").is_some()) {
        need_str(with, "ic")?
    } else {
        with.get("ic").and_then(Value::as_str).unwrap_or_default()
    };
    let (ax, ay) = need_point(with, "at")?;
    let pitch = optional_f64(with, "pitch")?.unwrap_or(DECOUPLE_PITCH);
    let ground = with
        .get("ground")
        .and_then(Value::as_str)
        .unwrap_or("GND")
        .to_string();
    let lib_id = with
        .get("lib_id")
        .and_then(Value::as_str)
        .unwrap_or("Device:C")
        .to_string();
    let default_value = with
        .get("value")
        .and_then(Value::as_str)
        .unwrap_or("100nF")
        .to_string();

    let mut placements = Vec::with_capacity(caps.len());
    let mut connects = Vec::new();
    let mut powers = Vec::new();

    for (index, cap) in caps.iter().enumerate() {
        let field = |name: &str| format!("caps[{index}].{name}");
        let reference = cap
            .get("reference")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                ExpandError::invalid(
                    field("reference"),
                    "required — this operation does not invent reference designators, because \
                     it cannot see which ones the schematic already uses",
                )
            })?;
        let value = cap
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or(&default_value);

        let x = snap(ax + pitch * index as f64);
        let y = snap(ay);

        placements.push(json!({
            "lib_id": lib_id,
            "reference": reference,
            "value": value,
            "x": x,
            "y": y,
        }));

        match (cap.get("pin"), cap.get("rail")) {
            (Some(pin), None) => {
                let pin = pin.as_str().map(str::to_string).unwrap_or_else(|| {
                    // A pin number written as a number rather than a string is
                    // the commonest way to get this wrong, and refusing it
                    // would be pedantry: '3' and 3 name the same pin.
                    pin.to_string()
                });
                connects.push(StepSpec {
                    tool: "connect_pins".to_string(),
                    args: json!({
                        "schematic": schematic,
                        "ref1": reference, "pin1": "1",
                        "ref2": ic, "pin2": pin,
                    }),
                });
            }
            (None, Some(rail)) => {
                let rail = rail
                    .as_str()
                    .ok_or_else(|| ExpandError::invalid(field("rail"), "must be a net name"))?;
                powers.push(StepSpec {
                    tool: "add_power_symbol".to_string(),
                    args: json!({
                        "schematic": schematic,
                        "power_net": rail,
                        "x": x,
                        "y": snap(y - CAP_PIN_OFFSET),
                    }),
                });
            }
            _ => {
                return Err(ExpandError::invalid(
                    field("pin"),
                    "give exactly one of 'pin' (wire this capacitor to that IC pin) or 'rail' \
                     (put a power symbol on it)",
                ))
            }
        }

        powers.push(StepSpec {
            tool: "add_power_symbol".to_string(),
            args: json!({
                "schematic": schematic,
                "power_net": ground,
                "x": x,
                "y": snap(y + CAP_PIN_OFFSET),
            }),
        });
    }

    if placements.is_empty() {
        return Err(ExpandError::invalid("caps", "needs at least one capacitor"));
    }

    // Placement first, in one call: `connect_pins` looks the pins up by
    // reference, so every capacitor has to exist before any wiring starts.
    let mut steps = Vec::with_capacity(1 + connects.len() + powers.len());
    steps.push(StepSpec {
        tool: "batch_place_components".to_string(),
        args: json!({ "schematic": schematic, "components": placements }),
    });
    steps.extend(connects);
    steps.extend(powers);
    Ok(steps)
}

// ─── Argument helpers ────────────────────────────────────────────────────────

/// Snap to the schematic grid, then round off the float noise the division
/// leaves behind — 100.32999999999998 is the same point as 100.33 to every
/// tolerance in this system and a worse thing to read in a preview.
fn snap(value: f64) -> f64 {
    (snap_to_grid(value, GRID) * 1e6).round() / 1e6
}

fn need_str<'a>(with: &'a Value, field: &str) -> Result<&'a str, ExpandError> {
    with.get(field)
        .and_then(Value::as_str)
        .ok_or_else(|| ExpandError::invalid(field, "required"))
}

fn need_array<'a>(with: &'a Value, field: &str) -> Result<&'a Vec<Value>, ExpandError> {
    match with.get(field) {
        Some(Value::Array(items)) if !items.is_empty() => Ok(items),
        Some(Value::Array(_)) => Err(ExpandError::invalid(field, "must not be empty")),
        _ => Err(ExpandError::invalid(field, "required, as an array")),
    }
}

fn optional_f64(with: &Value, field: &str) -> Result<Option<f64>, ExpandError> {
    match with.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => coordinate(v, field).map(Some),
    }
}

/// A coordinate must be a number here, never a `${...}` reference.
///
/// The operation is about to do arithmetic on it and snap the result, and a
/// value that only exists at run time cannot be snapped at compile time. Rather
/// than silently pass such a value through — which would put a hole in the one
/// guarantee this module makes — it is refused, and `call` remains available
/// for a caller who really does want a coordinate from a previous result.
fn coordinate(value: &Value, field: &str) -> Result<f64, ExpandError> {
    value.as_f64().ok_or_else(|| {
        ExpandError::invalid(
            field,
            "must be a number; a plan operation snaps coordinates to the grid at compile time, \
             so it cannot take one from a previous step's output — use the 'call' operation for \
             that",
        )
    })
}

fn optional_point(with: &Value, field: &str) -> Result<Option<(f64, f64)>, ExpandError> {
    match with.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(_) => need_point(with, field).map(Some),
    }
}

fn need_point(with: &Value, field: &str) -> Result<(f64, f64), ExpandError> {
    let point = with
        .get(field)
        .ok_or_else(|| ExpandError::invalid(field, "required, as {x, y}"))?;
    let x = point
        .get("x")
        .ok_or_else(|| ExpandError::invalid(format!("{field}.x"), "required"))?;
    let y = point
        .get("y")
        .ok_or_else(|| ExpandError::invalid(format!("{field}.y"), "required"))?;
    Ok((
        coordinate(x, &format!("{field}.x"))?,
        coordinate(y, &format!("{field}.y"))?,
    ))
}

fn direction(with: &Value) -> Result<(f64, f64), ExpandError> {
    match with.get("direction").and_then(Value::as_str) {
        None | Some("right") => Ok((1.0, 0.0)),
        Some("left") => Ok((-1.0, 0.0)),
        Some("down") => Ok((0.0, 1.0)),
        Some("up") => Ok((0.0, -1.0)),
        Some(other) => Err(ExpandError::invalid(
            "direction",
            format!("'{other}' is not one of right, left, down, up"),
        )),
    }
}

fn pin_field(item: &Value, key: &str, field: &str) -> Result<String, ExpandError> {
    match item.get(key) {
        Some(Value::String(s)) => Ok(s.clone()),
        Some(Value::Number(n)) => Ok(n.to_string()),
        _ => Err(ExpandError::invalid(field, "required")),
    }
}

fn split_pin(value: &Value, field: &str) -> Result<(String, String), ExpandError> {
    let text = value
        .as_str()
        .ok_or_else(|| ExpandError::invalid(field, "must be a string like 'R1.2'"))?;
    // Split at the last dot: a reference may contain one, a pin number may not.
    let Some((reference, pin)) = text.rsplit_once('.') else {
        return Err(ExpandError::invalid(
            field,
            format!("'{text}' must name a reference and a pin, e.g. 'U1.3'"),
        ));
    };
    if reference.is_empty() || pin.is_empty() {
        return Err(ExpandError::invalid(
            field,
            format!("'{text}' must name a reference and a pin, e.g. 'U1.3'"),
        ));
    }
    Ok((reference.to_string(), pin.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use kam_plan::{compile, Plan};
    use serde_json::json;

    fn expand(op: &str, with: Value) -> Result<Vec<StepSpec>, ExpandError> {
        KicadOps.expand(&Op {
            id: "t".to_string(),
            op: op.to_string(),
            with,
        })
    }

    fn calls(op: &str, with: Value) -> Vec<Value> {
        expand(op, with)
            .unwrap()
            .into_iter()
            .map(|s| json!({"tool": s.tool, "args": s.args}))
            .collect()
    }

    #[test]
    fn call_passes_a_tool_through_unchanged() {
        let steps = calls(
            "call",
            json!({"tool": "run_erc", "args": {"schematic": "/p/a.kicad_sch"}}),
        );
        assert_eq!(
            steps,
            vec![json!({"tool": "run_erc", "args": {"schematic": "/p/a.kicad_sch"}})]
        );
    }

    #[test]
    fn place_snaps_every_coordinate_it_is_given() {
        let steps = calls(
            "place",
            json!({
                "schematic": "/p/a.kicad_sch",
                "components": [{"lib_id": "Device:R", "reference": "R1", "x": 100.0, "y": 80.4}]
            }),
        );
        let placed = &steps[0]["args"]["components"][0];
        assert_eq!(placed["x"], json!(100.33));
        assert_eq!(placed["y"], json!(80.01));
        assert_eq!(placed["reference"], json!("R1"));
    }

    #[test]
    fn place_lays_out_from_an_anchor_when_a_component_gives_no_position() {
        let steps = calls(
            "place",
            json!({
                "schematic": "/p/a.kicad_sch",
                "at": {"x": 100.33, "y": 80.01},
                "pitch": 12.7,
                "components": [
                    {"lib_id": "Device:R", "reference": "R1"},
                    {"lib_id": "Device:R", "reference": "R2"},
                    {"lib_id": "Device:C", "reference": "C1"}
                ]
            }),
        );
        let xs: Vec<f64> = steps[0]["args"]["components"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["x"].as_f64().unwrap())
            .collect();
        assert_eq!(xs, vec![100.33, 113.03, 125.73]);
    }

    #[test]
    fn place_can_lay_out_downwards() {
        let steps = calls(
            "place",
            json!({
                "schematic": "/p/a.kicad_sch",
                "at": {"x": 100.33, "y": 80.01},
                "pitch": 15.24,
                "direction": "down",
                "components": [{"lib_id": "Device:R"}, {"lib_id": "Device:R"}]
            }),
        );
        let second = &steps[0]["args"]["components"][1];
        assert_eq!(second["x"], json!(100.33));
        assert_eq!(second["y"], json!(95.25));
    }

    #[test]
    fn place_becomes_one_call_however_many_symbols() {
        let steps = calls(
            "place",
            json!({
                "schematic": "/p/a.kicad_sch",
                "at": {"x": 0, "y": 0},
                "components": (0..12).map(|_| json!({"lib_id": "Device:R"})).collect::<Vec<_>>()
            }),
        );
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["args"]["components"].as_array().unwrap().len(), 12);
    }

    #[test]
    fn place_refuses_half_a_position() {
        let err = expand(
            "place",
            json!({
                "schematic": "/p/a.kicad_sch",
                "components": [{"lib_id": "Device:R", "x": 10}]
            }),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("components[0].x"));
    }

    #[test]
    fn power_symbols_are_snapped_which_is_the_whole_point() {
        // E6: add_power_symbol writes the coordinate verbatim, so a power
        // symbol at a resistor's nominal position lands 0.33 mm off its pin and
        // ERC reports both pins unconnected, with no tool error anywhere.
        let steps = calls(
            "power",
            json!({
                "schematic": "/p/a.kicad_sch",
                "symbols": [{"net": "+3V3", "x": 100.0, "y": 76.0}, {"net": "GND", "x": 100.0, "y": 99.0}]
            }),
        );
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["args"]["x"], json!(100.33));
        assert_eq!(steps[0]["args"]["y"], json!(76.2));
        assert_eq!(steps[1]["args"]["power_net"], json!("GND"));
        assert_eq!(steps[1]["args"]["y"], json!(99.06));
    }

    #[test]
    fn connect_accepts_the_compact_form() {
        let steps = calls(
            "connect",
            json!({
                "schematic": "/p/a.kicad_sch",
                "connections": [{"from": "R1.2", "to": "R2.1"}]
            }),
        );
        assert_eq!(
            steps[0]["args"],
            json!({"schematic": "/p/a.kicad_sch", "ref1": "R1", "pin1": "2", "ref2": "R2", "pin2": "1"})
        );
    }

    #[test]
    fn connect_accepts_the_explicit_form_and_a_numeric_pin() {
        let steps = calls(
            "connect",
            json!({
                "schematic": "/p/a.kicad_sch",
                "connections": [{"ref1": "U1", "pin1": 3, "ref2": "C1", "pin2": "1"}]
            }),
        );
        assert_eq!(steps[0]["args"]["pin1"], json!("3"));
    }

    #[test]
    fn connect_splits_at_the_last_dot() {
        let steps = calls(
            "connect",
            json!({"schematic": "/p/a.kicad_sch", "connections": [{"from": "U1.A.7", "to": "C1.1"}]}),
        );
        assert_eq!(steps[0]["args"]["ref1"], json!("U1.A"));
        assert_eq!(steps[0]["args"]["pin1"], json!("7"));
    }

    #[test]
    fn connect_refuses_a_pin_it_cannot_find() {
        let err = expand(
            "connect",
            json!({"schematic": "/p/a.kicad_sch", "connections": [{"from": "U1", "to": "C1.1"}]}),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("connections[0].from"));
    }

    #[test]
    fn wire_snaps_both_ends_and_adds_junctions_as_a_second_call() {
        let steps = calls(
            "wire",
            json!({
                "schematic": "/p/a.kicad_sch",
                "segments": [{"x1": 86.0, "y1": 88.0, "x2": 94.0, "y2": 88.0}],
                "junctions": [{"x": 86.0, "y": 88.0}]
            }),
        );
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0]["args"]["wires"][0]["x1"], json!(86.36));
        assert_eq!(steps[1]["tool"], json!("batch_add_junction"));
        assert_eq!(steps[1]["args"]["positions"][0]["y"], json!(87.63));
        // The junction lands on the same snapped point as the segment it
        // belongs to, which is the property that makes it a junction at all.
        assert_eq!(
            steps[1]["args"]["positions"][0]["y"],
            steps[0]["args"]["wires"][0]["y1"]
        );
    }

    #[test]
    fn decouple_places_once_then_wires_then_grounds() {
        let steps = calls(
            "decouple",
            json!({
                "schematic": "/p/a.kicad_sch",
                "ic": "U1",
                "at": {"x": 120.65, "y": 80.01},
                "caps": [
                    {"reference": "C10", "pin": "1"},
                    {"reference": "C11", "pin": "5"}
                ]
            }),
        );
        let tools: Vec<&str> = steps.iter().map(|s| s["tool"].as_str().unwrap()).collect();
        assert_eq!(
            tools,
            [
                "batch_place_components",
                "connect_pins",
                "connect_pins",
                "add_power_symbol",
                "add_power_symbol"
            ]
        );
        // Two capacitors: five calls from one operation, and the caller wrote
        // neither a coordinate nor a ground symbol.
        assert_eq!(steps[0]["args"]["components"].as_array().unwrap().len(), 2);
        assert_eq!(steps[1]["args"]["ref2"], json!("U1"));
        assert_eq!(steps[1]["args"]["pin2"], json!("1"));
    }

    #[test]
    fn decouple_grounds_each_capacitor_at_its_own_pin_2() {
        let steps = calls(
            "decouple",
            json!({
                "schematic": "/p/a.kicad_sch",
                "ic": "U1",
                "at": {"x": 120.65, "y": 80.01},
                "pitch": 7.62,
                "caps": [{"reference": "C10", "pin": "1"}, {"reference": "C11", "pin": "5"}]
            }),
        );
        let grounds: Vec<(f64, f64)> = steps
            .iter()
            .filter(|s| s["tool"] == json!("add_power_symbol"))
            .map(|s| {
                (
                    s["args"]["x"].as_f64().unwrap(),
                    s["args"]["y"].as_f64().unwrap(),
                )
            })
            .collect();
        assert_eq!(grounds, vec![(120.65, 83.82), (128.27, 83.82)]);
    }

    #[test]
    fn a_capacitor_on_a_rail_gets_a_power_symbol_instead_of_a_wire() {
        // And no `ic`: nothing here is wired to one, so requiring the name
        // would be the schema asking for a fact only to satisfy itself.
        let steps = calls(
            "decouple",
            json!({
                "schematic": "/p/a.kicad_sch",
                "at": {"x": 120.65, "y": 80.01},
                "caps": [{"reference": "C10", "rail": "+3V3"}]
            }),
        );
        let tools: Vec<&str> = steps.iter().map(|s| s["tool"].as_str().unwrap()).collect();
        assert_eq!(
            tools,
            [
                "batch_place_components",
                "add_power_symbol",
                "add_power_symbol"
            ]
        );
        assert_eq!(steps[1]["args"]["power_net"], json!("+3V3"));
        assert_eq!(steps[1]["args"]["y"], json!(76.2));
    }

    #[test]
    fn decouple_still_needs_an_ic_when_a_capacitor_names_a_pin() {
        let err = expand(
            "decouple",
            json!({
                "schematic": "/p/a.kicad_sch", "at": {"x": 0, "y": 0},
                "caps": [{"reference": "C10", "pin": "1"}]
            }),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("ic"));
    }

    #[test]
    fn decouple_refuses_a_capacitor_that_names_neither_a_pin_nor_a_rail() {
        let err = expand(
            "decouple",
            json!({
                "schematic": "/p/a.kicad_sch", "ic": "U1", "at": {"x": 0, "y": 0},
                "caps": [{"reference": "C10"}]
            }),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("caps[0].pin"));
    }

    #[test]
    fn decouple_will_not_invent_a_reference_designator() {
        let err = expand(
            "decouple",
            json!({
                "schematic": "/p/a.kicad_sch", "ic": "U1", "at": {"x": 0, "y": 0},
                "caps": [{"pin": "1"}]
            }),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("caps[0].reference"));
    }

    #[test]
    fn a_coordinate_from_a_previous_step_is_refused_rather_than_left_unsnapped() {
        let err = expand(
            "power",
            json!({
                "schematic": "/p/a.kicad_sch",
                "symbols": [{"net": "GND", "x": "${place.x}", "y": 80.01}]
            }),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("symbols[0].x"));
        assert!(err.reason.contains("'call'"));
    }

    #[test]
    fn an_unknown_operation_names_itself() {
        let err = expand("levitate", json!({})).unwrap_err();
        assert_eq!(err.code, "unknown_op");
    }

    #[test]
    fn an_operation_that_would_explode_is_refused() {
        let symbols: Vec<Value> = (0..MAX_STEPS_PER_OP + 1)
            .map(|i| json!({"net": "GND", "x": i as f64, "y": 0.0}))
            .collect();
        let err = expand(
            "power",
            json!({"schematic": "/p/a.kicad_sch", "symbols": symbols}),
        )
        .unwrap_err();
        assert!(err.reason.contains("over the limit"));
    }

    #[test]
    fn a_whole_divider_is_four_operations_and_five_calls() {
        // The shape the benchmark's first golden task has, written as a plan.
        // Nothing here is a coordinate the caller had to snap, and nothing is a
        // round trip: the plan is one payload.
        let plan = Plan::from_json(&json!({
            "plan_id": "divider",
            "documents": ["/p/divider.kicad_sch"],
            "ops": [
                {"op": "place", "with": {
                    "schematic": "/p/divider.kicad_sch",
                    "components": [
                        {"lib_id": "Device:R", "reference": "R1", "value": "10k", "x": 100.33, "y": 80.01},
                        {"lib_id": "Device:R", "reference": "R2", "value": "10k", "x": 100.33, "y": 95.25},
                        {"lib_id": "power:PWR_FLAG", "reference": "#FLG01", "x": 100.33, "y": 76.2},
                        {"lib_id": "power:PWR_FLAG", "reference": "#FLG02", "x": 100.33, "y": 99.06}
                    ]
                }},
                {"op": "power", "with": {
                    "schematic": "/p/divider.kicad_sch",
                    "symbols": [
                        {"net": "+3V3", "x": 100.33, "y": 76.2},
                        {"net": "GND", "x": 100.33, "y": 99.06}
                    ]
                }},
                {"op": "connect", "with": {
                    "schematic": "/p/divider.kicad_sch",
                    "connections": [{"from": "R1.2", "to": "R2.1"}]
                }},
                {"op": "label", "with": {
                    "schematic": "/p/divider.kicad_sch",
                    "labels": [{"net": "VOUT", "x": 100.33, "y": 87.63}]
                }}
            ]
        }))
        .unwrap();

        let program = compile(&plan, &KicadOps).unwrap();
        assert_eq!(program.steps.len(), 5);
        assert!(!program.has_references());
        assert_eq!(program.summary()["ops"], json!(4));
    }
}
