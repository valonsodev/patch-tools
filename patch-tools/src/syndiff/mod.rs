#![allow(dead_code)]

use std::ops::Range;

use self::syntax_graph::{SyntaxEdge, SyntaxRoute};
use self::syntax_tree::{BuildTreeOptions, SyntaxHint, SyntaxNode};

mod syntax_delimiters;
mod syntax_graph;
mod syntax_tree;

use self::syntax_tree::build_tree_with_options;
#[allow(unused_imports)]
pub use self::syntax_tree::{SyntaxTree, build_tree, build_tree_with_exclusions};
const EXCLUDED_SMALI_NODE_TYPES: &[&str] = &["jmp_label", "label", "line_directive"];
// const EXCLUDED_SMALI_NODE_TYPES: &[&str] = &["line_directive"];

type ByteRange = Range<usize>;
type DiffRanges = (Vec<ByteRange>, Vec<ByteRange>);

#[derive(Debug, Clone, Copy)]
pub struct SyntaxDiffOptions {
    pub graph_limit: usize,
}

impl Default for SyntaxDiffOptions {
    fn default() -> Self {
        Self {
            graph_limit: 1_000_000,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmaliSnippetKind {
    Method,
    ClassHeader,
}

pub fn diff_smali_snippets(
    lhs_source: &str,
    rhs_source: &str,
    kind: SmaliSnippetKind,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    let (lhs_ranges, rhs_ranges) = match kind {
        SmaliSnippetKind::Method => diff_smali_methods(lhs_source, rhs_source, options),
        SmaliSnippetKind::ClassHeader => {
            diff_smali_sources(lhs_source, rhs_source, None, None, options)
        }
    }?;

    Some((
        filter_ignored_smali_ranges(lhs_source, lhs_ranges),
        filter_ignored_smali_ranges(rhs_source, rhs_ranges),
    ))
}

pub fn diff_xml_snippets(
    lhs_source: &str,
    rhs_source: &str,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    let lhs_tree = parse_xml(lhs_source)?;
    let rhs_tree = parse_xml(rhs_source)?;
    diff_trees(&lhs_tree, &rhs_tree, None, None, options)
}

pub fn diff_trees(
    lhs_tree: &SyntaxTree,
    rhs_tree: &SyntaxTree,
    lhs_range: Option<&ByteRange>,
    rhs_range: Option<&ByteRange>,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    let options = options.unwrap_or_default();
    let route = syntax_graph::shortest_path(lhs_tree, rhs_tree, options.graph_limit)?;
    let (lhs_ranges, rhs_ranges) = collect_ranges(
        &route,
        lhs_tree,
        rhs_tree,
        lhs_range,
        rhs_range,
        options.graph_limit,
    );

    Some((merge_ranges(lhs_ranges), merge_ranges(rhs_ranges)))
}

fn diff_smali_methods(
    lhs_source: &str,
    rhs_source: &str,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    diff_smali_sources(lhs_source, rhs_source, None, None, options)
}

fn diff_smali_sources(
    lhs_source: &str,
    rhs_source: &str,
    lhs_range: Option<&ByteRange>,
    rhs_range: Option<&ByteRange>,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    let lhs_tree = parse_smali(lhs_source)?;
    let rhs_tree = parse_smali(rhs_source)?;
    diff_trees(&lhs_tree, &rhs_tree, lhs_range, rhs_range, options)
}

fn filter_ignored_smali_ranges(source: &str, ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return ranges;
    }

    let ignored_ranges = ignored_smali_ranges(source);
    if ignored_ranges.is_empty() {
        return ranges;
    }

    let mut filtered = Vec::new();

    for range in ranges {
        subtract_ignored_ranges(range, &ignored_ranges, &mut filtered);
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

fn ignored_smali_ranges(source: &str) -> Vec<Range<usize>> {
    let mut ignored = Vec::new();
    let mut line_start = 0;

    for line in source.split_inclusive('\n') {
        let line_len = line.len();
        let content_len = line.trim_end_matches(['\r', '\n']).len();
        collect_ignored_smali_line_ranges(&line[..content_len], line_start, &mut ignored);
        line_start += line_len;
    }

    merge_ranges(ignored)
}

fn collect_ignored_smali_line_ranges(
    line: &str,
    line_start: usize,
    ignored: &mut Vec<Range<usize>>,
) {
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
            while end < bytes.len() && is_smali_label_char(bytes[end] as char) {
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
        if !is_smali_label_char(ch) {
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

fn is_smali_label_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '$' | '-' | '.' | '<' | '>')
}

fn parse_smali(source: &str) -> Option<SyntaxTree<'_>> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_smali::LANGUAGE.into();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(source, None)?;
    if tree.root_node().has_error() {
        return None;
    }

    Some(build_tree_with_options(
        &tree.walk(),
        source,
        EXCLUDED_SMALI_NODE_TYPES,
        // Keep nearby instruction nodes paired so narrower subtree diffs can mark only operands.
        BuildTreeOptions::default().with_same_kind_replacements(true),
    ))
}

fn parse_xml(source: &str) -> Option<SyntaxTree<'_>> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_xml::LANGUAGE_XML.into();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(source, None)?;
    if tree.root_node().has_error() {
        return None;
    }

    Some(build_tree_with_options(
        &tree.walk(),
        source,
        &[],
        BuildTreeOptions::default()
            .with_skip_node(skip_insignificant_xml_node)
            .with_sort_children(sort_xml_children),
    ))
}

fn skip_insignificant_xml_node(node: tree_sitter::Node<'_>, source: &str) -> bool {
    node.kind() == "CharData"
        && source
            .get(node.byte_range())
            .is_some_and(|text| text.trim().is_empty())
}

fn sort_xml_children(
    parent: tree_sitter::Node<'_>,
    source: &str,
    children: &mut Vec<tree_sitter::Node<'_>>,
) {
    match parent.kind() {
        "STag" | "EmptyElemTag" => {
            children.sort_by(|lhs, rhs| compare_xml_tag_children(*lhs, *rhs, source));
        }
        _ => {}
    }
}

fn compare_xml_tag_children(
    lhs: tree_sitter::Node<'_>,
    rhs: tree_sitter::Node<'_>,
    source: &str,
) -> std::cmp::Ordering {
    xml_tag_child_sort_key(lhs, source).cmp(&xml_tag_child_sort_key(rhs, source))
}

fn xml_tag_child_sort_key<'a>(
    node: tree_sitter::Node<'_>,
    source: &'a str,
) -> (u8, &'a str, usize) {
    match node.kind() {
        "Name" => (
            0,
            source.get(node.byte_range()).unwrap_or_default(),
            node.start_byte(),
        ),
        "Attribute" => (
            1,
            xml_attribute_name(node, source).unwrap_or_default(),
            node.start_byte(),
        ),
        _ => (
            2,
            source.get(node.byte_range()).unwrap_or_default(),
            node.start_byte(),
        ),
    }
}

fn xml_attribute_name<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> Option<&'a str> {
    (0..node.child_count()).find_map(|index| {
        let child = node.child(u32::try_from(index).expect("child index overflow"))?;
        (child.kind() == "Name")
            .then(|| source.get(child.byte_range()))
            .flatten()
    })
}

fn collect_ranges(
    route: &SyntaxRoute<'_>,
    lhs_tree: &SyntaxTree,
    rhs_tree: &SyntaxTree,
    lhs_bounds: Option<&ByteRange>,
    rhs_bounds: Option<&ByteRange>,
    graph_limit: usize,
) -> DiffRanges {
    let mut lhs_ranges = Vec::new();
    let mut rhs_ranges = Vec::new();

    for path in &route.0 {
        let Some(edge) = path.edge else {
            continue;
        };
        let Some(vertex) = path.from.as_ref() else {
            continue;
        };

        match edge {
            SyntaxEdge::Replaced { levenshtein_pct } => {
                if let (Some(lhs_node), Some(rhs_node)) = (
                    vertex.lhs.id().map(|id| lhs_tree.get(id)),
                    vertex.rhs.id().map(|id| rhs_tree.get(id)),
                ) {
                    let recurse =
                        vertex
                            .lhs
                            .id()
                            .zip(vertex.rhs.id())
                            .and_then(|(lhs_id, rhs_id)| {
                                diff_paired_subtrees(
                                    lhs_tree,
                                    rhs_tree,
                                    lhs_id,
                                    rhs_id,
                                    lhs_bounds,
                                    rhs_bounds,
                                    graph_limit,
                                )
                            });

                    if let Some((lhs_replace_ranges, rhs_replace_ranges)) = recurse
                        .or_else(|| get_replace_ranges(lhs_node, rhs_node, lhs_bounds, rhs_bounds))
                    {
                        lhs_ranges.extend(lhs_replace_ranges);
                        rhs_ranges.extend(rhs_replace_ranges);
                    } else if levenshtein_pct <= 20 {
                        lhs_ranges.extend(get_novel_ranges(lhs_node, lhs_bounds));
                        rhs_ranges.extend(get_novel_ranges(rhs_node, rhs_bounds));
                    }
                }
            }
            SyntaxEdge::NovelAtomLHS | SyntaxEdge::EnterNovelDelimiterLHS => {
                if let Some(lhs_node) = vertex.lhs.id().map(|id| lhs_tree.get(id)) {
                    lhs_ranges.extend(get_novel_ranges(lhs_node, lhs_bounds));
                }
            }
            SyntaxEdge::NovelAtomRHS | SyntaxEdge::EnterNovelDelimiterRHS => {
                if let Some(rhs_node) = vertex.rhs.id().map(|id| rhs_tree.get(id)) {
                    rhs_ranges.extend(get_novel_ranges(rhs_node, rhs_bounds));
                }
            }
            SyntaxEdge::EnterNovelDelimiterBoth => {
                if let Some(lhs_node) = vertex.lhs.id().map(|id| lhs_tree.get(id)) {
                    lhs_ranges.extend(get_novel_ranges(lhs_node, lhs_bounds));
                }
                if let Some(rhs_node) = vertex.rhs.id().map(|id| rhs_tree.get(id)) {
                    rhs_ranges.extend(get_novel_ranges(rhs_node, rhs_bounds));
                }

                if let (Some(lhs_id), Some(rhs_id)) = (vertex.lhs.id(), vertex.rhs.id())
                    && let Some((lhs_subtree_ranges, rhs_subtree_ranges)) = diff_paired_subtrees(
                        lhs_tree,
                        rhs_tree,
                        lhs_id,
                        rhs_id,
                        lhs_bounds,
                        rhs_bounds,
                        graph_limit,
                    )
                {
                    lhs_ranges.extend(lhs_subtree_ranges);
                    rhs_ranges.extend(rhs_subtree_ranges);
                }
            }
            SyntaxEdge::Unchanged { .. } | SyntaxEdge::EnterUnchangedDelimiter { .. } => {}
        }
    }

    (lhs_ranges, rhs_ranges)
}

fn diff_paired_subtrees(
    lhs_tree: &SyntaxTree,
    rhs_tree: &SyntaxTree,
    lhs_id: syntax_tree::SyntaxId,
    rhs_id: syntax_tree::SyntaxId,
    lhs_bounds: Option<&ByteRange>,
    rhs_bounds: Option<&ByteRange>,
    graph_limit: usize,
) -> Option<DiffRanges> {
    let lhs_node = lhs_tree.get(lhs_id);
    let rhs_node = rhs_tree.get(rhs_id);

    if lhs_node.is_atom() || rhs_node.is_atom() {
        return None;
    }

    let lhs_subtree = lhs_tree.subtree(lhs_id);
    let rhs_subtree = rhs_tree.subtree(rhs_id);
    let route = syntax_graph::shortest_path_from(
        lhs_subtree.cursor().first_child(),
        rhs_subtree.cursor().first_child(),
        graph_limit,
    )?;

    let (lhs_ranges, rhs_ranges) = collect_ranges(
        &route,
        &lhs_subtree,
        &rhs_subtree,
        lhs_bounds,
        rhs_bounds,
        graph_limit,
    );

    Some((lhs_ranges, rhs_ranges))
}

fn get_novel_ranges(
    node: &SyntaxNode<'_>,
    bounds: Option<&ByteRange>,
) -> heapless::Vec<ByteRange, 2, u8> {
    let mut ranges = heapless::Vec::new();

    if node.is_atom() {
        let atom_range = node
            .delimited_content_range()
            .filter(|range| range.start < range.end)
            .unwrap_or_else(|| node.byte_range.clone());

        if let Some(range) = adjust_range_to_bounds(atom_range, bounds) {
            let _ = ranges.push(range);
        }
    } else {
        if let Some(range) = node
            .open_delimiter_range()
            .and_then(|range| adjust_range_to_bounds(range, bounds))
        {
            let _ = ranges.push(range);
        }

        if let Some(range) = node
            .close_delimiter_range()
            .and_then(|range| adjust_range_to_bounds(range, bounds))
        {
            let _ = ranges.push(range);
        }
    }

    ranges
}

fn get_replace_ranges(
    lhs_node: &SyntaxNode<'_>,
    rhs_node: &SyntaxNode<'_>,
    lhs_bounds: Option<&ByteRange>,
    rhs_bounds: Option<&ByteRange>,
) -> Option<DiffRanges> {
    if let (Some(SyntaxHint::Comment(_)), Some(SyntaxHint::Comment(_))) =
        (lhs_node.hint.as_ref(), rhs_node.hint.as_ref())
    {
        let lhs_offset = lhs_node.byte_range.start;
        let rhs_offset = rhs_node.byte_range.start;

        let lhs_ranges = Vec::<ByteRange>::new()
            .into_iter()
            .map(|range| (range.start + lhs_offset)..(range.end + lhs_offset))
            .filter_map(|range| adjust_range_to_bounds(range, lhs_bounds))
            .collect();

        let rhs_ranges = Vec::<ByteRange>::new()
            .into_iter()
            .map(|range| (range.start + rhs_offset)..(range.end + rhs_offset))
            .filter_map(|range| adjust_range_to_bounds(range, rhs_bounds))
            .collect();

        Some((lhs_ranges, rhs_ranges))
    } else {
        None
    }
}

fn merge_ranges(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
    if ranges.is_empty() {
        return ranges;
    }

    ranges.sort_by_key(|range| range.start);
    let mut merged = vec![ranges[0].clone()];

    for range in ranges.into_iter().skip(1) {
        let last = merged.last_mut().expect("merged is never empty");
        if range.start <= last.end {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }

    merged
}

fn adjust_range_to_bounds(range: ByteRange, bounds: Option<&ByteRange>) -> Option<ByteRange> {
    let Some(bounds) = bounds else {
        return Some(range);
    };

    if range.end <= bounds.start || range.start >= bounds.end {
        return None;
    }

    let start = range.start.max(bounds.start) - bounds.start;
    let end = range.end.min(bounds.end) - bounds.start;
    Some(start..end)
}
