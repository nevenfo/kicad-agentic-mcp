//! `sch_analysis` toolset — net connectivity, pin queries, trace paths, overlap/orphan detection.
//!
//! All operations are read-only S-expression analysis.
//! Net graph uses union-find (O(W+L+P)), matching net_analysis.py.

use crate::mcp::error::ToolErrorKind;
use crate::mcp::protocol::CallToolResult;
use crate::tool;
use crate::tools::{get_path, opt_f64, require_f64, require_str, ToolContext, ToolDef};
use konnect_schematic_editor as cse;
use konnect_sexp::{
    geometry::{point_on_segment, points_coincident},
    schematic::{
        extract_all_net_labels, extract_junction_points, extract_power_symbol_labels,
        extract_symbol_instances, extract_wires, find_lib_symbol, parse_at, read_schematic, Wire,
    },
};
use serde_json::json;
use std::collections::{HashMap, HashSet};

pub fn tools() -> Vec<ToolDef> {
    vec![
        tool!(
            "list_schematic_wires",
            "List all wire segments in a schematic with start/end coordinates and UUIDs.",
            json!({ "type": "object",
                "properties": { "schematic": { "type": "string" } },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_list_wires(args, ctx).await }
        ),
        tool!(
            "list_schematic_nets",
            "List all distinct net names derived from net labels, global labels, and power symbols.",
            json!({ "type": "object",
                "properties": { "schematic": { "type": "string" } },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_list_nets(args, ctx).await }
        ),
        tool!(
            "list_schematic_labels",
            "List all label instances (net_label, global_label, hierarchical_label) \
             with their positions, net names, types, and uuids — a label's uuid is \
             what addresses it in delete_schematic_net_label and rotate_schematic_label.",
            json!({ "type": "object",
                "properties": { "schematic": { "type": "string" } },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_list_labels(args, ctx).await }
        ),
        tool!(
            "get_net_connections",
            "Get all pins and labels connected to a named net.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "net": { "type": "string", "description": "Net name to query" }
                },
                "required": ["schematic", "net"] }),
            |args, ctx| async move { handle_get_net_connections(args, ctx).await }
        ),
        tool!(
            "get_net_connectivity",
            "Build the full connectivity graph for a net using union-find. \
             Returns all wire segments, labels, and T-junction locations.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "net": { "type": "string" }
                },
                "required": ["schematic", "net"] }),
            |args, ctx| async move { handle_get_net_connectivity(args, ctx).await }
        ),
        tool!(
            "get_pin_connections",
            "Get the net connected to a specific pin on a component by tracing wires from the pin endpoint.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" },
                    "pin_number": { "type": "string" }
                },
                "required": ["schematic", "reference", "pin_number"] }),
            |args, ctx| async move { handle_get_pin_connections(args, ctx).await }
        ),
        tool!(
            "get_pin_net_name",
            "Return just the net name for a specific pin on a component.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" },
                    "pin_number": { "type": "string" }
                },
                "required": ["schematic", "reference", "pin_number"] }),
            |args, ctx| async move { handle_get_pin_connections(args, ctx).await }
        ),
        tool!(
            "get_component_nets",
            "Get all nets connected to every pin of a component.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "reference": { "type": "string" }
                },
                "required": ["schematic", "reference"] }),
            |args, ctx| async move { handle_get_component_nets(args, ctx).await }
        ),
        tool!(
            "get_net_components",
            "Get all components (and their pins) connected to a named net.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "net": { "type": "string" }
                },
                "required": ["schematic", "net"] }),
            |args, ctx| async move { handle_get_net_components(args, ctx).await }
        ),
        tool!(
            "trace_from_point",
            "Trace connectivity from any (X,Y) point — returns what is at that point and the net it belongs to.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "x": { "type": "number" }, "y": { "type": "number" },
                    "tolerance": { "type": "number", "default": 0.05 }
                },
                "required": ["schematic", "x", "y"] }),
            |args, ctx| async move { handle_trace_from_point(args, ctx).await }
        ),
        tool!(
            "find_orphan_items",
            "Find dangling wire ends, floating labels, and unconnected pin endpoints (0.05mm tolerance).",
            json!({ "type": "object",
                "properties": { "schematic": { "type": "string" } },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_find_orphan_items(args, ctx).await }
        ),
        tool!(
            "find_shorted_nets",
            "Detect accidentally merged nets — pairs of distinct net names sharing a wire path.",
            json!({ "type": "object",
                "properties": { "schematic": { "type": "string" } },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_find_shorted_nets(args, ctx).await }
        ),
        tool!(
            "find_single_pin_nets",
            "Find symbol pins with no wire or label and no explicit no-connect marker.",
            json!({ "type": "object",
                "properties": { "schematic": { "type": "string" } },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_find_single_pin_nets(args, ctx).await }
        ),
        tool!(
            "get_connected_items",
            "Get all wires, labels, and components connected to a given component reference \
             by tracing net connectivity from each of its pins.",
            json!({
                "type": "object",
                "properties": {
                    "schematic": { "type": "string", "description": "Path to .kicad_sch file" },
                    "reference": { "type": "string", "description": "Component reference designator (e.g. 'R1')" }
                },
                "required": ["schematic", "reference"]
            }),
            |args, ctx| async move { handle_get_connected_items(args, ctx).await }
        ),
        tool!(
            "check_schematic_overlaps",
            "Find overlapping symbols or labels that may indicate placement errors.",
            json!({ "type": "object",
                "properties": {
                    "schematic": { "type": "string" },
                    "tolerance": { "type": "number", "default": 0.5 }
                },
                "required": ["schematic"] }),
            |args, ctx| async move { handle_check_overlaps(args, ctx).await }
        ),
    ]
}

// ─── Union-Find net graph ─────────────────────────────────────────────────────

pub(crate) fn pt_key(x: f64, y: f64) -> (i64, i64) {
    ((x * 1000.0).round() as i64, (y * 1000.0).round() as i64)
}

pub(crate) struct NetGraph {
    pub(crate) point_nets: HashMap<(i64, i64), String>,
    pub(crate) parent: HashMap<(i64, i64), (i64, i64)>,
}

impl NetGraph {
    pub(crate) fn new() -> Self {
        NetGraph {
            point_nets: HashMap::new(),
            parent: HashMap::new(),
        }
    }

    pub(crate) fn ensure(&mut self, k: (i64, i64)) {
        self.parent.entry(k).or_insert(k);
    }

    pub(crate) fn find(&mut self, k: (i64, i64)) -> (i64, i64) {
        self.ensure(k);
        let p = self.parent[&k];
        if p == k {
            return k;
        }
        let root = self.find(p);
        self.parent.insert(k, root);
        root
    }

    pub(crate) fn union(&mut self, a: (i64, i64), b: (i64, i64)) {
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent.insert(rb, ra);
        }
    }

    pub(crate) fn add_wire(&mut self, w: &Wire) {
        let a = pt_key(w.x1, w.y1);
        let b = pt_key(w.x2, w.y2);
        self.ensure(a);
        self.ensure(b);
        self.union(a, b);
    }

    pub(crate) fn add_label(&mut self, x: f64, y: f64, net: &str) {
        let k = pt_key(x, y);
        self.ensure(k);
        self.point_nets.insert(k, net.to_string());
    }

    pub(crate) fn net_at(&mut self, x: f64, y: f64) -> Option<String> {
        let k = pt_key(x, y);
        self.ensure(k);
        let root = self.find(k);
        let labels: Vec<_> = self.point_nets.clone().into_iter().collect();
        for (lk, net) in labels {
            if self.find(lk) == root {
                return Some(net);
            }
        }
        None
    }

    pub(crate) fn points_on_net(&mut self, net: &str) -> Vec<(i64, i64)> {
        // Collect keys first to avoid simultaneous borrow of point_nets and self.find()
        let net_keys: Vec<(i64, i64)> = self
            .point_nets
            .iter()
            .filter(|(_, n)| n.as_str() == net)
            .map(|(k, _)| *k)
            .collect();
        let net_roots: HashSet<(i64, i64)> = net_keys.iter().map(|k| self.find(*k)).collect();
        let all_keys: Vec<(i64, i64)> = self.parent.keys().cloned().collect();
        all_keys
            .into_iter()
            .filter(|k| net_roots.contains(&self.find(*k)))
            .collect()
    }
}

