use std::cmp::{Ordering, min};
use std::collections::BinaryHeap;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use rustc_hash::FxHashMap;

use super::syntax_delimiters::SyntaxDelimiters;
use super::syntax_tree::{SyntaxHint, SyntaxTree, SyntaxTreeCursor};

type Neighbours<'a> = heapless::Vec<(SyntaxEdge, SyntaxVertex<'a>), 8, u8>;

#[derive(Debug, Clone)]
pub(crate) struct SyntaxPath<'a> {
    pub(crate) from: Option<SyntaxVertex<'a>>,
    pub(crate) edge: Option<SyntaxEdge>,
    pub(crate) into: SyntaxVertex<'a>,
    pub(crate) cost: u32,
}

pub(crate) struct SyntaxRoute<'a>(pub(crate) Vec<SyntaxPath<'a>>);

impl Debug for SyntaxRoute<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "SyntaxRoute [")?;
        for path in &self.0 {
            let lhs_range = path.into.lhs.node().map(|node| &node.byte_range);
            let rhs_range = path.into.rhs.node().map(|node| &node.byte_range);
            writeln!(
                formatter,
                "  {:?} {:?} {:?}",
                path.edge, lhs_range, rhs_range
            )?;
        }
        write!(formatter, "]")
    }
}

impl PartialEq for SyntaxPath<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for SyntaxPath<'_> {}

impl PartialOrd for SyntaxPath<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SyntaxPath<'_> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.cost.cmp(&other.cost)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct SyntaxVertex<'a> {
    pub(crate) lhs: SyntaxTreeCursor<'a>,
    pub(crate) rhs: SyntaxTreeCursor<'a>,
    pub(crate) delimiters: SyntaxDelimiters,
}

impl<'a> SyntaxVertex<'a> {
    pub(crate) fn new(
        lhs: SyntaxTreeCursor<'a>,
        rhs: SyntaxTreeCursor<'a>,
        delimiters: SyntaxDelimiters,
    ) -> Self {
        Self {
            lhs,
            rhs,
            delimiters,
        }
    }

    pub(crate) fn is_end(&self) -> bool {
        self.lhs.is_end() && self.rhs.is_end() && self.delimiters.is_empty()
    }

    pub(crate) fn neighbours(&self) -> Neighbours<'a> {
        let mut neighbours = heapless::Vec::new();

        self.push_paired_neighbours(&mut neighbours);
        self.push_lhs_novel_neighbour(&mut neighbours);
        self.push_rhs_novel_neighbour(&mut neighbours);

