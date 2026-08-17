//! The operations a plan may name, and what each expands to.

use kam_plan::{ExpandError, Op, OpLibrary, Plan, StepSpec};
use konnect_sexp::geometry::{snap_to_grid, SCHEMATIC_GRID_MM};
use serde_json::{json, Map, Value};

/// KiCAD's schematic grid. Every coordinate an operation emits is a multiple
/// of this before it reaches a tool. Aliased from the one definition in
/// `konnect_sexp::geometry` rather than restated: two grid literals that drift
/// apart is E6 with extra steps.
const GRID: f64 = SCHEMATIC_GRID_MM;

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

/// Every operation's exact argument shape, next to the signature constants
/// below — this is the one place that pairs a name with its documentation,
/// so `plan::description()` and the per-operation example test in this
/// module's own `tests` block both read from it rather than retyping it.
pub const OP_LIBRARY: &[(&str, &str)] = &[
    ("create", CREATE_SIGNATURE),
    ("call", CALL_SIGNATURE),
    ("place", PLACE_SIGNATURE),
    ("power", POWER_SIGNATURE),
    ("label", LABEL_SIGNATURE),
    ("wire", WIRE_SIGNATURE),
    ("connect", CONNECT_SIGNATURE),
    ("decouple", DECOUPLE_SIGNATURE),
];

/// Every operation name, in the order they are documented. Derived from
/// [`OP_LIBRARY`] rather than restated: a second list of the same names is
/// exactly the divergence this module exists to rule out.
pub fn op_names() -> impl Iterator<Item = &'static str> {
    OP_LIBRARY.iter().map(|(name, _)| *name)
}

/// The `plan.description` schema text `preview_plan` and `apply_plan` share
/// (`tools/plan.rs::tools`), assembled from [`OP_LIBRARY`] so an operation's
/// documented shape and its expander's actual requirements are the same
/// strings, never a schema description hand-typed once and an expander
/// written separately from it.
pub fn description() -> String {
    let operations: Vec<&str> = OP_LIBRARY.iter().map(|(_, doc)| *doc).collect();
    format!(
        "A plan document: {{ops: [{{op, with, id?}}], plan_id?, documents?, rollback_policy?, \
         defaults?}}. Operations: {}. A later operation reads an earlier one's output as \
         ${{op_id.field}} — ids default to op1, op2, ... by position when not given. \
         ${{ops[N].field}} names the operation at position N instead (0-based); \
         ${{op_type.field}} (e.g. ${{create.schematic}}) names the one operation of that type, \
         when the plan has only one; a bare number such as ${{0.field}} is read as position N \
         unless an earlier operation is also literally named N, in which case it is refused as \
         ambiguous. defaults:{{key:value,...}} fills that key into every \
         operation's 'with' that omits it — write defaults:{{schematic:<path>}} once instead of \
         repeating 'schematic' in every operation. schematic? may be omitted after a single \
         earlier create, whose output it then means.",
        operations.join("; ")
    )
}

/// Operations whose `with` names a `schematic:path` they read. `create` makes
/// the schematic rather than reading one, and `call` is opaque — this module
/// does not know what its arguments mean — so neither is in this list, and
/// [`infer_omitted_schematics`] never touches either.
const NEEDS_SCHEMATIC: &[&str] = &["place", "power", "label", "wire", "connect", "decouple"];

/// Fill in a missing `schematic` with the reference a model should have
/// written, when the plan leaves exactly one operation it could mean.
///
/// E24: 8 of 60 measured refusals were `schematic ... required` in a plan
/// whose first operation was `create` — the model had already written the
/// one fact `schematic` could name, one operation earlier, and did not repeat
/// it. This resolves that shape by injecting the same `${op_id.schematic}`
/// placeholder a model would write by hand, so [`kam_plan::compile`]'s
/// reference checking (D22) runs over it unchanged, and no plan reaches the
/// executor through a path that skips it.
///
/// Only injected when exactly one earlier `create` exists. Zero, or two or
/// more, is genuine ambiguity — which one? — and is left alone, so the
/// operation's own `required` error still fires, unchanged.
///
/// Called on the plan *before* [`kam_plan::compile`], so the injected value
/// is indistinguishable from one the model wrote itself by the time the
/// op library or the reference checker ever sees it.
pub fn infer_omitted_schematics(plan: &mut Plan) {
    for index in 0..plan.ops.len() {
        if !NEEDS_SCHEMATIC.contains(&plan.ops[index].op.as_str()) {
            continue;
        }
        let has_schematic = plan.ops[index]
            .with
            .as_object()
            .is_some_and(|with| with.contains_key("schematic"));
        if has_schematic {
            continue;
        }

        let creator_id = {
            let mut creators = plan.ops[..index].iter().filter(|op| op.op == "create");
            match (creators.next(), creators.next()) {
                (Some(only), None) => Some(only.id.clone()),
                _ => None, // zero, or two-or-more: ambiguous, leave it be refused
            }
        };

        if let Some(id) = creator_id {
            if let Value::Object(with) = &mut plan.ops[index].with {
                with.insert(
                    "schematic".to_string(),
                    Value::String(format!("${{{id}.schematic}}")),
                );
            }
        }
    }
}