pub(crate) fn build_net_graph(
    wires: &[Wire],
    labels: &[konnect_sexp::schematic::Label],
    junctions: &[(f64, f64)],
) -> NetGraph {
    let mut g = NetGraph::new();
    for w in wires {
        g.add_wire(w);
    }
    // Labels and junction dots connect anywhere along a wire, not only at
    // endpoints — union each such point with the segment it sits on.
    // ponytail: O(P×W) scan; fine at schematic scale, index wires if it hurts.
    let attach = |g: &mut NetGraph, x: f64, y: f64| {
        for w in wires {
            if point_on_segment(x, y, w.x1, w.y1, w.x2, w.y2, 0.01) {
                g.union(pt_key(x, y), pt_key(w.x1, w.y1));
            }
        }
    };
    for l in labels {
        g.add_label(l.x, l.y, &l.net);
        attach(&mut g, l.x, l.y);
    }
    for &(jx, jy) in junctions {
        g.ensure(pt_key(jx, jy));
        attach(&mut g, jx, jy);
    }
    g
}

/// Splice power-symbol net labels into a `cse::Schematic`-derived label list.
///
/// `cse::Schematic` (via `sch_bridge`) doesn't parse power symbols yet, so
/// callers on that path re-read the sexp tree once and extend with
/// `extract_power_symbol_labels` — the same net-graph fix as the
/// `extract_all_net_labels` tree-based call sites, applied at the `sch_bridge`
/// boundary instead of inside `konnect-schematic-editor` (#262).
fn with_power_symbol_labels(
    sch_path: &std::path::Path,
    mut labels: Vec<konnect_sexp::schematic::Label>,
) -> anyhow::Result<Vec<konnect_sexp::schematic::Label>> {
    let (_, tree) = read_schematic(sch_path)?;
    labels.extend(extract_power_symbol_labels(&tree));
    Ok(labels)
}

// ─── Handlers ─────────────────────────────────────────────────────────────────