        neighbours
    }

    fn push_paired_neighbours(&self, neighbours: &mut Neighbours<'a>) {
        if let (Some(lhs_node), Some(rhs_node)) = (self.lhs.node(), self.rhs.node()) {
            if lhs_node.structural_hash == rhs_node.structural_hash {
                let probably_punctuation = self
                    .lhs
                    .node()
                    .is_some_and(|node| node.hint == Some(SyntaxHint::Punctuation));
                push_unchanged_neighbour(
                    self.lhs.next_sibling(),
                    self.rhs.next_sibling(),
                    self.delimiters,
                    depth_difference(self.lhs.depth(), self.rhs.depth()),
                    probably_punctuation,
                    neighbours,
                );
            } else if let (
                Some(SyntaxHint::Comment(lhs_comment)),
                Some(SyntaxHint::Comment(rhs_comment)),
            ) = (lhs_node.hint.as_ref(), rhs_node.hint.as_ref())
            {
                push_replaced_comment_neighbour(
                    self.lhs.next_sibling(),
                    self.rhs.next_sibling(),
                    self.delimiters,
                    lhs_comment,
                    rhs_comment,
                    neighbours,
                );
            } else if can_replace_same_kind_nodes(self.lhs, self.rhs) {
                push_neighbour(
                    neighbours,
                    SyntaxEdge::Replaced { levenshtein_pct: 0 },
                    self.lhs.next_sibling(),
                    self.rhs.next_sibling(),
                    self.delimiters,
                );
            }

            if lhs_node.is_list() && rhs_node.is_list() {
                let delimiters_match = lhs_node.has_delimiters()
                    && rhs_node.has_delimiters()
                    && lhs_node.open_delimiter() == rhs_node.open_delimiter()
                    && lhs_node.close_delimiter() == rhs_node.close_delimiter();
                push_list_neighbour(
                    self.lhs.first_child(),
                    self.rhs.first_child(),
                    self.delimiters,
                    delimiters_match,
                    depth_difference(self.lhs.depth(), self.rhs.depth()),
                    neighbours,
                );
            }
        }
    }

    fn push_lhs_novel_neighbour(&self, neighbours: &mut Neighbours<'a>) {
        if let Some(lhs_node) = self.lhs.node() {
            if lhs_node.is_atom() {
                let (lhs, rhs, delimiters) =
                    pop_all_delimiters(self.lhs.next_sibling(), self.rhs, self.delimiters);
                let _ = neighbours.push((
                    SyntaxEdge::NovelAtomLHS,
                    SyntaxVertex {
                        lhs,
                        rhs,
                        delimiters,
                    },
                ));
            } else {
                let delimiters = self.delimiters.push_lhs();
                let (lhs, rhs, delimiters) =
                    pop_all_delimiters(self.lhs.first_child(), self.rhs, delimiters);
                let _ = neighbours.push((
                    SyntaxEdge::EnterNovelDelimiterLHS,
                    SyntaxVertex {
                        lhs,
                        rhs,
                        delimiters,
                    },
                ));
            }
        }
    }

    fn push_rhs_novel_neighbour(&self, neighbours: &mut Neighbours<'a>) {
        if let Some(rhs_node) = self.rhs.node() {
            if rhs_node.is_atom() {
                let (lhs, rhs, delimiters) =
                    pop_all_delimiters(self.lhs, self.rhs.next_sibling(), self.delimiters);
                let _ = neighbours.push((
                    SyntaxEdge::NovelAtomRHS,
                    SyntaxVertex {
                        lhs,
                        rhs,
                        delimiters,
                    },
                ));
            } else {
                let delimiters = self.delimiters.push_rhs();
                let (lhs, rhs, delimiters) =
                    pop_all_delimiters(self.lhs, self.rhs.first_child(), delimiters);
                let _ = neighbours.push((
                    SyntaxEdge::EnterNovelDelimiterRHS,
                    SyntaxVertex {
                        lhs,
                        rhs,
                        delimiters,
                    },
                ));
            }
        }
    }

    fn can_pop_either(&self) -> bool {
        self.delimiters.can_pop_lhs() || self.delimiters.can_pop_rhs()
    }
}

fn can_replace_same_kind_nodes(lhs: SyntaxTreeCursor<'_>, rhs: SyntaxTreeCursor<'_>) -> bool {
    let (Some(lhs_id), Some(rhs_id)) = (lhs.id(), rhs.id()) else {
        return false;
    };
    let lhs_tree = lhs.tree();
    let rhs_tree = rhs.tree();
    let lhs_node = lhs_tree.get(lhs_id);
    let rhs_node = rhs_tree.get(rhs_id);

    if !lhs_tree.allow_same_kind_replacements()
        || !rhs_tree.allow_same_kind_replacements()
        || lhs_node.kind_id != rhs_node.kind_id
        || lhs_node.has_delimiters()
        || rhs_node.has_delimiters()
        || lhs_node.is_atom()
        || rhs_node.is_atom()
    {
        return false;
    }

    let (Some(lhs_first_child), Some(rhs_first_child)) =
        (lhs_tree.first_child(lhs_id), rhs_tree.first_child(rhs_id))
    else {
        return false;
    };

    lhs_tree.get(lhs_first_child).structural_hash == rhs_tree.get(rhs_first_child).structural_hash
}