impl OpLibrary for KicadOps {
    fn expand(&self, op: &Op) -> Result<Vec<StepSpec>, ExpandError> {
        let steps = match op.op.as_str() {
            "create" => expand_create(&op.with)?,
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

/// `create` — a new project, from nothing.
///
/// The only operation a plan can open with when the work directory is empty:
/// every other operation needs a `schematic` that already exists. Wraps the
/// existing `create_project` tool under a name this library documents, since
/// `call` cannot rescue a model that never saw a tool catalogue (E17) — it can
/// only invoke a tool name it already knows, and `create_project` is not one
/// this library ever mentions elsewhere. Its result carries `schematic` and
/// `pcb`, so a later operation reaches them as `${op_id.schematic}`.
const CREATE_SIGNATURE: &str = "create{path:path,name:string} creates a project: a .kicad_pro, \
     empty .kicad_sch and .kicad_pcb, at 'path' named 'name'; the result's 'schematic' and 'pcb' \
     fields are then available to later operations as ${op_id.schematic} / ${op_id.pcb}";

fn expand_create(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let path = need_str(with, "path")?;
    let name = need_str(with, "name")?;
    Ok(vec![StepSpec {
        tool: "create_project".to_string(),
        args: json!({ "path": path, "name": name }),
    }])
}

/// `call` — one tool, verbatim.
///
/// The escape hatch, and the reason a plan is never less expressive than the
/// batch it replaces: anything `kicad_invoke` can call, a plan can call. It is
/// also the only operation that does no arithmetic, so a coordinate passed
/// through it is the caller's responsibility.
const CALL_SIGNATURE: &str = "call{tool:string,args?} runs any tool verbatim: 'tool' is \
     required, 'args' defaults to {}";

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
const PLACE_SIGNATURE: &str = "place{schematic?:path,components:[{lib_id:string,x?:number,\
     y?:number,reference?:string,value?:string,rotation?:number,unit?:number}],\
     at?:{x:number,y:number},pitch?:number,direction?:string} places symbols, snapped to the \
     1.27mm grid, in one call. Each component needs lib_id, plus either both x and y or neither \
     of them — a component that gives neither is laid out from 'at' (then required, as {x,y}) \
     along 'direction' (right|left|down|up, default right) at 'pitch' (default 12.7mm)";

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
const POWER_SIGNATURE: &str = "power{schematic?:path,symbols:[{net:string,x:number,y:number,\
     rotation?:number}]} adds power symbols, snapped to the grid; each symbol needs net (or \
     power_net) plus x and y";

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
const LABEL_SIGNATURE: &str = "label{schematic?:path,labels:[{net:string,x:number,y:number,\
     rotation?:number,label_type?:string,shape?:string}]} adds net labels, snapped to the grid; \
     each label needs net plus x and y";

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
const WIRE_SIGNATURE: &str = "wire{schematic?:path,segments:[{x1:number,y1:number,x2:number,\
     y2:number}],junctions?:[{x:number,y:number}]} adds snapped segments, and snapped junctions \
     where given";

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

/// `connect` — pin-to-pin or pin-to-net wiring.
///
/// Accepts `{"from": "R1.2", "to": "R2.1"}` as well as the explicit field
/// form. The compact form is not sugar: a plan is text a model writes, and
/// half the keys of a connection carry no information.
///
/// A pin may also go to a net rather than another pin — `{"from": "R1.2",
/// "to": "+3V3"}`, or `{ref1, pin1, net}` explicitly. Decided syntactically,
/// never by trying both: a compact side with no `.` names a net, one with a
/// `.` names `REF.PIN`; explicitly, `ref2` given without `pin2` cannot mean a
/// pin (a pin always needs both), so it is read as the net name instead.
const CONNECT_SIGNATURE: &str = "connect{schematic?:path,connections:[{from:string,to:string}|\
     {ref1:string,pin1:string,ref2:string,pin2:string}|{ref1:string,pin1:string,net:string}]} \
     wires a pin to a pin or a pin to a net; 'from'/'to' are compact refs like 'R1.2' (a side \
     with no '.' names a net, not a pin), or give ref1+pin1 with either ref2+pin2 (a pin) or net \
     (a net) — ref2 alone, with no pin2, is read as net; pin numbers may be given as numbers";

fn expand_connect(with: &Value) -> Result<Vec<StepSpec>, ExpandError> {
    let schematic = need_str(with, "schematic")?;
    let connections = need_array(with, "connections")?;

    connections
        .iter()
        .enumerate()
        .map(|(index, item)| {
            let field = |name: &str| format!("connections[{index}].{name}");
            match (item.get("from"), item.get("to")) {
                (Some(from), Some(to)) => expand_compact_connection(schematic, from, to, &field),
                _ => expand_explicit_connection(schematic, item, &field),
            }
        })
        .collect()
}

/// The `{from, to}` compact form. A side with a `.` is `REF.PIN`; a side
/// without one is a net name — decided by that shape alone, never by which
/// side happens to resolve, so a plan is checkable before anything runs.
fn expand_compact_connection(
    schematic: &str,
    from: &Value,
    to: &Value,
    field: &dyn Fn(&str) -> String,
) -> Result<StepSpec, ExpandError> {
    let from_s = from
        .as_str()
        .ok_or_else(|| ExpandError::invalid(field("from"), "must be a string"))?;
    let to_s = to
        .as_str()
        .ok_or_else(|| ExpandError::invalid(field("to"), "must be a string"))?;

    match (from_s.contains('.'), to_s.contains('.')) {
        (true, true) => {
            let (ref1, pin1) = split_pin(from, &field("from"))?;
            let (ref2, pin2) = split_pin(to, &field("to"))?;
            Ok(pin_to_pin_step(schematic, ref1, pin1, ref2, pin2))
        }
        (true, false) => {
            let (reference, pin) = split_pin(from, &field("from"))?;
            Ok(pin_to_net_step(schematic, reference, pin, to_s.to_string()))
        }
        (false, true) => {
            let (reference, pin) = split_pin(to, &field("to"))?;
            Ok(pin_to_net_step(
                schematic,
                reference,
                pin,
                from_s.to_string(),
            ))
        }
        (false, false) => Err(ExpandError::invalid(
            field("to"),
            "neither 'from' nor 'to' names a pin as 'REF.PIN'; one side must",
        )),
    }
}

/// The explicit-fields form: `ref1`/`pin1` always name the first pin; what
/// they connect to is either `ref2`+`pin2` (another pin), or a net — given as
/// `net`, or as `ref2` alone, since a `ref2` with no `pin2` cannot address a
/// pin.
fn expand_explicit_connection(
    schematic: &str,
    item: &Value,
    field: &dyn Fn(&str) -> String,
) -> Result<StepSpec, ExpandError> {
    let ref1 = pin_field(item, "ref1", &field("ref1"))?;
    let pin1 = pin_field(item, "pin1", &field("pin1"))?;

    if let Some(net) = item.get("net") {
        let net = net
            .as_str()
            .ok_or_else(|| ExpandError::invalid(field("net"), "must be a string"))?;
        return Ok(pin_to_net_step(schematic, ref1, pin1, net.to_string()));
    }

    match (item.get("ref2"), item.get("pin2")) {
        (Some(_), Some(_)) => {
            let ref2 = pin_field(item, "ref2", &field("ref2"))?;
            let pin2 = pin_field(item, "pin2", &field("pin2"))?;
            Ok(pin_to_pin_step(schematic, ref1, pin1, ref2, pin2))
        }
        (Some(_), None) => {
            // No pin2: ref2 cannot name a component pin, so it names a net.
            let net = pin_field(item, "ref2", &field("ref2"))?;
            Ok(pin_to_net_step(schematic, ref1, pin1, net))
        }
        (None, _) => Err(ExpandError::invalid(
            field("ref2"),
            "required — give ref2 with pin2 (a pin), or a net (via 'net', or 'ref2' alone)",
        )),
    }
}

fn pin_to_pin_step(
    schematic: &str,
    ref1: String,
    pin1: String,
    ref2: String,
    pin2: String,
) -> StepSpec {
    StepSpec {
        tool: "connect_pins".to_string(),
        args: json!({
            "schematic": schematic,
            "ref1": ref1, "pin1": pin1,
            "ref2": ref2, "pin2": pin2,
        }),
    }
}

/// Expands to `batch_connect_to_net` with one pin: the tool named
/// `connect_to_net` takes a resolved pin coordinate, which this operation
/// cannot produce without reading the schematic, so the by-reference tool is
/// the one a stateless expansion can target.
fn pin_to_net_step(schematic: &str, reference: String, pin: String, net: String) -> StepSpec {
    StepSpec {
        tool: "batch_connect_to_net".to_string(),
        args: json!({
            "schematic": schematic,
            "net_name": net,
            "pins": [{"reference": reference, "pin_number": pin}],
        }),
    }
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
const DECOUPLE_SIGNATURE: &str = "decouple{schematic?:path,ic?:string,at:{x:number,y:number},\
     caps:[{reference:string,pin:string|rail:string,value?:string}],pitch?:number,\
     ground?:string,lib_id?:string,value?:string} places a capacitor bank, wires each to its IC \
     pin and grounds it. Each cap needs reference plus exactly one of pin (wired to 'ic', which \
     is then required) or rail (given a power symbol); at is required, as {x,y}";

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
    use kam_plan::compile;
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

    // ─── A parser for the `*_SIGNATURE` DSL, for the anti-drift test below ──
    //
    // The test that follows must not just run the hand-written minimal example
    // for each operation — that only proves the example itself compiles, which
    // is also true of an example that compiles because of a field the
    // signature never mentions. What actually rules out drift is checking the
    // example, and the expander's required-field errors, against what the
    // signature string documents. That needs the signature parsed, not just
    // read by eye, so it is parsed here, once, into a flat field list every
    // check below shares.

    /// One field a `*_SIGNATURE` constant documents, flattened to a path into
    /// the operation's `with` object.
    #[derive(Debug, Clone)]
    struct DocField {
        /// Dotted/bracketed path, e.g. `components[].lib_id` or `at.x`. `[]`
        /// stands for "in each element of this array".
        path: String,
        /// The DSL marks this with a trailing `?`, or (like `args?`) gives it
        /// no type at all — either way, an example may omit it.
        optional: bool,
        /// Reachable only through one branch of a `|` — either a field-level
        /// union (`pin:string|rail:string`) or an array-of-alternatives union
        /// (`[{...}|{...}]`). A union member is never truly required on its
        /// own: some other branch may satisfy the operation instead, so the
        /// required-field check below must not demand it.
        in_union: bool,
        /// No type was given at all (`args?`), so the DSL says nothing about
        /// what lives underneath this field — an example may nest anything
        /// there without it counting as an undocumented field.
        opaque: bool,
    }

    /// Parse one `OP_LIBRARY` signature constant into its documented
    /// operation name and the flat field list it promises.
    fn parse_signature(sig: &str) -> (&str, Vec<DocField>) {
        let brace = sig.find('{').expect("signature must open with name{...}");
        let name = &sig[..brace];
        let body = balanced(&sig[brace..]);
        let mut fields = Vec::new();
        parse_object_body(&body[1..body.len() - 1], "", false, &mut fields);
        (name, fields)
    }

    /// The substring starting at `s`'s first byte (a `{` or `[`), up to and
    /// including its matching close. Only the requested bracket kind is
    /// tracked, so a `{` opened inside a `[...]` (or vice versa) does not
    /// confuse the count.
    fn balanced(s: &str) -> &str {
        let open = s.as_bytes()[0];
        let close = match open {
            b'{' => b'}',
            b'[' => b']',
            _ => panic!("balanced() called on '{s}', which opens with neither '{{' nor '['"),
        };
        let mut depth = 0i32;
        for (i, b) in s.bytes().enumerate() {
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    return &s[..=i];
                }
            }
        }
        panic!("unbalanced '{}' in signature fragment: {s}", open as char);
    }

    /// Split `s` at top-level occurrences of `sep` — never inside a `{...}`
    /// or `[...]` it contains, so a comma inside a nested object does not
    /// split the field list it belongs to, one level up.
    fn split_top_level(s: &str, sep: char) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut depth = 0i32;
        let mut start = 0usize;
        for (i, c) in s.char_indices() {
            match c {
                '{' | '[' => depth += 1,
                '}' | ']' => depth -= 1,
                c if c == sep && depth == 0 => {
                    parts.push(&s[start..i]);
                    start = i + c.len_utf8();
                }
                _ => {}
            }
        }
        parts.push(&s[start..]);
        parts
    }