async fn handle_list_wires(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let sch = cse::Schematic::load(&sch_path)?;
    let items: Vec<serde_json::Value> = sch.wires.iter()
        .map(|w| json!({ "x1": w.start.0, "y1": w.start.1, "x2": w.end.0, "y2": w.end.1, "uuid": w.uuid }))
        .collect();
    Ok(CallToolResult::json(
        &json!({ "count": items.len(), "wires": items }),
    ))
}

async fn handle_list_nets(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let sch = cse::Schematic::load(&sch_path)?;
    let (_, tree) = read_schematic(&sch_path)?;
    let mut nets: Vec<String> = sch
        .labels
        .iter()
        .map(|l| l.text.clone())
        .chain(sch.global_labels.iter().map(|l| l.text.clone()))
        .chain(sch.hierarchical_labels.iter().map(|l| l.text.clone()))
        // Power symbols (power:GND, power:+3V3, ...) name the net they touch
        // via their placed Value, same as a label does (#262).
        .chain(
            extract_power_symbol_labels(&tree)
                .into_iter()
                .map(|l| l.net),
        )
        .collect();
    nets.sort();
    nets.dedup();
    Ok(CallToolResult::json(
        &json!({ "count": nets.len(), "nets": nets }),
    ))
}

async fn handle_list_labels(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let sch = cse::Schematic::load(&sch_path)?;
    let mut items: Vec<serde_json::Value> = Vec::new();
    for l in sch.labels.iter() {
        items.push(json!({ "net": l.text, "type": "NetLabel", "x": l.at.x, "y": l.at.y, "rotation": l.at.rotation.unwrap_or(0.0), "uuid": l.uuid }));
    }
    for g in sch.global_labels.iter() {
        items.push(json!({ "net": g.text, "type": "GlobalLabel", "x": g.at.x, "y": g.at.y, "rotation": g.at.rotation.unwrap_or(0.0), "uuid": g.uuid }));
    }
    for h in sch.hierarchical_labels.iter() {
        items.push(json!({ "net": h.text, "type": "HierarchicalLabel", "x": h.at.x, "y": h.at.y, "rotation": h.at.rotation.unwrap_or(0.0), "uuid": h.uuid }));
    }
    Ok(CallToolResult::json(
        &json!({ "count": items.len(), "labels": items }),
    ))
}

async fn handle_get_net_connections(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let net = match require_str(args, "net") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let sch = cse::Schematic::load(&sch_path)?;
    let wires = super::sch_bridge::all_wires_as_sexp(&sch);
    let labels = with_power_symbol_labels(&sch_path, super::sch_bridge::all_labels_as_sexp(&sch))?;
    let matching: Vec<_> = labels
        .iter()
        .filter(|l| l.net == net)
        .map(|l| json!({ "type": format!("{:?}", l.kind), "x": l.x, "y": l.y }))
        .collect();
    let mut g = build_net_graph(&wires, &labels, &super::sch_bridge::all_junctions(&sch));
    let pts = g.points_on_net(&net).len();
    Ok(CallToolResult::json(
        &json!({ "net": net, "label_count": matching.len(), "labels": matching, "connected_points": pts }),
    ))
}

async fn handle_get_net_connectivity(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let net = match require_str(args, "net") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let sch = cse::Schematic::load(&sch_path)?;
    let wires = super::sch_bridge::all_wires_as_sexp(&sch);
    let labels = with_power_symbol_labels(&sch_path, super::sch_bridge::all_labels_as_sexp(&sch))?;
    let mut g = build_net_graph(&wires, &labels, &super::sch_bridge::all_junctions(&sch));
    let net_pts: HashSet<(i64, i64)> = g.points_on_net(&net).into_iter().collect();
    let net_wires: Vec<_> = wires
        .iter()
        .filter(|w| net_pts.contains(&pt_key(w.x1, w.y1)) || net_pts.contains(&pt_key(w.x2, w.y2)))
        .map(|w| json!({ "x1": w.x1, "y1": w.y1, "x2": w.x2, "y2": w.y2 }))
        .collect();
    let net_labels: Vec<_> = labels
        .iter()
        .filter(|l| l.net == net)
        .map(|l| json!({ "type": format!("{:?}", l.kind), "x": l.x, "y": l.y }))
        .collect();
    let net_wire_objs: Vec<Wire> = wires
        .iter()
        .filter(|w| net_pts.contains(&pt_key(w.x1, w.y1)) || net_pts.contains(&pt_key(w.x2, w.y2)))
        .cloned()
        .collect();
    let t_junctions = konnect_sexp::schematic::find_t_junctions(&net_wire_objs, 0.01);
    Ok(CallToolResult::json(&json!({
        "net": net,
        "wires": net_wires,
        "labels": net_labels,
        "t_junctions": t_junctions.iter().map(|(x,y)| json!({"x": x, "y": y})).collect::<Vec<_>>()
    })))
}