fn push_neighbour<'a>(
    neighbours: &mut Neighbours<'a>,
    edge: SyntaxEdge,
    lhs: SyntaxTreeCursor<'a>,
    rhs: SyntaxTreeCursor<'a>,
    delimiters: SyntaxDelimiters,
) {
    let _ = neighbours.push((
        edge,
        SyntaxVertex {
            lhs,
            rhs,
            delimiters,
        },
    ));
}

fn push_unchanged_neighbour<'a>(
    lhs: SyntaxTreeCursor<'a>,
    rhs: SyntaxTreeCursor<'a>,
    delimiters: SyntaxDelimiters,
    depth_difference: u32,
    probably_punctuation: bool,
    neighbours: &mut Neighbours<'a>,
) {
    let (lhs, rhs, delimiters) = pop_all_delimiters(lhs, rhs, delimiters);
    push_neighbour(
        neighbours,
        SyntaxEdge::Unchanged {
            depth_difference,
            probably_punctuation,
        },
        lhs,
        rhs,
        delimiters,
    );
}

fn push_replaced_comment_neighbour<'a>(
    lhs: SyntaxTreeCursor<'a>,
    rhs: SyntaxTreeCursor<'a>,
    delimiters: SyntaxDelimiters,
    lhs_comment: &str,
    rhs_comment: &str,
    neighbours: &mut Neighbours<'a>,
) {
    let (lhs, rhs, delimiters) = pop_all_delimiters(lhs, rhs, delimiters);
    push_neighbour(
        neighbours,
        SyntaxEdge::Replaced {
            levenshtein_pct: levenshtein_pct(lhs_comment, rhs_comment),
        },
        lhs,
        rhs,
        delimiters,
    );
}

fn push_list_neighbour<'a>(
    lhs: SyntaxTreeCursor<'a>,
    rhs: SyntaxTreeCursor<'a>,
    delimiters: SyntaxDelimiters,
    delimiters_match: bool,
    depth_difference: u32,
    neighbours: &mut Neighbours<'a>,
) {
    let (edge, delimiters) = if delimiters_match {
        (
            SyntaxEdge::EnterUnchangedDelimiter { depth_difference },
            delimiters.push_both(),
        )
    } else {
        (
            SyntaxEdge::EnterNovelDelimiterBoth,
            delimiters.push_lhs().push_rhs(),
        )
    };

    let (lhs, rhs, delimiters) = pop_all_delimiters(lhs, rhs, delimiters);
    push_neighbour(neighbours, edge, lhs, rhs, delimiters);
}

fn depth_difference(lhs_depth: usize, rhs_depth: usize) -> u32 {
    u32::try_from(lhs_depth.abs_diff(rhs_depth)).expect("syntax depth difference exceeds u32")
}

#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn levenshtein_pct(lhs_comment: &str, rhs_comment: &str) -> u8 {
    (strsim::normalized_levenshtein(lhs_comment, rhs_comment) * 100.0)
        .round()
        .clamp(0.0, 100.0) as u8
}

impl PartialEq for SyntaxVertex<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.lhs == other.lhs
            && self.rhs == other.rhs
            && self.can_pop_either() == other.can_pop_either()
    }
}

impl Eq for SyntaxVertex<'_> {}

impl Hash for SyntaxVertex<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.lhs.hash(state);
        self.rhs.hash(state);
        self.can_pop_either().hash(state);
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub(crate) enum SyntaxEdge {
    Unchanged {
        depth_difference: u32,
        probably_punctuation: bool,
    },
    EnterUnchangedDelimiter {
        depth_difference: u32,
    },
    Replaced {
        levenshtein_pct: u8,
    },
    NovelAtomLHS,
    NovelAtomRHS,
    EnterNovelDelimiterLHS,
    EnterNovelDelimiterRHS,
    EnterNovelDelimiterBoth,
}

