//! Buses: bus segments, bus entries, bus aliases, and member expansion.
//!
//! Until J.2.2.2 the engine handled wires and labels only, and `bus` nodes were
//! carried through `raw_other` untouched — safe for a round-trip, and no way to
//! author a bus-based design. These three types model what a `.kicad_sch`
//! actually holds:
//!
//! * [`Bus`] — a bus segment. Geometrically a [`Wire`](super::wire::Wire); a
//!   separate type because KiCAD's connectivity treats the two differently and
//!   a caller must not be able to confuse them.
//! * [`BusEntry`] — the short diagonal stub that taps one member out of a bus.
//! * [`BusAlias`] — a name standing for a list of members, declared once at the
//!   top of the sheet.
//!
//! [`expand_members`] is the reader for KiCAD's bus-name syntax, which is what
//! makes a bus more than a thick wire. Its answers are checked against KiCAD
//! itself in `crates/konnect-core/tests/bus_live.rs`: a netlist exported from a
//! schematic this engine wrote names the members KiCAD derived, so the
//! expansion here is measured rather than believed.

use crate::error::{Error, Result};
use crate::sexp::{atom, qstr, tagged, SexpNode};
use crate::types::{fmt_f64, Stroke};

// ---- Bus --------------------------------------------------------------------

/// A bus segment. Same geometry as a wire, deliberately not the same type.
#[derive(Debug, Clone)]
pub struct Bus {
    pub start: (f64, f64),
    pub end: (f64, f64),
    pub uuid: String,
    pub stroke: Option<Stroke>,
}

impl Bus {
    pub fn new(x1: f64, y1: f64, x2: f64, y2: f64) -> Self {
        Bus {
            start: (x1, y1),
            end: (x2, y2),
            uuid: uuid::Uuid::new_v4().to_string(),
            stroke: None,
        }
    }

    pub fn from_sexp(node: &SexpNode) -> Result<Self> {
        let pts = node.find("pts").ok_or(Error::MissingField("pts"))?;
        let xys: Vec<&SexpNode> = pts.find_all("xy");

        let parse_xy = |n: &SexpNode| -> Option<(f64, f64)> {
            let s = n.scalar_args();
            let x = s.first()?.parse().ok()?;
            let y = s.get(1)?.parse().ok()?;
            Some((x, y))
        };

        let start = xys.first().and_then(|n| parse_xy(n)).unwrap_or((0.0, 0.0));
        let end = xys.get(1).and_then(|n| parse_xy(n)).unwrap_or((0.0, 0.0));

        Ok(Bus {
            start,
            end,
            uuid: node.get_value("uuid").unwrap_or("").to_owned(),
            stroke: node.find("stroke").and_then(Stroke::from_sexp),
        })
    }

    pub fn to_sexp(&self) -> SexpNode {
        let (x1, y1) = self.start;
        let (x2, y2) = self.end;
        let pts = tagged(
            "pts",
            vec![
                tagged("xy", vec![atom(fmt_f64(x1)), atom(fmt_f64(y1))]),
                tagged("xy", vec![atom(fmt_f64(x2)), atom(fmt_f64(y2))]),
            ],
        );
        let mut c = vec![atom("bus"), pts];
        if let Some(s) = &self.stroke {
            c.push(s.to_sexp());
        }
        c.push(tagged("uuid", vec![qstr(self.uuid.clone())]));
        SexpNode::List(c)
    }

    pub fn is_horizontal(&self) -> bool {
        (self.start.1 - self.end.1).abs() < 1e-9
    }

    pub fn is_vertical(&self) -> bool {
        (self.start.0 - self.end.0).abs() < 1e-9
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.start.0 += dx;
        self.start.1 += dy;
        self.end.0 += dx;
        self.end.1 += dy;
    }

    pub fn touches(&self, x: f64, y: f64) -> bool {
        let eq = |p: (f64, f64)| (p.0 - x).abs() < 1e-9 && (p.1 - y).abs() < 1e-9;
        eq(self.start) || eq(self.end)
    }
}

// ---- BusEntry ---------------------------------------------------------------

/// The stub connecting one wire to a bus. `size` is a delta, not a corner, and
/// may be negative on either axis — that is how the entry picks its diagonal.
#[derive(Debug, Clone)]
pub struct BusEntry {
    pub x: f64,
    pub y: f64,
    pub size: (f64, f64),
    pub uuid: String,
    pub stroke: Option<Stroke>,
}

/// KiCAD's default bus-entry stub: one grid unit each way.
pub const DEFAULT_BUS_ENTRY_SIZE: (f64, f64) = (2.54, 2.54);

impl BusEntry {
    pub fn new(x: f64, y: f64) -> Self {
        BusEntry {
            x,
            y,
            size: DEFAULT_BUS_ENTRY_SIZE,
            uuid: uuid::Uuid::new_v4().to_string(),
            stroke: None,
        }
    }

    pub fn with_size(x: f64, y: f64, dx: f64, dy: f64) -> Self {
        BusEntry {
            size: (dx, dy),
            ..BusEntry::new(x, y)
        }
    }