async fn handle_get_pin_connections(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let pin_number = match require_str(args, "pin_number") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let inst = instances
        .iter()
        .find(|i| i.reference == reference)
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", reference))?;
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let lib_sym = find_lib_symbol(&lib_syms, inst);
    let pin_ep = lib_sym.and_then(|sym| {
        konnect_sexp::schematic::extract_lib_pins_for_unit(sym, inst.unit)
            .iter()
            .find(|p| p.number == pin_number)
            .map(|p| konnect_sexp::schematic::pin_endpoint(p, inst.pin_transform()))
    });
    let (px, py) = match pin_ep {
        Some(ep) => ep,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::NotFound {
                    document: sch_path.display().to_string(),
                    item_kind: "pin".to_string(),
                    key: format!("{reference}:{pin_number}"),
                    candidates: Vec::new(),
                },
                format!("Pin '{}' not found on '{}'", pin_number, reference),
            ))
        }
    };
    let mut g = build_net_graph(&wires, &labels, &extract_junction_points(&tree));
    Ok(CallToolResult::json(
        &json!({ "reference": reference, "pin": pin_number, "pin_x": px, "pin_y": py, "net": g.net_at(px, py) }),
    ))
}

async fn handle_get_component_nets(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let inst = instances
        .iter()
        .find(|i| i.reference == reference)
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", reference))?;
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let lib_sym = find_lib_symbol(&lib_syms, inst);
    let mut g = build_net_graph(&wires, &labels, &extract_junction_points(&tree));
    let pins: Vec<serde_json::Value> = if let Some(sym) = lib_sym {
        let t = inst.pin_transform();
        konnect_sexp::schematic::extract_lib_pins_for_unit(sym, inst.unit).iter().map(|p| {
            let (px, py) = konnect_sexp::schematic::pin_endpoint(p, t);
            json!({ "pin": p.number, "name": p.name, "x": px, "y": py, "net": g.net_at(px, py) })
        }).collect()
    } else {
        Vec::new()
    };
    Ok(CallToolResult::json(
        &json!({ "reference": reference, "pins": pins }),
    ))
}

async fn handle_get_net_components(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let net = match require_str(args, "net") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };
    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();
    let mut g = build_net_graph(&wires, &labels, &extract_junction_points(&tree));
    let net_pts: HashSet<(i64, i64)> = g.points_on_net(&net).into_iter().collect();
    let result: Vec<serde_json::Value> = instances
        .iter()
        .filter_map(|inst| {
            let ls = find_lib_symbol(&lib_syms, inst)?;
            let t = inst.pin_transform();
            let connected: Vec<_> =
                konnect_sexp::schematic::extract_lib_pins_for_unit(ls, inst.unit)
                    .iter()
                    .filter_map(|p| {
                        let (px, py) = konnect_sexp::schematic::pin_endpoint(p, t);
                        if net_pts.contains(&pt_key(px, py)) {
                            Some(json!({ "pin": p.number, "name": p.name }))
                        } else {
                            None
                        }
                    })
                    .collect();
            if connected.is_empty() {
                None
            } else {
                Some(json!({ "reference": inst.reference, "value": inst.value, "pins": connected }))
            }
        })
        .collect();
    Ok(CallToolResult::json(
        &json!({ "net": net, "components": result }),
    ))
}

async fn handle_trace_from_point(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let x = match require_f64(args, "x") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let y = match require_f64(args, "y") {
        Ok(v) => v,
        Err(e) => return Ok(e),
    };
    let tol = opt_f64(args, "tolerance").unwrap_or(0.05);
    let sch = cse::Schematic::load(&sch_path)?;
    let wires = super::sch_bridge::all_wires_as_sexp(&sch);
    let labels = with_power_symbol_labels(&sch_path, super::sch_bridge::all_labels_as_sexp(&sch))?;
    let mut g = build_net_graph(&wires, &labels, &super::sch_bridge::all_junctions(&sch));
    let on_wire: Vec<_> = wires
        .iter()
        .filter(|w| {
            points_coincident(x, y, w.x1, w.y1, tol)
                || points_coincident(x, y, w.x2, w.y2, tol)
                || point_on_segment(x, y, w.x1, w.y1, w.x2, w.y2, tol)
        })
        .map(|w| json!({ "x1": w.x1, "y1": w.y1, "x2": w.x2, "y2": w.y2 }))
        .collect();
    let at_label: Vec<_> = labels
        .iter()
        .filter(|l| points_coincident(x, y, l.x, l.y, tol))
        .map(|l| json!({ "net": l.net, "type": format!("{:?}", l.kind) }))
        .collect();
    Ok(CallToolResult::json(
        &json!({ "x": x, "y": y, "net": g.net_at(x, y), "wires_here": on_wire, "labels_here": at_label }),
    ))
}