impl SyntaxEdge {
    /// Cost weights for the shortest-path search. Lower = preferred.
    ///
    /// - **`Unchanged`** (`1..=40`): cheapest possible transition. Base grows
    ///   linearly with depth difference up to a cap of 40, so a node matching
    ///   its sibling costs ~1 but a node matching a deep nephew costs more.
    ///   When the matched atom is mere punctuation (`,` / `;` / `.`) we add a
    ///   200 penalty — without it the search happily chains punctuation
    ///   matches over real content changes and produces nonsense diffs.
    /// - **`EnterUnchangedDelimiter`** (`100..=140`): entering a matching
    ///   delimited subtree (`{…}`, `(…)`). Costs more than a plain unchanged
    ///   node because it commits to the delimiter pairing for the duration of
    ///   that subtree.
    /// - **`NovelAtomLHS` / `NovelAtomRHS` / `EnterNovelDelimiter{LHS,RHS}`**
    ///   (300): single-sided insertions or deletions. The same cost for atoms
    ///   and delimiter entries on a single side keeps the search neutral
    ///   between flat and nested edit shapes.
    /// - **`EnterNovelDelimiterBoth`** (550): both sides entered a different
    ///   delimiter. Strictly worse than two single-sided Novels would be
    ///   (`2 × 300 = 600`) — and that's intentional, but the slight discount
    ///   below `2 × Novel` lets the search prefer it when the surrounding
    ///   context is more consistent with paired-but-different delimiters than
    ///   with two independent novels.
    /// - **`Replaced`** (`500..=600`): same-kind replacement (e.g. Smali
    ///   instruction with different operands, or comment with different text).
    ///   Scales with Levenshtein similarity: identical text → 500, completely
    ///   dissimilar → 600.
    ///
    /// Invariants to preserve when tuning:
    /// - `Replaced` cost stays > single-sided `Novel` cost, otherwise the
    ///   search prefers spurious renames over honest insert+delete pairs.
    /// - Punctuation penalty (200) stays comfortably above the
    ///   `Unchanged` base ceiling (40), otherwise the search uses punctuation
    ///   matches to bridge unrelated content.
    pub(crate) fn cost(self) -> u32 {
        match self {
            Self::Unchanged {
                depth_difference,
                probably_punctuation,
            } => {
                let base = min(40, depth_difference + 1);
                base + if probably_punctuation { 200 } else { 0 }
            }
            Self::EnterUnchangedDelimiter { depth_difference } => 100 + min(40, depth_difference),
            Self::NovelAtomLHS
            | Self::NovelAtomRHS
            | Self::EnterNovelDelimiterLHS
            | Self::EnterNovelDelimiterRHS => 300,
            Self::EnterNovelDelimiterBoth => 550,
            Self::Replaced { levenshtein_pct } => 500 + u32::from(100 - levenshtein_pct),
        }
    }
}

fn pop_all_delimiters<'a>(
    mut lhs: SyntaxTreeCursor<'a>,
    mut rhs: SyntaxTreeCursor<'a>,
    mut delimiters: SyntaxDelimiters,
) -> (SyntaxTreeCursor<'a>, SyntaxTreeCursor<'a>, SyntaxDelimiters) {
    loop {
        let mut popped = false;

        while lhs.is_end() {
            if let Some(new_delimiters) = delimiters.pop_lhs() {
                lhs = lhs.last().parent().next_sibling();
                delimiters = new_delimiters;
                popped = true;
            } else {
                break;
            }
        }

        while rhs.is_end() {
            if let Some(new_delimiters) = delimiters.pop_rhs() {
                rhs = rhs.last().parent().next_sibling();
                delimiters = new_delimiters;
                popped = true;
            } else {
                break;
            }
        }

        if lhs.is_end()
            && rhs.is_end()
            && let Some(new_delimiters) = delimiters.pop_both()
        {
            lhs = lhs.last().parent().next_sibling();
            rhs = rhs.last().parent().next_sibling();
            delimiters = new_delimiters;
            popped = true;
        }

        if !popped {
            break;
        }
    }

    (lhs, rhs, delimiters)
}