    /// Parse a comma-separated field list — the inside of a signature's
    /// outer `{...}`, or of a nested object type — into flat, path-qualified
    /// fields.
    ///
    /// `prefix` is the dotted path fields here are relative to (`""` at the
    /// top, `"at."` inside `at:{...}`, `"components[]."` inside
    /// `components:[{...}]`). `parent_union` is true when this whole object
    /// was reached through a `|` branch, so every field found here is a
    /// union member too, whether or not it also has siblings joined by `|`.
    fn parse_object_body(body: &str, prefix: &str, parent_union: bool, out: &mut Vec<DocField>) {
        for chunk in split_top_level(body, ',') {
            let chunk = chunk.trim();
            if chunk.is_empty() {
                continue;
            }
            // A chunk like `pin:string|rail:string` is not one field with a
            // union type — it is two alternative fields, only one of which
            // the caller gives. Each alternative is parsed on its own.
            let alts = split_top_level(chunk, '|');
            let in_union = parent_union || alts.len() > 1;
            for alt in alts {
                parse_field(alt.trim(), prefix, in_union, out);
            }
        }
    }

    /// Parse one `key[?][:type]` field spec — never containing a top-level
    /// `|`, [`parse_object_body`] has already split those out — and recurse
    /// into its type when that type is a nested object or an array of them.
    fn parse_field(spec: &str, prefix: &str, in_union: bool, out: &mut Vec<DocField>) {
        let (key_raw, type_spec) = match spec.split_once(':') {
            Some((k, t)) => (k, Some(t)),
            None => (spec, None),
        };
        let optional = key_raw.ends_with('?');
        let key = key_raw.trim_end_matches('?');
        let path = format!("{prefix}{key}");

        out.push(DocField {
            path: path.clone(),
            optional,
            in_union,
            opaque: type_spec.is_none(),
        });

        match type_spec {
            Some(t) if t.starts_with('{') => {
                let obj = balanced(t);
                parse_object_body(&obj[1..obj.len() - 1], &format!("{path}."), in_union, out);
            }
            Some(t) if t.starts_with('[') => {
                let arr = balanced(t);
                let inner = &arr[1..arr.len() - 1];
                let alts = split_top_level(inner, '|');
                let array_union = in_union || alts.len() > 1;
                for alt in alts {
                    let alt = alt.trim();
                    if alt.starts_with('{') {
                        let obj = balanced(alt);
                        parse_object_body(
                            &obj[1..obj.len() - 1],
                            &format!("{path}[]."),
                            array_union,
                            out,
                        );
                    }
                    // A bare type name inside `[...]` (no signature here
                    // uses one, but nothing rules it out) names no
                    // sub-fields, so there is nothing further to recurse into.
                }
            }
            _ => {}
        }
    }