async fn handle_find_orphan_items(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let (_, tree) = read_schematic(&sch_path)?;
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let junctions = extract_junction_points(&tree);

    let label_pts: HashSet<(i64, i64)> = labels.iter().map(|l| pt_key(l.x, l.y)).collect();
    let junction_pts: HashSet<(i64, i64)> = junctions.iter().map(|&(x, y)| pt_key(x, y)).collect();
    let pin_pts: HashSet<(i64, i64)> = placed_pins(&tree)
        .iter()
        .map(|p| pt_key(p.x, p.y))
        .collect();

    let mut endpoint_counts: HashMap<(i64, i64), usize> = HashMap::new();
    for w in &wires {
        *endpoint_counts.entry(pt_key(w.x1, w.y1)).or_insert(0) += 1;
        *endpoint_counts.entry(pt_key(w.x2, w.y2)).or_insert(0) += 1;
    }

    // A wire end is an orphan only if nothing meets it: no second wire end, no
    // junction, no label, and no pin. The pin was the missing one — a wire
    // drawn to a component landed here as `dangling_wire_end` while KiCAD
    // reported that same pin as connected (measured, see `attached_at`).
    // Lying on another wire's body is deliberately not a rescue: KiCAD calls
    // that `unconnected_wire_endpoint` unless a junction is on it, and the
    // junction is already checked.
    let mut all: Vec<serde_json::Value> = endpoint_counts
        .iter()
        .filter(|(k, &count)| {
            count == 1
                && !label_pts.contains(*k)
                && !junction_pts.contains(*k)
                && !pin_pts.contains(*k)
        })
        .map(|(k, _)| {
            json!({
                "type": "dangling_wire_end",
                "x": k.0 as f64 / 1000.0,
                "y": k.1 as f64 / 1000.0
            })
        })
        .collect();

    // A label attaches anywhere it touches a wire, not only at an end, so the
    // endpoint table alone reported a mid-wire label as floating.
    all.extend(
        labels
            .iter()
            .filter(|l| !on_a_wire(l.x, l.y, &wires))
            .filter(|l| !pin_pts.contains(&pt_key(l.x, l.y)))
            .map(|l| json!({ "type": "floating_label", "net": l.net, "x": l.x, "y": l.y })),
    );

    // The half the tool's description always promised and never delivered: a
    // pin with nothing on it was never reported at all (#271). Same finder
    // `find_single_pin_nets` uses, so the two cannot disagree about what an
    // unconnected pin is.
    all.extend(find_isolated_pins(&tree));

    Ok(CallToolResult::json(
        &json!({ "orphan_count": all.len(), "orphans": all }),
    ))
}

async fn handle_find_shorted_nets(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let sch = cse::Schematic::load(&sch_path)?;
    let wires = super::sch_bridge::all_wires_as_sexp(&sch);
    let labels = with_power_symbol_labels(&sch_path, super::sch_bridge::all_labels_as_sexp(&sch))?;
    let mut g = build_net_graph(&wires, &labels, &super::sch_bridge::all_junctions(&sch));
    let mut root_nets: HashMap<(i64, i64), Vec<String>> = HashMap::new();
    for l in &labels {
        let root = g.find(pt_key(l.x, l.y));
        root_nets.entry(root).or_default().push(l.net.clone());
    }
    let shorts: Vec<serde_json::Value> = root_nets
        .into_values()
        .filter_map(|mut nets| {
            nets.sort();
            nets.dedup();
            if nets.len() > 1 {
                Some(json!({ "shorted_nets": nets }))
            } else {
                None
            }
        })
        .collect();
    Ok(CallToolResult::json(
        &json!({ "short_count": shorts.len(), "shorts": shorts }),
    ))
}

async fn handle_find_single_pin_nets(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let (_, tree) = read_schematic(&sch_path)?;
    let singles = find_isolated_pins(&tree);
    Ok(CallToolResult::json(
        &json!({ "single_pin_net_count": singles.len(), "nets": singles }),
    ))
}

// `PlacedPin` and `placed_pins` live in `crate::tools` (D136): this was the
// second copy of "for each instance, for each pin of its unit, compute the
// point", next to `all_pin_endpoints` in `tools/mod.rs`. Re-exported here so
// call sites in this file did not have to change.
pub(crate) use crate::tools::placed_pins;

/// Whether anything electrical meets `(x, y)`.
///
/// Measured against KiCAD 10.0.3 on a two-resistor sheet, and it is narrower
/// than it looks:
/// * a wire **end** on a pin connects — ERC leaves that pin out of
///   `pin_not_connected` while reporting the sheet's three other pins;
/// * a wire end on another wire's **body** does **not** connect: ERC reports
///   `unconnected_wire_endpoint` at exactly that point, and adding the
///   junction makes it go away. So touching a body is only a connection for
///   something that is not itself a wire end — a pin or a label sitting on a
///   wire.
///
/// What is deliberately not modelled: ERC's own rule names. `wire_dangling`
/// fires on this fixture in cases these three facts do not explain, and
/// guessing at it would put an unmeasured claim in the answer. This tool
/// reports geometric attachment, which is what "orphan" means here.
fn attached_at(
    x: f64,
    y: f64,
    graph: &mut NetGraph,
    wires: &[konnect_sexp::schematic::Wire],
) -> bool {
    graph.net_at(x, y).is_some() || on_a_wire(x, y, wires)
}

/// Whether a wire passes through `(x, y)` — its ends included, since an end
/// lies on its own segment.
///
/// The geometric half of [`attached_at`], and the whole test for a label: the
/// net graph carries every label as a node of its own, so asking it whether a
/// label is attached always answers yes. A label is attached when it touches a
/// wire (anywhere along it) or a pin, and nothing else.
fn on_a_wire(x: f64, y: f64, wires: &[konnect_sexp::schematic::Wire]) -> bool {
    wires
        .iter()
        .any(|wire| point_on_segment(x, y, wire.x1, wire.y1, wire.x2, wire.y2, 0.01))
}

