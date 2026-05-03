use std::cell::{Cell, RefCell};
use std::cmp::min;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

use pathfinding::prelude::dijkstra;
use rustc_hash::FxHashSet;

use super::syntax_delimiters::SyntaxDelimiters;
use super::syntax_tree::{SyntaxHint, SyntaxTree, SyntaxTreeCursor};

type Neighbours<'a> = heapless::Vec<(SyntaxEdge, SyntaxVertex<'a>), 8, u8>;

#[derive(Debug, Clone)]
pub struct SyntaxPath<'a> {
    pub from: Option<SyntaxVertex<'a>>,
    pub edge: Option<SyntaxEdge>,
    pub into: SyntaxVertex<'a>,
    pub cost: u32,
}

pub struct SyntaxRoute<'a>(pub Vec<SyntaxPath<'a>>);

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
pub struct SyntaxVertex<'a> {
    pub lhs: SyntaxTreeCursor<'a>,
    pub rhs: SyntaxTreeCursor<'a>,
    pub delimiters: SyntaxDelimiters,
}

impl<'a> SyntaxVertex<'a> {
    pub fn new(
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

    pub fn is_end(&self) -> bool {
        self.lhs.is_end() && self.rhs.is_end() && self.delimiters.is_empty()
    }

    pub fn neighbours(&self) -> Neighbours<'a> {
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

    fn edge_to(&self, next: &SyntaxVertex<'a>) -> Option<SyntaxEdge> {
        self.neighbours()
            .into_iter()
            .filter_map(|(edge, candidate)| (candidate == *next).then_some(edge))
            .min_by_key(|edge| edge.cost())
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
pub enum SyntaxEdge {
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
    pub fn cost(self) -> u32 {
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

pub fn shortest_path<'a>(
    lhs_tree: &'a SyntaxTree,
    rhs_tree: &'a SyntaxTree,
    graph_limit: usize,
) -> Option<SyntaxRoute<'a>> {
    shortest_path_from(lhs_tree.cursor(), rhs_tree.cursor(), graph_limit)
}

pub fn shortest_path_from<'a>(
    lhs_cursor: SyntaxTreeCursor<'a>,
    rhs_cursor: SyntaxTreeCursor<'a>,
    graph_limit: usize,
) -> Option<SyntaxRoute<'a>> {
    let graph_limit = std::cmp::min(
        2 * lhs_cursor.tree().len() * rhs_cursor.tree().len(),
        graph_limit,
    );

    let start = SyntaxVertex::new(lhs_cursor, rhs_cursor, SyntaxDelimiters::default());
    let visited = RefCell::new(FxHashSet::default());
    let aborted = Cell::new(false);
    let (vertices, _cost) = dijkstra(
        &start,
        |vertex| {
            if aborted.get() {
                return Vec::new();
            }

            {
                let mut visited = visited.borrow_mut();
                if visited.insert(vertex.clone()) && visited.len() > graph_limit {
                    aborted.set(true);
                    return Vec::new();
                }
            }

            vertex
                .neighbours()
                .into_iter()
                .map(|(edge, next_vertex)| (next_vertex, edge.cost()))
                .collect::<Vec<_>>()
        },
        SyntaxVertex::is_end,
    )?;

    if aborted.get() {
        return None;
    }

    Some(SyntaxRoute(reconstruct_route(&vertices)))
}

fn reconstruct_route<'a>(vertices: &[SyntaxVertex<'a>]) -> Vec<SyntaxPath<'a>> {
    let mut route = Vec::with_capacity(vertices.len().saturating_sub(1));
    let mut cost = 0;

    for window in vertices.windows(2) {
        let from = window[0].clone();
        let into = window[1].clone();
        let edge = from
            .edge_to(&into)
            .expect("shortest path should only contain valid syntax transitions");
        cost += edge.cost();
        route.push(SyntaxPath {
            from: Some(from),
            edge: Some(edge),
            into,
            cost,
        });
    }

    route
}
