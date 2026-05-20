use std::ops::Range;

use self::languages::Language;
use self::syntax_graph::{SyntaxEdge, SyntaxRoute};
use self::syntax_tree::{SyntaxHint, SyntaxNode, SyntaxTree};

mod languages;
mod syntax_defaults;
mod syntax_delimiters;
mod syntax_graph;
mod syntax_tree;

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

pub fn diff_smali_snippets(
    lhs_source: &str,
    rhs_source: &str,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    diff_with_language(Language::Smali, lhs_source, rhs_source, options)
}

/// Re-export of the Smali "structural-only line" predicate so the rendering
/// layer can tell `:cond_0` / `.line N` lines apart from real instructions
/// when deciding which line-level Delete/Insert pairs are pure rename noise
/// and which are part of a real structural change.
pub fn is_smali_structural_only_line(line: &str) -> bool {
    languages::smali::is_structural_only_line(line)
}

/// Re-export of the "label-rename pair" predicate: two non-identical
/// smali lines that share the same label-stripped skeleton (different
/// `:cond_NN`, `:try_end_NN`, `.line N` only). The rendering layer uses
/// it to suppress the wall of Delete/Insert noise that appears after a
/// patch shifts every bytecode offset in a method.
pub fn smali_lines_are_label_rename(lhs: &str, rhs: &str) -> bool {
    languages::smali::lines_are_label_rename(lhs, rhs)
}

pub fn diff_xml_snippets(
    lhs_source: &str,
    rhs_source: &str,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    diff_with_language(Language::Xml, lhs_source, rhs_source, options)
}

fn diff_with_language(
    lang: Language,
    lhs_source: &str,
    rhs_source: &str,
    options: Option<SyntaxDiffOptions>,
) -> Option<DiffRanges> {
    let lhs_tree = languages::parse(lang, lhs_source)?;
    let rhs_tree = languages::parse(lang, rhs_source)?;
    let (lhs_ranges, rhs_ranges) = diff_trees(&lhs_tree, &rhs_tree, None, None, options)?;
    Some((
        languages::post_filter(lang, lhs_source, lhs_ranges),
        languages::post_filter(lang, rhs_source, rhs_ranges),
    ))
}

pub(crate) fn diff_trees(
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

                    if let Some((lhs_replace_ranges, rhs_replace_ranges)) =
                        recurse.or_else(|| get_replace_ranges(lhs_node, rhs_node))
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

/// When both replaced nodes are comments, suppress the per-comment highlight:
/// returning `Some((empty, empty))` consumes the `Replaced` edge in
/// `collect_ranges` and skips the `levenshtein_pct <= 20` novel-range
/// fallback, so commented changes don't bleed onto the diff. Returns `None`
/// for any other node-type pair so the caller can apply its fallback logic.
fn get_replace_ranges(
    lhs_node: &SyntaxNode<'_>,
    rhs_node: &SyntaxNode<'_>,
) -> Option<DiffRanges> {
    matches!(
        (lhs_node.hint.as_ref(), rhs_node.hint.as_ref()),
        (Some(SyntaxHint::Comment(_)), Some(SyntaxHint::Comment(_)))
    )
    .then(|| (Vec::new(), Vec::new()))
}

pub(crate) fn merge_ranges(mut ranges: Vec<ByteRange>) -> Vec<ByteRange> {
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