fn find_isolated_pins(tree: &konnect_sexp::parser::SexpNode) -> Vec<serde_json::Value> {
    let wires = extract_wires(tree);
    let labels = extract_all_net_labels(tree);
    let no_connects: HashSet<(i64, i64)> = tree
        .find_all("no_connect")
        .iter()
        .filter_map(|node| parse_at(node).map(|(x, y, _)| pt_key(x, y)))
        .collect();
    let mut graph = build_net_graph(&wires, &labels, &extract_junction_points(tree));

    placed_pins(tree)
        .into_iter()
        .filter(|pin| !no_connects.contains(&pt_key(pin.x, pin.y)))
        .filter(|pin| !attached_at(pin.x, pin.y, &mut graph, &wires))
        .map(|pin| {
            json!({
                "net": null,
                "reference": pin.reference,
                "pin_number": pin.number,
                "pin_name": pin.name,
                "x": pin.x,
                "y": pin.y,
                "type": "unconnected_pin"
            })
        })
        .collect()
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::*;
    use crate::{mcp::protocol::ToolContent, router::ToolRouter};
    use konnect_sexp::parser::parse_sexp;
    use std::{process::Command, sync::Arc};

    fn ctx() -> ToolContext {
        ToolContext::new(
            crate::tools::ServerConfig {
                kicad_cli: "kicad-cli".to_string(),
                kicad_binary: "kicad".to_string(),
                ipc_address: String::new(),
                project_dir: None,
                jlcpcb_db_path: None,
                auto_load_toolsets: false,
                mode: kam_state::OperatingMode::Write,
            },
            Arc::new(ToolRouter::new()),
        )
    }

    const THREE_ISOLATED_RESISTORS: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/three_isolated_resistors.kicad_sch"
    );

    /// `power:GND` on R2 pin 2, `power:+3V3` on R1 pin 1, each backed by a
    /// `PWR_FLAG`, plus a plain `(label "VOUT" ...)` at the divider midpoint.
    /// Copied from `bench/fixtures/divider.kicad_sch`, verified ERC-clean
    /// (`kicad-cli sch erc`: 0 violations) before use (#262, 6d394a4).
    const POWER_SYMBOL_DIVIDER: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/power_symbol_divider.kicad_sch"
    );

    /// Two `U1` `SymbolInstance`s (one per unit) over the real
    /// `Amplifier_Operational:LM2904` definition. Unit 1 declares pins 1-3,
    /// unit 2 declares pins 5-7 — and unit 1's pin 3 sits at the same local
    /// point as unit 2's pin 5, the coordinate collision P.6.8.1 exploits.
    const MULTIUNIT_LM2904: &str = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/multiunit_lm2904.kicad_sch"
    );

    async fn tool_json(
        handler: impl std::future::Future<Output = anyhow::Result<CallToolResult>>,
    ) -> serde_json::Value {
        let result = handler.await.unwrap();
        let ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text result");
        };
        serde_json::from_str(text).unwrap()
    }

    /// Red before the backport: `list_schematic_nets` only reads
    /// `extract_labels`, so a rail named solely by a `power:GND` / `power:+3V3`
    /// symbol's `Value` property never appears — only the plain "VOUT" label
    /// does, and the count is 1 instead of 3.
    #[tokio::test]
    async fn list_nets_sees_power_symbol_rails() {
        let out = tool_json(handle_list_nets(
            &json!({ "schematic": POWER_SYMBOL_DIVIDER }),
            &ctx(),
        ))
        .await;
        let nets: Vec<&str> = out["nets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| n.as_str().unwrap())
            .collect();
        assert!(nets.contains(&"GND"), "GND rail missing from {nets:?}");
        assert!(nets.contains(&"+3V3"), "+3V3 rail missing from {nets:?}");
        assert!(nets.contains(&"VOUT"), "plain label must still be seen");
        // The fixture's two PWR_FLAGs carry a `power_out` pin: they assert a
        // source exists on the rail for ERC's sake, they do not name it. A
        // net called "PWR_FLAG" would mean the `power_in` filter is gone and
        // every flagged rail has been renamed after its flag.
        assert!(
            !nets.contains(&"PWR_FLAG"),
            "PWR_FLAG names no net; it must not appear in {nets:?}"
        );
    }

    /// Red before the backport: `get_net_connections` builds its graph from
    /// `extract_labels` alone, so the "GND" net — named only by the placed
    /// `power:GND` symbol's `Value` — has zero connected points even though
    /// it is wired straight to R2 pin 2.
    #[tokio::test]
    async fn get_net_connections_sees_the_power_symbol_pin() {
        let out = tool_json(handle_get_net_connections(
            &json!({ "schematic": POWER_SYMBOL_DIVIDER, "net": "GND" }),
            &ctx(),
        ))
        .await;
        assert!(
            out["connected_points"].as_u64().unwrap() > 0,
            "GND must resolve to at least the power symbol's own point, got {out}"
        );
    }

    /// P.6.8.1: `get_component_nets` used the unit-blind `extract_lib_pins`,
    /// which superimposes every unit's pins onto whichever instance `find()`
    /// picked first. The first `U1` instance in the fixture is unit 1 (pins
    /// 1-3); reporting pin 5, 6, or 7 for it would mean unit 2's pins leaked
    /// through.
    #[tokio::test]
    async fn get_component_nets_does_not_leak_the_other_units_pins() {
        let out = tool_json(handle_get_component_nets(
            &json!({ "schematic": MULTIUNIT_LM2904, "reference": "U1" }),
            &ctx(),
        ))
        .await;
        let pins: Vec<&str> = out["pins"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["pin"].as_str().unwrap())
            .collect();
        assert_eq!(
            pins.len(),
            3,
            "unit 1 of LM2904 declares exactly 3 pins (1,2,3), got {pins:?}"
        );
        for leaked in ["5", "6", "7"] {
            assert!(
                !pins.contains(&leaked),
                "unit 1 must not report unit 2's pin '{leaked}': {pins:?}"
            );
        }
        for own in ["1", "2", "3"] {
            assert!(
                pins.contains(&own),
                "unit 1 is missing its own pin '{own}': {pins:?}"
            );
        }
    }

    async fn in_process_single_pin_nets() -> usize {
        let result =
            handle_find_single_pin_nets(&json!({ "schematic": THREE_ISOLATED_RESISTORS }), &ctx())
                .await
                .unwrap();
        let ToolContent::Text { text } = &result.content[0] else {
            panic!("expected text result");
        };
        serde_json::from_str::<serde_json::Value>(text).unwrap()["single_pin_net_count"]
            .as_u64()
            .unwrap() as usize
    }

    #[tokio::test]
    async fn finds_six_isolated_pins_for_three_isolated_resistors() {
        assert_eq!(in_process_single_pin_nets().await, 6);
    }

    const RESISTOR_LIBRARY: &str = r#"
        (lib_symbols
          (symbol "Device:R"
            (symbol "R_1_1"
              (pin passive line (at 0 3.81 270) (length 1.27) (name "~") (number "1"))
              (pin passive line (at 0 -3.81 90) (length 1.27) (name "~") (number "2")))))
    "#;

    #[test]
    fn excludes_explicit_no_connect_pins() {
        let tree = parse_sexp(&format!(
            r#"(kicad_sch {RESISTOR_LIBRARY}
              (symbol (lib_id "Device:R") (at 100 50 0) (unit 1)
                (property "Reference" "R1") (property "Value" "10k"))
              (no_connect (at 100 53.81)))"#
        ))
        .unwrap();
        let pins = find_isolated_pins(&tree);

        assert_eq!(pins.len(), 1);
        assert_eq!(pins[0]["pin_number"], "1");
    }

    #[test]
    fn recognizes_pins_connected_by_a_wire() {
        let tree = parse_sexp(&format!(
            r#"(kicad_sch {RESISTOR_LIBRARY}
              (symbol (lib_id "Device:R") (at 100 50 0) (unit 1)
                (property "Reference" "R1") (property "Value" "10k"))
              (symbol (lib_id "Device:R") (at 110 50 0) (unit 1)
                (property "Reference" "R2") (property "Value" "10k"))
              (wire (pts (xy 100 53.81) (xy 110 53.81))))"#
        ))
        .unwrap();
        let pins = find_isolated_pins(&tree);

        assert_eq!(pins.len(), 2);
        assert!(pins.iter().all(|pin| pin["pin_number"] == "1"));
    }

    #[tokio::test]
    #[ignore = "requires KICAD_CLI to reproduce the external ERC divergence"]
    async fn reproduces_kicad_cli_erc_divergence_for_isolated_resistors() {
        let kicad_cli = std::env::var("KICAD_CLI")
            .expect("KICAD_CLI must name the kicad-cli executable for this probe");
        let output_dir = tempfile::tempdir().unwrap();
        let output = output_dir.path().join("erc.json");
        let cli = Command::new(kicad_cli)
            .args(["sch", "erc", "--format", "json", "--output"])
            .arg(&output)
            .arg(THREE_ISOLATED_RESISTORS)
            .output()
            .expect("KICAD_CLI could not be executed");
        assert!(
            cli.status.success(),
            "kicad-cli sch erc failed: {}",
            String::from_utf8_lossy(&cli.stderr)
        );
        let erc: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        let cli_unconnected_pins = erc["sheets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|sheet| sheet["violations"].as_array().unwrap())
            .filter(|violation| violation["type"] == "pin_not_connected")
            .count();

        assert_eq!(in_process_single_pin_nets().await, 6);
        assert_eq!(cli_unconnected_pins, 6);
    }
}

