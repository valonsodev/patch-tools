use std::ops::Range;

use super::super::merge_ranges;
use super::super::syntax_tree::{BuildTreeOptions, SyntaxTree, build_tree};

/// Tree-sitter node kinds skipped at parse time because their byte ranges are
/// noise in a method-level diff (jump targets and source-line directives that
/// the assembler regenerates).
const EXCLUDED_NODE_TYPES: &[&str] = &["jmp_label", "label", "line_directive"];

/// Tree-sitter node kinds treated as atomic leaves. The grammar splits class
/// type literals into per-segment children (`L`, `identifier("com")`, `/`,
/// `identifier("foo")`, …, `;`), which makes the AST diff emit per-segment
/// `[-foo-]{+bar+}` markers when a class path changes. Flattening these to
/// atoms makes a path change render as one whole-token delete + insert.
const ATOMIC_NODE_TYPES: &[&str] = &["class_identifier", "array_type"];

pub(crate) fn parse(source: &str) -> Option<SyntaxTree<'_>> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_smali::LANGUAGE.into();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(source, None)?;
    if tree.root_node().has_error() {
        return None;
    }

    Some(build_tree(
        &tree.walk(),
        source,
        // Keep nearby instruction nodes paired so narrower subtree diffs can mark only operands.
        BuildTreeOptions::default()
            .with_excluded_node_types(EXCLUDED_NODE_TYPES)
            .with_atomic_node_types(ATOMIC_NODE_TYPES)
            .with_same_kind_replacements(true),
    ))
}

/// Strip first, then promote: if promotion ran first, an opcode-change
/// full-line range would re-expose the label byte-ranges that stripping was
/// meant to remove, and they'd render as unchanged context inside an
/// otherwise-deleted line.
pub(crate) fn post_filter(source: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let stripped = filter_ignored_ranges(source, ranges);
    promote_opcode_change_lines(source, stripped)
}

/// When the diff touches a line's opcode token, expand every range on that
/// line to cover the full line content. Different opcodes have different
/// operand shapes (e.g. `iget-boolean` → `const/4`), so the otherwise-
/// matching common atoms (`v0,`) would mislead `combine_semantic_replacement`
/// into interleaving the lines inline. Same-opcode operand changes don't
/// touch the opcode token and are unaffected.
fn promote_opcode_change_lines(source: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return ranges;
    }

    let mut line_starts: Vec<(usize, Range<usize>, Option<Range<usize>>)> = Vec::new();
    let mut cursor = 0;
    for line in source.split_inclusive('\n') {
        let content_len = line.trim_end_matches(['\r', '\n']).len();
        let content_range = cursor..(cursor + content_len);
        let opcode_range = opcode_byte_range(&line[..content_len], cursor);
        line_starts.push((cursor, content_range, opcode_range));
        cursor += line.len();
    }

    let mut promoted_lines: Vec<bool> = vec![false; line_starts.len()];
    let mut any_promoted = false;
    for range in &ranges {
        for (line_index, (_, content_range, opcode_range)) in line_starts.iter().enumerate() {
            if range.start >= content_range.end {
                continue;
            }
            if range.end <= content_range.start {
                break;
            }
            if let Some(opcode_range) = opcode_range
                && range.start < opcode_range.end
                && opcode_range.start < range.end
            {
                promoted_lines[line_index] = true;
                any_promoted = true;
            }
        }
    }

    if !any_promoted {
        return ranges;
    }

    let mut emitted_full_line: Vec<bool> = vec![false; line_starts.len()];
    let mut output: Vec<Range<usize>> = Vec::with_capacity(ranges.len());

    for range in ranges {
        for (line_index, (_, content_range, _)) in line_starts.iter().enumerate() {
            if range.start >= content_range.end {
                continue;
            }
            if range.end <= content_range.start {
                break;
            }
            if promoted_lines[line_index] {
                if !emitted_full_line[line_index] {
                    emitted_full_line[line_index] = true;
                    output.push(content_range.clone());
                }
            } else {
                let clipped =
                    range.start.max(content_range.start)..range.end.min(content_range.end);
                if clipped.start < clipped.end {
                    output.push(clipped);
                }
            }
        }
    }

    merge_ranges(output)
}

/// True for lines consisting only of a `:label` declaration or `.line N`
/// directive — assembler-regenerated structural anchors that pair as
/// rebuild noise but signal real change when unpaired.
pub(crate) fn is_structural_only_line(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    if let Some(rest) = trimmed.strip_prefix(".line") {
        return rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace);
    }

    if let Some(rest) = trimmed.strip_prefix(':') {
        return !rest.is_empty() && rest.chars().all(is_label_char);
    }

    false
}

pub(crate) fn is_line_directive(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed
        .strip_prefix(".line")
        .is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
}

/// True if two non-identical smali lines share the same label-stripped
/// skeleton — the rebuild-noise signature when bytecode-offset changes
/// renumber every label below without touching real instructions.
pub(crate) fn lines_are_label_rename(lhs: &str, rhs: &str) -> bool {
    if lhs == rhs {
        return false;
    }
    label_free_skeleton(lhs) == label_free_skeleton(rhs)
}