    pub fn from_sexp(node: &SexpNode) -> Result<Self> {
        let at = node.find("at").ok_or(Error::MissingField("at"))?;
        let a = at.scalar_args();
        let x: f64 = a.first().and_then(|v| v.parse().ok()).unwrap_or(0.0);
        let y: f64 = a.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0);

        let size = node
            .find("size")
            .map(|n| {
                let s = n.scalar_args();
                (
                    s.first().and_then(|v| v.parse().ok()).unwrap_or(0.0),
                    s.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0),
                )
            })
            .unwrap_or(DEFAULT_BUS_ENTRY_SIZE);

        Ok(BusEntry {
            x,
            y,
            size,
            uuid: node.get_value("uuid").unwrap_or("").to_owned(),
            stroke: node.find("stroke").and_then(Stroke::from_sexp),
        })
    }

    pub fn to_sexp(&self) -> SexpNode {
        let mut c = vec![
            atom("bus_entry"),
            tagged("at", vec![atom(fmt_f64(self.x)), atom(fmt_f64(self.y))]),
            tagged(
                "size",
                vec![atom(fmt_f64(self.size.0)), atom(fmt_f64(self.size.1))],
            ),
        ];
        if let Some(s) = &self.stroke {
            c.push(s.to_sexp());
        }
        c.push(tagged("uuid", vec![qstr(self.uuid.clone())]));
        SexpNode::List(c)
    }

    /// Where the stub ends — the point a wire has to meet.
    pub fn end(&self) -> (f64, f64) {
        (self.x + self.size.0, self.y + self.size.1)
    }

    pub fn translate(&mut self, dx: f64, dy: f64) {
        self.x += dx;
        self.y += dy;
    }
}

// ---- BusAlias ---------------------------------------------------------------

/// `(bus_alias "NAME" (members "A" "B"))` — a name that stands for a list.
#[derive(Debug, Clone)]
pub struct BusAlias {
    pub name: String,
    pub members: Vec<String>,
}

impl BusAlias {
    pub fn new(name: impl Into<String>, members: Vec<String>) -> Self {
        BusAlias {
            name: name.into(),
            members,
        }
    }