async fn handle_get_connected_items(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let reference = match require_str(args, "reference") {
        Ok(v) => v.to_string(),
        Err(e) => return Ok(e),
    };

    let (_, tree) = read_schematic(&sch_path)?;
    let instances = extract_symbol_instances(&tree);
    let wires = extract_wires(&tree);
    let labels = extract_all_net_labels(&tree);
    let lib_syms = tree
        .find("lib_symbols")
        .map(|n| n.find_all("symbol"))
        .unwrap_or_default();

    let inst = match instances.iter().find(|i| i.reference == reference) {
        Some(i) => i,
        None => {
            return Ok(CallToolResult::error_kind(
                ToolErrorKind::NotFound {
                    document: sch_path.display().to_string(),
                    item_kind: "component".to_string(),
                    key: reference.to_string(),
                    candidates: Vec::new(),
                },
                format!("Component '{}' not found", reference),
            ))
        }
    };

    let lib_sym = find_lib_symbol(&lib_syms, inst);
    let mut g = build_net_graph(&wires, &labels, &extract_junction_points(&tree));

    // Get nets for each pin
    let mut connected_nets: HashSet<String> = HashSet::new();
    if let Some(sym) = lib_sym {
        let t = inst.pin_transform();
        for p in konnect_sexp::schematic::extract_lib_pins_for_unit(sym, inst.unit) {
            let (px, py) = konnect_sexp::schematic::pin_endpoint(&p, t);
            if let Some(net) = g.net_at(px, py) {
                connected_nets.insert(net);
            }
        }
    }

    // Find all wires, labels, and components on those nets
    let mut all_net_pts: HashSet<(i64, i64)> = HashSet::new();
    for net in &connected_nets {
        for pt in g.points_on_net(net) {
            all_net_pts.insert(pt);
        }
    }

    let connected_wires: Vec<serde_json::Value> = wires
        .iter()
        .filter(|w| {
            all_net_pts.contains(&pt_key(w.x1, w.y1)) || all_net_pts.contains(&pt_key(w.x2, w.y2))
        })
        .map(|w| json!({ "x1": w.x1, "y1": w.y1, "x2": w.x2, "y2": w.y2, "uuid": w.uuid }))
        .collect();

    let connected_labels: Vec<serde_json::Value> = labels
        .iter()
        .filter(|l| connected_nets.contains(&l.net))
        .map(|l| json!({ "net": l.net, "type": format!("{:?}", l.kind), "x": l.x, "y": l.y }))
        .collect();

    // Find other components on the same nets (excluding the queried one)
    let connected_components: Vec<serde_json::Value> = instances.iter()
        .filter(|i| i.reference != reference)
        .filter_map(|i| {
            let ls = find_lib_symbol(&lib_syms, i)?;
            let t = i.pin_transform();
            let matching_pins: Vec<_> = konnect_sexp::schematic::extract_lib_pins_for_unit(ls, i.unit).iter()
                .filter_map(|p| {
                    let (px, py) = konnect_sexp::schematic::pin_endpoint(p, t);
                    if all_net_pts.contains(&pt_key(px, py)) {
                        Some(json!({ "pin": p.number, "name": p.name }))
                    } else { None }
                }).collect();
            if matching_pins.is_empty() { None }
            else { Some(json!({ "reference": i.reference, "value": i.value, "connected_pins": matching_pins })) }
        })
        .collect();

    Ok(CallToolResult::json(&json!({
        "reference": reference,
        "nets": connected_nets.iter().collect::<Vec<_>>(),
        "connected_wires": connected_wires.len(),
        "wires": connected_wires,
        "labels": connected_labels,
        "connected_components": connected_components
    })))
}