    /// Every field path an example actually uses, keys only (leaf scalar
    /// values contribute nothing beyond the key that names them). Mirrors
    /// [`DocField::path`]'s notation: `.` into an object, `[].` into each
    /// element of an array.
    fn used_field_paths(value: &Value, prefix: &str, out: &mut Vec<String>) {
        if let Value::Object(map) = value {
            for (k, v) in map {
                let path = format!("{prefix}{k}");
                out.push(path.clone());
                match v {
                    Value::Object(_) => used_field_paths(v, &format!("{path}."), out),
                    Value::Array(items) => {
                        for item in items {
                            used_field_paths(item, &format!("{path}[]."), out);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    /// Whether `path` currently resolves to something in `value` — checked
    /// against the array's first element, since every anti-drift example
    /// uses at most one element per array. Used to skip required-field
    /// checks for a field whose *parent* the minimal example never included
    /// in the first place (e.g. `place`'s `at.x`, when that example gives
    /// `x`/`y` directly and omits `at` entirely) — the signature's `?` on
    /// the parent already says that whole branch is optional.
    fn field_present(value: &Value, path: &str) -> bool {
        fn go(value: &Value, segments: &[&str]) -> bool {
            let Some((seg, rest)) = segments.split_first() else {
                return true;
            };
            if let Some(key) = seg.strip_suffix("[]") {
                match value.get(key) {
                    Some(Value::Array(items)) if !items.is_empty() => go(&items[0], rest),
                    _ => false,
                }
            } else if rest.is_empty() {
                value.get(*seg).is_some()
            } else {
                value.get(*seg).is_some_and(|child| go(child, rest))
            }
        }
        go(value, &path.split('.').collect::<Vec<_>>())
    }

    /// Remove whatever `path` names from `value` — `[]` removes the field
    /// from every element of the array it follows, though every anti-drift
    /// example only ever has one.
    fn remove_field(value: &mut Value, path: &str) {
        fn go(value: &mut Value, segments: &[&str]) {
            let Some((seg, rest)) = segments.split_first() else {
                return;
            };
            if let Some(key) = seg.strip_suffix("[]") {
                if let Some(Value::Array(items)) = value.get_mut(key) {
                    for item in items {
                        go(item, rest);
                    }
                }
            } else if rest.is_empty() {
                if let Value::Object(map) = value {
                    map.remove(*seg);
                }
            } else if let Some(child) = value.get_mut(*seg) {
                go(child, rest);
            }
        }
        go(value, &path.split('.').collect::<Vec<_>>());
    }

    /// The `ExpandError::field` a removal at `doc_path` should produce.
    /// Documented paths use `[]` for "any element"; every anti-drift example
    /// has exactly one, so the error it actually names is always index 0.
    fn expected_error_field(doc_path: &str) -> String {
        doc_path.replace("[]", "[0]")
    }

    #[test]
    fn create_expands_to_one_call_to_create_project() {
        let steps = calls("create", json!({"path": "/p", "name": "divider"}));
        assert_eq!(
            steps,
            vec![json!({
                "tool": "create_project",
                "args": {"path": "/p", "name": "divider"}
            })]
        );
    }

    #[test]
    fn create_needs_both_path_and_name() {
        let err = expand("create", json!({"path": "/p"})).unwrap_err();
        assert_eq!(err.field.as_deref(), Some("name"));
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
    fn connect_reads_a_dotless_compact_side_as_a_net_not_a_missing_pin() {
        // "U1" has no '.', so it names a net — decided by shape alone, never
        // by trying to look U1 up as a component.
        let steps = calls(
            "connect",
            json!({"schematic": "/p/a.kicad_sch", "connections": [{"from": "U1", "to": "C1.1"}]}),
        );
        assert_eq!(steps[0]["tool"], json!("batch_connect_to_net"));
        assert_eq!(steps[0]["args"]["net_name"], json!("U1"));
        assert_eq!(steps[0]["args"]["pins"][0]["reference"], json!("C1"));
        assert_eq!(steps[0]["args"]["pins"][0]["pin_number"], json!("1"));
    }

    #[test]
    fn connect_refuses_a_compact_connection_naming_no_pin_at_all() {
        let err = expand(
            "connect",
            json!({"schematic": "/p/a.kicad_sch", "connections": [{"from": "GND", "to": "+3V3"}]}),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("connections[0].to"));
    }

    #[test]
    fn connect_accepts_a_pin_to_net_in_the_explicit_form() {
        let steps = calls(
            "connect",
            json!({
                "schematic": "/p/a.kicad_sch",
                "connections": [{"ref1": "R1", "pin1": "2", "net": "+3V3"}]
            }),
        );
        assert_eq!(steps[0]["tool"], json!("batch_connect_to_net"));
        assert_eq!(steps[0]["args"]["net_name"], json!("+3V3"));
        assert_eq!(steps[0]["args"]["pins"][0]["reference"], json!("R1"));
        assert_eq!(steps[0]["args"]["pins"][0]["pin_number"], json!("2"));
    }

    #[test]
    fn connect_reads_a_ref2_with_no_pin2_as_a_net() {
        // Exactly what models write: a rail in 'ref2', no 'pin2' — a ref2
        // without a pin2 cannot address a component pin.
        let steps = calls(
            "connect",
            json!({
                "schematic": "/p/a.kicad_sch",
                "connections": [{"ref1": "R1", "pin1": "2", "ref2": "+3V3"}]
            }),
        );
        assert_eq!(steps[0]["tool"], json!("batch_connect_to_net"));
        assert_eq!(steps[0]["args"]["net_name"], json!("+3V3"));
    }

    #[test]
    fn connect_accepts_a_numeric_pin1_in_the_pin_to_net_form() {
        let steps = calls(
            "connect",
            json!({
                "schematic": "/p/a.kicad_sch",
                "connections": [{"ref1": "R1", "pin1": 2, "net": "+3V3"}]
            }),
        );
        assert_eq!(steps[0]["args"]["pins"][0]["pin_number"], json!("2"));
    }

    #[test]
    fn connect_refuses_an_incomplete_explicit_connection() {
        let err = expand(
            "connect",
            json!({
                "schematic": "/p/a.kicad_sch",
                "connections": [{"ref1": "R1", "pin1": "2"}]
            }),
        )
        .unwrap_err();
        assert_eq!(err.field.as_deref(), Some("connections[0].ref2"));
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

    /// One minimal example per operation, in `OP_LIBRARY` order, built from
    /// nothing but the signature constant next to each expander above.
    ///
    /// Shared by both anti-drift tests below rather than written out twice:
    /// they check the same examples from two directions — that everything an
    /// example uses is documented, and that everything documented as required
    /// really is — and two copies drifting apart would quietly weaken both.
    fn minimal_examples() -> Vec<(&'static str, Value)> {
        vec![
            ("create", json!({"path": "/p", "name": "a"})),
            (
                "call",
                json!({"tool": "run_erc", "args": {"schematic": "/p/a.kicad_sch"}}),
            ),
            (
                "place",
                json!({
                    "schematic": "/p/a.kicad_sch",
                    "components": [{"lib_id": "Device:R", "x": 100.33, "y": 80.01}]
                }),
            ),
            (
                "power",
                json!({
                    "schematic": "/p/a.kicad_sch",
                    "symbols": [{"net": "GND", "x": 100.33, "y": 80.01}]
                }),
            ),
            (
                "label",
                json!({
                    "schematic": "/p/a.kicad_sch",
                    "labels": [{"net": "VOUT", "x": 100.33, "y": 80.01}]
                }),
            ),
            (
                "wire",
                json!({
                    "schematic": "/p/a.kicad_sch",
                    "segments": [{"x1": 0.0, "y1": 0.0, "x2": 12.7, "y2": 0.0}]
                }),
            ),
            (
                "connect",
                json!({
                    "schematic": "/p/a.kicad_sch",
                    "connections": [{"from": "R1.2", "to": "R2.1"}]
                }),
            ),
            (
                "decouple",
                json!({
                    "schematic": "/p/a.kicad_sch",
                    "ic": "U1",
                    "at": {"x": 100.33, "y": 80.01},
                    "caps": [{"reference": "C1", "pin": "1"}]
                }),
            ),
        ]
    }

    #[test]
    fn every_documented_signature_has_a_working_minimal_example() {
        // One example per operation, built from nothing but the signature
        // constant next to its expander above. If an expander ever starts
        // requiring a field its signature does not mention — the bug this
        // module exists to close — the missing field makes `expand` fail
        // here, not silently in a model's hands.
        let examples = minimal_examples();
        assert_eq!(
            examples.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            OP_LIBRARY.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
            "an example above is missing, extra, or out of order relative to OP_LIBRARY"
        );
        for (name, with) in &examples {
            expand(name, with.clone()).unwrap_or_else(|e| {
                panic!("{name}: its own documented signature's example did not compile: {e}")
            });

            // The documented name and OP_LIBRARY's key for it cannot drift:
            // the parser trusts the text before the first '{' is the name.
            let (_, sig) = OP_LIBRARY.iter().find(|(n, _)| n == name).unwrap();
            assert!(
                sig.starts_with(&format!("{name}{{")),
                "{name}: its signature does not open with '{name}{{' — the documented name and \
                 OP_LIBRARY's key for it have drifted apart"
            );

            // An example that only compiles because of a field its own
            // signature never mentions is exactly the drift this test
            // exists to catch — so every key the example uses must appear,
            // at that path, in the parsed signature.
            let (_, fields) = parse_signature(sig);
            let documented: std::collections::HashSet<&str> =
                fields.iter().map(|f| f.path.as_str()).collect();
            let opaque: Vec<&str> = fields
                .iter()
                .filter(|f| f.opaque)
                .map(|f| f.path.as_str())
                .collect();
            let mut used = Vec::new();
            used_field_paths(with, "", &mut used);
            for path in &used {
                let documented_directly = documented.contains(path.as_str());
                let under_an_opaque_field = opaque.iter().any(|p| {
                    path == p
                        || path.starts_with(&format!("{p}."))
                        || path.starts_with(&format!("{p}[]"))
                });
                assert!(
                    documented_directly || under_an_opaque_field,
                    "{name}: its example uses '{path}', which its own documented signature does \
                     not mention"
                );
            }
        }
    }

    #[test]
    fn every_field_a_signature_marks_required_and_outside_a_union_is_actually_required() {
        // The complement of the check above: not just that every field the
        // example uses is documented, but that every field the signature
        // calls required — no '?', and not one branch of a '|' — really is
        // one. Removing it from the same minimal example must make `expand`
        // fail, naming that exact field, or the signature is promising more
        // than the expander enforces.
        //
        // `schematic?` is the one deliberate exception: it is documented
        // optional because `infer_omitted_schematics` fills it in before an
        // expander ever runs, even though the expander itself calls
        // `need_str(with, "schematic")` — so it is already excluded by
        // being marked optional in the signature, not by a special case here.
        let examples = minimal_examples();

        for (name, with) in &examples {
            let (_, sig) = OP_LIBRARY.iter().find(|(n, _)| n == name).unwrap();
            let (_, fields) = parse_signature(sig);
            for field in fields.iter().filter(|f| !f.optional && !f.in_union) {
                if !field_present(with, &field.path) {
                    // The field's own parent branch is not in this example
                    // (e.g. place's `at.x`, when the example gives x/y
                    // directly and omits `at` — itself documented optional).
                    continue;
                }
                let mut broken = with.clone();
                remove_field(&mut broken, &field.path);
                match expand(name, broken) {
                    Ok(_) => panic!(
                        "{name}: removing required field '{}' should have failed to expand, but \
                         didn't",
                        field.path
                    ),
                    Err(e) => assert_eq!(
                        e.field.as_deref(),
                        Some(expected_error_field(&field.path).as_str()),
                        "{name}: removing required field '{}' failed for the wrong reason: {e}",
                        field.path
                    ),
                }
            }
        }
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
    fn a_missing_schematic_resolves_when_exactly_one_create_precedes_it() {
        let mut plan = Plan::from_json(&json!({"ops": [
            {"id": "c", "op": "create", "with": {"path": "/p", "name": "a"}},
            {"op": "place", "with": {
                "components": [{"lib_id": "Device:R", "x": 100.33, "y": 80.01}]
            }}
        ]}))
        .unwrap();
        infer_omitted_schematics(&mut plan);
        assert_eq!(
            plan.ops[1].with["schematic"],
            json!("${c.schematic}"),
            "the injected reference should be the same form a model would write"
        );
        // And it must actually resolve: compile runs D22's reference checking
        // over it unchanged.
        let program = compile(&plan, &KicadOps).unwrap();
        assert!(program.has_references());
    }

    #[test]
    fn zero_creates_leaves_schematic_missing_and_still_refused() {
        let mut plan = Plan::from_json(&json!({"ops": [
            {"op": "place", "with": {
                "components": [{"lib_id": "Device:R", "x": 100.33, "y": 80.01}]
            }}
        ]}))
        .unwrap();
        infer_omitted_schematics(&mut plan);
        let err = compile(&plan, &KicadOps).unwrap_err();
        assert_eq!(err.code(), "plan_expansion_failed");
        assert!(err.to_string().contains("schematic"), "{err}");
    }

    #[test]
    fn two_creates_is_genuine_ambiguity_and_stays_refused() {
        let mut plan = Plan::from_json(&json!({"ops": [
            {"id": "c1", "op": "create", "with": {"path": "/p", "name": "a"}},
            {"id": "c2", "op": "create", "with": {"path": "/p", "name": "b"}},
            {"op": "place", "with": {
                "components": [{"lib_id": "Device:R", "x": 100.33, "y": 80.01}]
            }}
        ]}))
        .unwrap();
        infer_omitted_schematics(&mut plan);
        let err = compile(&plan, &KicadOps).unwrap_err();
        assert_eq!(err.code(), "plan_expansion_failed");
        assert!(err.to_string().contains("schematic"), "{err}");
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