    pub fn from_sexp(node: &SexpNode) -> Result<Self> {
        let name = node
            .args()
            .first()
            .and_then(|n| n.text())
            .ok_or(Error::MissingField("bus_alias name"))?
            .to_owned();
        let members = node
            .find("members")
            .map(|n| {
                n.args()
                    .iter()
                    .filter_map(|m| m.text().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        Ok(BusAlias { name, members })
    }

    pub fn to_sexp(&self) -> SexpNode {
        let members = SexpNode::List(
            std::iter::once(atom("members"))
                .chain(self.members.iter().map(|m| qstr(m.clone())))
                .collect(),
        );
        SexpNode::List(vec![atom("bus_alias"), qstr(self.name.clone()), members])
    }
}

// ---- Member expansion -------------------------------------------------------

/// What a bus name turns out to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BusKind {
    /// `DATA[0..7]` — a prefix and an inclusive range.
    Vector,
    /// `{A B}` or `MEM{A0 A1}` — an explicit list, optionally prefixed.
    Group,
    /// A name declared by a `bus_alias` on the sheet.
    Alias,
    /// Not a bus name at all.
    Plain,
}

/// Expand a bus name into its member nets, the way KiCAD's connectivity does.
///
/// Three syntaxes, and they are KiCAD's, not ours:
///
/// * **vector** — `DATA[0..7]` is `DATA0` … `DATA7`. A descending range
///   (`DATA[7..0]`) names the same members in the opposite order, which is how
///   a schematic states bit order.
/// * **group** — `{SDA SCL}` is exactly those names. With a prefix,
///   `MEM{A0 A1}` is `MEM.A0` and `MEM.A1`: KiCAD joins the two with a dot.
/// * **alias** — a name declared by a `bus_alias` expands to its members.
///   `aliases` is what the sheet declares; pass an empty slice when there are
///   none.
///
/// Anything else is [`BusKind::Plain`] and expands to itself, so a caller can
/// hand any label here and act on the answer.
pub fn expand_members(name: &str, aliases: &[BusAlias]) -> (BusKind, Vec<String>) {
    let name = name.trim();

    if let Some(members) = expand_vector(name) {
        return (BusKind::Vector, members);
    }
    if let Some(members) = expand_group(name) {
        return (BusKind::Group, members);
    }
    if let Some(alias) = aliases.iter().find(|alias| alias.name == name) {
        return (BusKind::Alias, alias.members.clone());
    }
    (BusKind::Plain, vec![name.to_string()])
}

/// `PREFIX[M..N]` → `PREFIXM` … `PREFIXN`, in the order written.
fn expand_vector(name: &str) -> Option<Vec<String>> {
    let open = name.find('[')?;
    if !name.ends_with(']') {
        return None;
    }
    let prefix = &name[..open];
    if prefix.is_empty() {
        return None;
    }
    let range = &name[open + 1..name.len() - 1];
    let (first, last) = range.split_once("..")?;
    let first: i64 = first.trim().parse().ok()?;
    let last: i64 = last.trim().parse().ok()?;

    let indices: Vec<i64> = if first <= last {
        (first..=last).collect()
    } else {
        (last..=first).rev().collect()
    };
    Some(
        indices
            .into_iter()
            .map(|index| format!("{prefix}{index}"))
            .collect(),
    )
}

/// `{A B}` → `A`, `B`; `PREFIX{A B}` → `PREFIX.A`, `PREFIX.B`.
fn expand_group(name: &str) -> Option<Vec<String>> {
    let open = name.find('{')?;
    if !name.ends_with('}') {
        return None;
    }
    let prefix = &name[..open];
    let inner = &name[open + 1..name.len() - 1];

    let members: Vec<String> = inner
        .split_whitespace()
        .map(|member| {
            if prefix.is_empty() {
                member.to_string()
            } else {
                format!("{prefix}.{member}")
            }
        })
        .collect();
    (!members.is_empty()).then_some(members)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_vector_expands_in_the_order_it_is_written() {
        let (kind, members) = expand_members("DATA[0..3]", &[]);
        assert_eq!(kind, BusKind::Vector);
        assert_eq!(members, ["DATA0", "DATA1", "DATA2", "DATA3"]);

        // Bit order is information, so a descending range keeps its order.
        let (_, descending) = expand_members("DATA[3..0]", &[]);
        assert_eq!(descending, ["DATA3", "DATA2", "DATA1", "DATA0"]);
    }

    #[test]
    fn a_single_bit_vector_is_still_a_vector() {
        let (kind, members) = expand_members("A[2..2]", &[]);
        assert_eq!(kind, BusKind::Vector);
        assert_eq!(members, ["A2"]);
    }

    #[test]
    fn a_group_takes_its_members_verbatim_and_a_prefix_joins_with_a_dot() {
        let (kind, members) = expand_members("{SDA SCL}", &[]);
        assert_eq!(kind, BusKind::Group);
        assert_eq!(members, ["SDA", "SCL"]);

        let (_, prefixed) = expand_members("MEM{A0 A1}", &[]);
        assert_eq!(prefixed, ["MEM.A0", "MEM.A1"]);
    }

    #[test]
    fn an_alias_expands_to_what_the_sheet_declared() {
        let aliases = [BusAlias::new(
            "USB",
            vec!["USB_D_P".to_string(), "USB_D_N".to_string()],
        )];
        let (kind, members) = expand_members("USB", &aliases);
        assert_eq!(kind, BusKind::Alias);
        assert_eq!(members, ["USB_D_P", "USB_D_N"]);
    }

    /// A plain net is not an error: the caller hands over any label and reads
    /// the kind.
    #[test]
    fn a_plain_name_expands_to_itself() {
        let (kind, members) = expand_members("VCC", &[]);
        assert_eq!(kind, BusKind::Plain);
        assert_eq!(members, ["VCC"]);
    }

    /// Malformed bus syntax is reported as plain rather than guessed at — a
    /// half-expanded bus would silently create nets nobody asked for.
    #[test]
    fn malformed_bus_syntax_is_not_guessed_at() {
        for name in ["DATA[0..]", "DATA[]", "[0..3]", "DATA[a..b]", "DATA[0-3]"] {
            let (kind, members) = expand_members(name, &[]);
            assert_eq!(kind, BusKind::Plain, "'{name}' should not expand");
            assert_eq!(members, [name]);
        }
    }

    #[test]
    fn a_bus_round_trips_through_sexp() {
        let bus = Bus::new(10.0, 20.0, 40.0, 20.0);
        let parsed = Bus::from_sexp(&bus.to_sexp()).expect("a written bus parses");
        assert_eq!(parsed.start, (10.0, 20.0));
        assert_eq!(parsed.end, (40.0, 20.0));
        assert_eq!(parsed.uuid, bus.uuid);
        assert!(parsed.is_horizontal());
    }

    #[test]
    fn a_bus_entry_round_trips_and_keeps_a_negative_size() {
        let entry = BusEntry::with_size(50.8, 25.4, -2.54, 2.54);
        let parsed = BusEntry::from_sexp(&entry.to_sexp()).expect("a written entry parses");
        assert_eq!((parsed.x, parsed.y), (50.8, 25.4));
        assert_eq!(parsed.size, (-2.54, 2.54));
        let (end_x, end_y) = parsed.end();
        assert!((end_x - 48.26).abs() < 1e-9 && (end_y - 27.94).abs() < 1e-9);
    }

    #[test]
    fn a_bus_alias_round_trips_through_sexp() {
        let alias = BusAlias::new("CTRL", vec!["RD".to_string(), "WR".to_string()]);
        let parsed = BusAlias::from_sexp(&alias.to_sexp()).expect("a written alias parses");
        assert_eq!(parsed.name, "CTRL");
        assert_eq!(parsed.members, ["RD", "WR"]);
    }
}
