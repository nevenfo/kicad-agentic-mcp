use super::SexpNode;

/// Indentation unit used when serializing a document.
///
/// KiCAD 10 writes one tab per depth level; this is also our default for
/// callers that serialize a bare fragment (`write`) rather than a full file
/// (`write_styled`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Tab,
    Spaces(usize),
}

/// Serialization style for a whole document: indentation unit and line
/// ending. Sniffed from the source a `Schematic` was loaded from so a
/// round-tripped file diffs cleanly against what KiCAD itself would write,
/// instead of reformatting the whole document on every save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteStyle {
    pub indent: IndentStyle,
    /// `true` writes `\r\n`, matching every KiCAD 10 demo sheet on Windows.
    /// Writing plain `\n` into a CRLF file reproduces the "whole document in
    /// the diff" symptom this style exists to avoid, just via line endings
    /// instead of indentation — so this is tracked as its own axis, not
    /// folded into `indent`.
    pub crlf: bool,
}

impl Default for WriteStyle {
    fn default() -> Self {
        WriteStyle {
            indent: IndentStyle::Tab,
            crlf: false,
        }
    }
}

/// Serialize `node` using the default style (tab indent, `\n`). Intended for
/// callers that serialize a standalone fragment outside the context of a
/// specific file — a full document should go through `write_styled` with the
/// style sniffed from its source so round-trips don't reformat it.
pub fn write(node: &SexpNode) -> String {
    write_styled(node, WriteStyle::default())
}

/// Serialize `node` as a full document using `style`.
pub fn write_styled(node: &SexpNode, style: WriteStyle) -> String {
    let mut buf = String::with_capacity(16384);
    write_node(node, &mut buf, 0, &style);
    buf.push_str(newline(&style));
    buf
}

fn newline(style: &WriteStyle) -> &'static str {
    if style.crlf {
        "\r\n"
    } else {
        "\n"
    }
}

fn write_node(node: &SexpNode, buf: &mut String, depth: usize, style: &WriteStyle) {
    match node {
        SexpNode::Atom(s) => buf.push_str(s),
        SexpNode::Str(s) => {
            buf.push('"');
            for c in s.chars() {
                match c {
                    '"' => buf.push_str("\\\""),
                    '\\' => buf.push_str("\\\\"),
                    '\n' => buf.push_str("\\n"),
                    '\t' => buf.push_str("\\t"),
                    '\r' => buf.push_str("\\r"),
                    c => buf.push(c),
                }
            }
            buf.push('"');
        }
        SexpNode::List(children) => {
            if children.is_empty() {
                buf.push_str("()");
                return;
            }

            let has_list_child = children.iter().skip(1).any(|c| c.is_list());

            buf.push('(');

            if depth == 0 {
                // Root: tag on same line, each child on its own indented line.
                for (i, child) in children.iter().enumerate() {
                    if i == 0 {
                        write_node(child, buf, 1, style);
                    } else {
                        buf.push_str(newline(style));
                        write_indent(buf, 1, style);
                        write_node(child, buf, 1, style);
                    }
                }
                buf.push_str(newline(style));
            } else if has_list_child {
                // Multi-line: scalars inline after tag, sub-lists on new lines,
                // closing paren alone on its own line at the parent's depth.
                //
                // KiCAD also packs several `(xy …)` per line inside a `(pts …)`
                // up to a target width; we always emit one `(xy …)` per line.
                // That's a known, deliberate residual divergence — not
                // implemented here.
                for (i, child) in children.iter().enumerate() {
                    if i == 0 {
                        write_node(child, buf, depth + 1, style);
                    } else if child.is_list() {
                        buf.push_str(newline(style));
                        write_indent(buf, depth + 1, style);
                        write_node(child, buf, depth + 1, style);
                    } else {
                        buf.push(' ');
                        write_node(child, buf, depth + 1, style);
                    }
                }
                buf.push_str(newline(style));
                write_indent(buf, depth, style);
            } else {
                // All scalars: single line.
                for (i, child) in children.iter().enumerate() {
                    if i > 0 {
                        buf.push(' ');
                    }
                    write_node(child, buf, depth + 1, style);
                }
            }

            buf.push(')');
        }
    }
}

fn write_indent(buf: &mut String, depth: usize, style: &WriteStyle) {
    match style.indent {
        IndentStyle::Tab => {
            for _ in 0..depth {
                buf.push('\t');
            }
        }
        IndentStyle::Spaces(n) => {
            for _ in 0..depth {
                for _ in 0..n {
                    buf.push(' ');
                }
            }
        }
    }
}