async fn handle_check_overlaps(
    args: &serde_json::Value,
    _ctx: &ToolContext,
) -> anyhow::Result<CallToolResult> {
    let sch_path = get_path(args, "schematic")?;
    let tol = opt_f64(args, "tolerance").unwrap_or(0.5);
    let sch = cse::Schematic::load(&sch_path)?;

    // Component overlap detection using the new crate's spatial query
    let symbols: Vec<&cse::Symbol> = sch.symbols.iter().collect();
    let mut comp_overlaps: Vec<serde_json::Value> = Vec::new();
    for (i, a) in symbols.iter().enumerate() {
        let (ax, ay) = a.position();
        for b in &symbols[i + 1..] {
            let (bx, by) = b.position();
            if points_coincident(ax, ay, bx, by, tol) {
                comp_overlaps.push(json!({
                    "type": "component_overlap",
                    "a": a.reference().unwrap_or("?"),
                    "b": b.reference().unwrap_or("?"),
                    "x": ax, "y": ay
                }));
            }
        }
    }

    // Label overlap detection — collect all label types into a uniform list
    struct LabelInfo {
        net: String,
        x: f64,
        y: f64,
    }
    let mut all_labels: Vec<LabelInfo> = Vec::new();
    for l in sch.labels.iter() {
        all_labels.push(LabelInfo {
            net: l.text.clone(),
            x: l.at.x,
            y: l.at.y,
        });
    }
    for g in sch.global_labels.iter() {
        all_labels.push(LabelInfo {
            net: g.text.clone(),
            x: g.at.x,
            y: g.at.y,
        });
    }
    for h in sch.hierarchical_labels.iter() {
        all_labels.push(LabelInfo {
            net: h.text.clone(),
            x: h.at.x,
            y: h.at.y,
        });
    }
    let mut label_overlaps: Vec<serde_json::Value> = Vec::new();
    for (i, a) in all_labels.iter().enumerate() {
        for b in &all_labels[i + 1..] {
            if points_coincident(a.x, a.y, b.x, b.y, tol) && a.net != b.net {
                label_overlaps.push(json!({ "type": "label_overlap", "net_a": a.net, "net_b": b.net, "x": a.x, "y": a.y }));
            }
        }
    }

    let mut all = comp_overlaps;
    all.extend(label_overlaps);
    Ok(CallToolResult::json(
        &json!({ "overlap_count": all.len(), "overlaps": all }),
    ))
}