/// Label-colon detection must mirror [`is_label_boundary`] /
/// [`is_label_char`] so this stays in sync with what
/// [`collect_ignored_line_ranges`] strips from AST ranges — otherwise the
/// type-separator colon in `field:Ltype;` would be mistaken for a label.
fn label_free_skeleton(line: &str) -> String {
    let bytes = line.as_bytes();
    let trimmed_start = bytes
        .iter()
        .position(|byte| *byte != b' ' && *byte != b'\t')
        .unwrap_or(bytes.len());

    if line.len() >= trimmed_start + 5
        && &line[trimmed_start..trimmed_start + 5] == ".line"
        && (line.len() == trimmed_start + 5
            || (bytes[trimmed_start + 5] as char).is_ascii_whitespace())
    {
        let mut result = String::with_capacity(trimmed_start + 6);
        result.push_str(&line[..trimmed_start]);
        result.push_str(".line N");
        return result;
    }

    let mut out = String::with_capacity(line.len());
    let mut cursor = 0;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        let is_label_start = byte == b':'
            && (cursor == 0 || is_label_boundary(bytes[cursor - 1] as char))
            && cursor + 1 < bytes.len()
            && is_label_char(bytes[cursor + 1] as char);

        if is_label_start {
            let mut end = cursor + 1;
            while end < bytes.len() && is_label_char(bytes[end] as char) {
                end += 1;
            }
            out.push_str(":LABEL");
            cursor = end;
        } else {
            out.push(byte as char);
            cursor += 1;
        }
    }

    out
}

/// Returns the absolute byte range of the opcode token, or `None` for
/// blank, comment-only, or directive (`.method`, `.line`, …) lines.
fn opcode_byte_range(line: &str, line_start: usize) -> Option<Range<usize>> {
    let bytes = line.as_bytes();
    let mut cursor = 0;

    while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
        cursor += 1;
    }
    if cursor >= bytes.len() || bytes[cursor] == b'#' {
        return None;
    }

    if bytes[cursor] == b':' {
        let label_len = leading_jmp_label_len(&line[cursor..])?;
        cursor += label_len;
        while cursor < bytes.len() && (bytes[cursor] == b' ' || bytes[cursor] == b'\t') {
            cursor += 1;
        }
        if cursor >= bytes.len() || bytes[cursor] == b'#' {
            return None;
        }
    }

    if bytes[cursor] == b'.' {
        return None;
    }

    let start = cursor;
    while cursor < bytes.len() && !bytes[cursor].is_ascii_whitespace() {
        cursor += 1;
    }
    if cursor == start {
        return None;
    }

    Some((line_start + start)..(line_start + cursor))
}

/// Strip `.line` directives, leading `:label` names, and trailing `#`
/// comments from `ranges`. Walks the source text by line — the tree-sitter
/// grammar doesn't always surface these as discrete nodes.
pub(crate) fn filter_ignored_ranges(source: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return ranges;
    }

    let ignored = ignored_ranges(source);
    if ignored.is_empty() {
        return ranges;
    }

    let mut filtered = Vec::new();

    for range in ranges {
        subtract_ignored_ranges(range, &ignored, &mut filtered);
    }

    merge_ranges(filtered)
}

fn subtract_ignored_ranges(
    range: Range<usize>,
    ignored_ranges: &[Range<usize>],
    filtered: &mut Vec<Range<usize>>,
) {
    let mut cursor = range.start;

    for ignored in ignored_ranges {
        if ignored.end <= cursor {
            continue;
        }
        if ignored.start >= range.end {
            break;
        }

        if ignored.start > cursor {
            filtered.push(cursor..ignored.start.min(range.end));
        }

        cursor = cursor.max(ignored.end.min(range.end));
        if cursor >= range.end {
            return;
        }
    }

    if cursor < range.end {
        filtered.push(cursor..range.end);
    }
}

fn ignored_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ignored = Vec::new();
    let mut line_start = 0;

    for line in source.split_inclusive('\n') {
        let line_len = line.len();
        let content_len = line.trim_end_matches(['\r', '\n']).len();
        collect_ignored_line_ranges(&line[..content_len], line_start, &mut ignored);
        line_start += line_len;
    }

    merge_ranges(ignored)
}

fn collect_ignored_line_ranges(line: &str, line_start: usize, ignored: &mut Vec<Range<usize>>) {
    let trimmed_start = line
        .bytes()
        .position(|byte| byte != b' ' && byte != b'\t')
        .unwrap_or(line.len());
    let trimmed = &line[trimmed_start..];

    if trimmed
        .strip_prefix(".line")
        .is_some_and(|rest| rest.is_empty() || rest.chars().next().is_some_and(char::is_whitespace))
    {
        ignored.push((line_start + trimmed_start)..(line_start + line.len()));
        return;
    }

    if let Some(label_len) = leading_jmp_label_len(trimmed) {
        ignored.push((line_start + trimmed_start)..(line_start + trimmed_start + label_len));
    }

    let bytes = trimmed.as_bytes();
    let mut cursor = 0;
    let mut in_string = false;
    let mut escaped = false;

    while cursor < bytes.len() {
        let byte = bytes[cursor];

        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }

            cursor += 1;
            continue;
        }

        if byte == b'"' {
            in_string = true;
            cursor += 1;
            continue;
        }

        if byte == b'#' {
            break;
        }

        if byte == b':' && (cursor == 0 || is_label_boundary(bytes[cursor - 1] as char)) {
            let mut end = cursor + 1;
            while end < bytes.len() && is_label_char(bytes[end] as char) {
                end += 1;
            }

            if end > cursor + 1 {
                ignored.push(
                    (line_start + trimmed_start + cursor)..(line_start + trimmed_start + end),
                );
                cursor = end;
                continue;
            }
        }

        cursor += 1;
    }
}

fn leading_jmp_label_len(line: &str) -> Option<usize> {
    let mut end = 0;
    let mut chars = line.chars().peekable();

    while let Some(&ch) = chars.peek() {
        if !is_label_char(ch) {
            break;
        }
        end += ch.len_utf8();
        chars.next();
    }

    if end == 0 || !line[end..].starts_with(':') {
        return None;
    }

    Some(end + 1)
}

fn is_label_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '{' | '}' | '(' | ')' | ',' | '-' | '>')
}

fn is_label_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '-' | '.' | '<' | '>')
}