/// Ordered by `cost` only (vertex types aren't `Ord`); sorted as a min-heap
/// via reversed comparison in [`Ord::cmp`].
struct HeapEntry<'a> {
    cost: u32,
    vertex: SyntaxVertex<'a>,
}

impl PartialEq for HeapEntry<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.cost == other.cost
    }
}

impl Eq for HeapEntry<'_> {}

impl PartialOrd for HeapEntry<'_> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for HeapEntry<'_> {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap; reverse so smallest cost pops first.
        other.cost.cmp(&self.cost)
    }
}

pub(crate) fn shortest_path<'a>(
    lhs_tree: &'a SyntaxTree,
    rhs_tree: &'a SyntaxTree,
    graph_limit: usize,
) -> Option<SyntaxRoute<'a>> {
    shortest_path_from(lhs_tree.cursor(), rhs_tree.cursor(), graph_limit)
}

pub(crate) fn shortest_path_from<'a>(
    lhs_cursor: SyntaxTreeCursor<'a>,
    rhs_cursor: SyntaxTreeCursor<'a>,
    graph_limit: usize,
) -> Option<SyntaxRoute<'a>> {
    let graph_limit = min(
        2 * lhs_cursor.tree().len() * rhs_cursor.tree().len(),
        graph_limit,
    );

    let start = SyntaxVertex::new(lhs_cursor, rhs_cursor, SyntaxDelimiters::default());
    let mut heap: BinaryHeap<HeapEntry<'a>> = BinaryHeap::new();
    let mut dist: FxHashMap<SyntaxVertex<'a>, u32> = FxHashMap::default();
    let mut parent: FxHashMap<SyntaxVertex<'a>, (SyntaxVertex<'a>, SyntaxEdge)> =
        FxHashMap::default();

    dist.insert(start.clone(), 0);
    heap.push(HeapEntry {
        cost: 0,
        vertex: start.clone(),
    });

    while let Some(HeapEntry { cost, vertex }) = heap.pop() {
        if vertex.is_end() {
            return Some(SyntaxRoute(reconstruct_route(&parent, &start, vertex)));
        }

        // Skip stale heap entries (a shorter path to this vertex was found later).
        if dist.get(&vertex).copied() != Some(cost) {
            continue;
        }

        if dist.len() > graph_limit {
            return None;
        }

        for (edge, next) in vertex.neighbours() {
            let next_cost = cost + edge.cost();
            let existing = dist.get(&next).copied().unwrap_or(u32::MAX);
            if next_cost < existing {
                dist.insert(next.clone(), next_cost);
                parent.insert(next.clone(), (vertex.clone(), edge));
                heap.push(HeapEntry {
                    cost: next_cost,
                    vertex: next,
                });
            }
        }
    }

    None
}

fn reconstruct_route<'a>(
    parent: &FxHashMap<SyntaxVertex<'a>, (SyntaxVertex<'a>, SyntaxEdge)>,
    start: &SyntaxVertex<'a>,
    goal: SyntaxVertex<'a>,
) -> Vec<SyntaxPath<'a>> {
    let mut reverse_path: Vec<(SyntaxVertex<'a>, SyntaxEdge)> = Vec::new();
    let mut current = goal;
    while current != *start {
        let Some((prev, edge)) = parent.get(&current) else {
            break;
        };
        reverse_path.push((current.clone(), *edge));
        current = prev.clone();
    }
    reverse_path.reverse();

    let mut route = Vec::with_capacity(reverse_path.len());
    let mut cumulative_cost = 0u32;
    let mut from = start.clone();
    for (into, edge) in reverse_path {
        cumulative_cost += edge.cost();
        route.push(SyntaxPath {
            from: Some(from),
            edge: Some(edge),
            into: into.clone(),
            cost: cumulative_cost,
        });
        from = into;
    }

    route
}
