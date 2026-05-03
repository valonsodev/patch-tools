use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;
use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SyntaxId(NonZeroUsize);

impl SyntaxId {
    fn new(index: usize) -> Self {
        Self(NonZeroUsize::new(index + 1).expect("index overflow"))
    }

    #[inline]
    pub fn index(self) -> usize {
        self.0.get() - 1
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SyntaxHint<'a> {
    String,
    Comment(&'a str),
    Punctuation,
}

#[derive(Debug)]
pub struct SyntaxTree<'a> {
    nodes: Vec<SyntaxNode<'a>>,
    allow_same_kind_replacements: bool,
}

#[derive(Debug, Clone)]
pub struct SyntaxNode<'a> {
    pub id: SyntaxId,
    pub kind_id: u16,
    pub structural_hash: u64,
    pub byte_range: Range<usize>,
    pub hint: Option<SyntaxHint<'a>>,
    pub delimiters: Delimiters<'a>,
    pub descendant_count: usize,
    pub depth: usize,
    parent: Option<SyntaxId>,
}

type Delimiters<'a> = [Option<(Range<usize>, &'a str)>; 2];

impl SyntaxNode<'_> {
    #[inline]
    pub fn open_delimiter(&self) -> Option<&str> {
        self.delimiters[0].as_ref().map(|delimiter| delimiter.1)
    }

    #[inline]
    pub fn close_delimiter(&self) -> Option<&str> {
        self.delimiters[1].as_ref().map(|delimiter| delimiter.1)
    }

    #[inline]
    pub fn open_delimiter_range(&self) -> Option<Range<usize>> {
        self.delimiters[0]
            .as_ref()
            .map(|delimiter| delimiter.0.clone())
    }

    #[inline]
    pub fn close_delimiter_range(&self) -> Option<Range<usize>> {
        self.delimiters[1]
            .as_ref()
            .map(|delimiter| delimiter.0.clone())
    }

    #[inline]
    pub fn delimited_content_range(&self) -> Option<Range<usize>> {
        delimited_content_range(self.byte_range.clone(), &self.delimiters)
    }

    #[inline]
    pub fn has_delimiters(&self) -> bool {
        self.delimiters[0].is_some() && self.delimiters[1].is_some()
    }

    #[inline]
    pub fn is_list(&self) -> bool {
        self.descendant_count > 0
    }

    #[inline]
    pub fn is_atom(&self) -> bool {
        self.descendant_count == 0
    }
}

impl<'a> SyntaxTree<'a> {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            allow_same_kind_replacements: false,
        }
    }

    pub fn root(&self) -> Option<SyntaxId> {
        if self.nodes.is_empty() {
            None
        } else {
            Some(SyntaxId::new(0))
        }
    }

    #[inline]
    pub fn get(&self, id: SyntaxId) -> &SyntaxNode<'a> {
        &self.nodes[id.index()]
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn first_child(&self, id: SyntaxId) -> Option<SyntaxId> {
        let node = self.get(id);
        if node.descendant_count > 0 {
            Some(SyntaxId::new(id.index() + 1))
        } else {
            None
        }
    }

    pub fn next_sibling(&self, id: SyntaxId) -> Option<SyntaxId> {
        let node = self.get(id);
        let next_index = id.index() + 1 + node.descendant_count;

        let parent_id = node.parent?;
        let parent = self.get(parent_id);
        let parent_end = parent_id.index() + 1 + parent.descendant_count;

        if next_index < parent_end {
            Some(SyntaxId::new(next_index))
        } else {
            None
        }
    }

    #[inline]
    pub fn parent(&self, id: SyntaxId) -> Option<SyntaxId> {
        self.get(id).parent
    }

    pub fn preorder(&self) -> impl Iterator<Item = SyntaxId> + '_ {
        (0..self.nodes.len()).map(SyntaxId::new)
    }

    pub fn cursor(&self) -> SyntaxTreeCursor<'_> {
        SyntaxTreeCursor {
            tree: self,
            last: None,
            current: self.root(),
        }
    }

    pub fn cursor_at(&self, node: SyntaxId) -> SyntaxTreeCursor<'_> {
        SyntaxTreeCursor {
            tree: self,
            last: None,
            current: Some(node),
        }
    }

    pub fn subtree(&self, root: SyntaxId) -> Self {
        let root_node = self.get(root);
        let start = root.index();
        let end = start + root_node.descendant_count + 1;
        let depth_offset = root_node.depth;

        let nodes = self.nodes[start..end]
            .iter()
            .enumerate()
            .map(|(relative_index, node)| {
                let parent = node.parent.and_then(|parent| {
                    (parent.index() >= start && parent.index() < end)
                        .then(|| SyntaxId::new(parent.index() - start))
                });

                SyntaxNode {
                    id: SyntaxId::new(relative_index),
                    kind_id: node.kind_id,
                    structural_hash: node.structural_hash,
                    byte_range: node.byte_range.clone(),
                    hint: node.hint.clone(),
                    delimiters: node.delimiters.clone(),
                    descendant_count: node.descendant_count,
                    depth: node.depth - depth_offset,
                    parent,
                }
            })
            .collect();

        Self {
            nodes,
            allow_same_kind_replacements: self.allow_same_kind_replacements,
        }
    }

    #[inline]
    pub fn allow_same_kind_replacements(&self) -> bool {
        self.allow_same_kind_replacements
    }
}

impl Default for SyntaxTree<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SyntaxTreeCursor<'a> {
    tree: &'a SyntaxTree<'a>,
    last: Option<SyntaxId>,
    current: Option<SyntaxId>,
}

impl<'a> SyntaxTreeCursor<'a> {
    #[inline]
    pub fn id(&self) -> Option<SyntaxId> {
        self.current
    }

    #[inline]
    pub fn node(&self) -> Option<&'a SyntaxNode<'_>> {
        self.current.map(|id| self.tree.get(id))
    }

    #[inline]
    pub fn is_end(&self) -> bool {
        self.current.is_none()
    }

    #[inline]
    pub fn tree(&self) -> &'a SyntaxTree<'_> {
        self.tree
    }

    pub fn goto_first_child(&mut self) -> bool {
        if let Some(id) = self.current
            && let Some(child) = self.tree.first_child(id)
        {
            self.last = Some(id);
            self.current = Some(child);
            return true;
        }
        false
    }

    pub fn goto_next_sibling(&mut self) -> bool {
        if let Some(id) = self.current
            && let Some(sibling) = self.tree.next_sibling(id)
        {
            self.last = Some(id);
            self.current = Some(sibling);
            return true;
        }
        false
    }

    pub fn goto_parent(&mut self) -> bool {
        if let Some(id) = self.current
            && let Some(parent) = self.tree.parent(id)
        {
            self.last = Some(id);
            self.current = Some(parent);
            return true;
        }
        false
    }

    pub fn goto_last(&mut self) -> bool {
        if let Some(id) = self.last {
            self.last = self.current;
            self.current = Some(id);
            return true;
        }
        false
    }

    #[inline]
    pub fn first_child(&self) -> Self {
        Self {
            tree: self.tree,
            last: self.current.or(self.last),
            current: self.current.and_then(|id| self.tree.first_child(id)),
        }
    }

    #[inline]
    pub fn next_sibling(&self) -> Self {
        Self {
            tree: self.tree,
            last: self.current.or(self.last),
            current: self.current.and_then(|id| self.tree.next_sibling(id)),
        }
    }

    #[inline]
    pub fn last(&self) -> Self {
        Self {
            tree: self.tree,
            last: self.current,
            current: self.last,
        }
    }

    #[inline]
    pub fn parent(&self) -> Self {
        Self {
            tree: self.tree,
            last: self.current.or(self.last),
            current: self.current.and_then(|id| self.tree.parent(id)),
        }
    }

    #[inline]
    pub fn depth(&self) -> usize {
        self.current.map_or(0, |id| self.tree.get(id).depth)
    }
}

impl PartialEq for SyntaxTreeCursor<'_> {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self.tree, other.tree) && self.current == other.current
    }
}

impl Eq for SyntaxTreeCursor<'_> {}

impl Hash for SyntaxTreeCursor<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.current.hash(state);
    }
}

pub fn build_tree<'a>(cursor: &tree_sitter::TreeCursor<'_>, source: &'a str) -> SyntaxTree<'a> {
    build_tree_with_options(cursor, source, &[], BuildTreeOptions::default())
}

pub fn build_tree_with_exclusions<'a>(
    cursor: &tree_sitter::TreeCursor<'_>,
    source: &'a str,
    excluded_node_types: &[&str],
) -> SyntaxTree<'a> {
    build_tree_with_options(
        cursor,
        source,
        excluded_node_types,
        BuildTreeOptions::default(),
    )
}

#[derive(Clone, Copy, Default)]
pub struct BuildTreeOptions {
    skip_node: Option<fn(tree_sitter::Node<'_>, &str) -> bool>,
    sort_children: Option<fn(tree_sitter::Node<'_>, &str, &mut Vec<tree_sitter::Node<'_>>)>,
    allow_same_kind_replacements: bool,
}

impl BuildTreeOptions {
    #[inline]
    pub const fn with_skip_node(
        mut self,
        skip_node: fn(tree_sitter::Node<'_>, &str) -> bool,
    ) -> Self {
        self.skip_node = Some(skip_node);
        self
    }

    #[inline]
    pub const fn with_sort_children(
        mut self,
        sort_children: fn(tree_sitter::Node<'_>, &str, &mut Vec<tree_sitter::Node<'_>>),
    ) -> Self {
        self.sort_children = Some(sort_children);
        self
    }

    #[inline]
    pub const fn with_same_kind_replacements(mut self, allow: bool) -> Self {
        self.allow_same_kind_replacements = allow;
        self
    }
}

pub fn build_tree_with_options<'a>(
    cursor: &tree_sitter::TreeCursor<'_>,
    source: &'a str,
    excluded_node_types: &[&str],
    options: BuildTreeOptions,
) -> SyntaxTree<'a> {
    let mut nodes = Vec::new();
    let root = cursor.node();

    if root.child_count() > 0 || !root.is_extra() {
        let _ = build_tree_recursive(root, &mut nodes, None, source, excluded_node_types, options);
    }

    SyntaxTree {
        nodes,
        allow_same_kind_replacements: options.allow_same_kind_replacements,
    }
}

fn build_tree_recursive<'a>(
    mut ts_node: tree_sitter::Node<'_>,
    nodes: &mut Vec<SyntaxNode<'a>>,
    parent: Option<SyntaxId>,
    source: &'a str,
    excluded_node_types: &[&str],
    options: BuildTreeOptions,
) -> Option<SyntaxId> {
    let flattened = ts_node
        .child(0)
        .filter(|child| ts_node.child_count() == 1 && ts_node.byte_range() == child.byte_range());

    if let Some(flattened) = flattened {
        ts_node = flattened;
    }

    if excluded_node_types.contains(&ts_node.kind())
        || options
            .skip_node
            .is_some_and(|skip_node| skip_node(ts_node, source))
    {
        return None;
    }

    let this_id = SyntaxId::new(nodes.len());
    nodes.push(SyntaxNode {
        id: this_id,
        kind_id: ts_node.kind_id(),
        structural_hash: 0,
        byte_range: ts_node.byte_range(),
        delimiters: [None, None],
        hint: None,
        descendant_count: 0,
        depth: parent.map_or(0, |id| nodes[id.index()].depth + 1),
        parent,
    });

    let mut hasher = std::hash::DefaultHasher::new();
    ts_node.kind_id().hash(&mut hasher);

    let (delimiters, _) = detect_node_delimiters(ts_node, source, &mut hasher);
    let mut descendant_count = 0;

    let mut children = collect_child_nodes(ts_node, source, &delimiters, options);
    if let Some(sort_children) = options.sort_children {
        sort_children(ts_node, source, &mut children);
    }

    for child in children {
        let child_id = build_tree_recursive(
            child,
            nodes,
            Some(this_id),
            source,
            excluded_node_types,
            options,
        );

        if let Some(child_id) = child_id {
            let child_node = &nodes[child_id.index()];
            descendant_count += child_node.descendant_count + 1;
            child_node.structural_hash.hash(&mut hasher);
        }
    }

    let hint = infer_leaf_hint(descendant_count, ts_node, source, &delimiters, &mut hasher);

    let node = &mut nodes[this_id.index()];
    node.structural_hash = hasher.finish();
    node.delimiters = delimiters;
    node.descendant_count = descendant_count;
    node.hint = hint;

    Some(this_id)
}

fn collect_child_nodes<'tree>(
    ts_node: tree_sitter::Node<'tree>,
    source: &str,
    delimiters: &Delimiters<'_>,
    options: BuildTreeOptions,
) -> Vec<tree_sitter::Node<'tree>> {
    let child_count = ts_node.child_count();
    let skip_first = usize::from(delimiters[0].is_some());
    let skip_last = usize::from(delimiters[1].is_some());

    let end = child_count.saturating_sub(skip_last);
    let mut children = Vec::with_capacity(end.saturating_sub(skip_first));

    for index in skip_first..end {
        let Some(child) = ts_node.child(u32::try_from(index).expect("child index overflow")) else {
            continue;
        };

        if options
            .skip_node
            .is_some_and(|skip_node| skip_node(child, source))
        {
            continue;
        }

        children.push(child);
    }

    children
}

fn detect_node_delimiters<'a>(
    ts_node: tree_sitter::Node<'_>,
    source: &'a str,
    hasher: &mut std::hash::DefaultHasher,
) -> (Delimiters<'a>, usize) {
    let mut delimiters = [None, None];
    let mut remaining_children = ts_node.child_count();

    if remaining_children >= 2
        && let (Some(first_child), Some(last_child)) = (
            ts_node.child(0),
            ts_node.child(
                (remaining_children - 1)
                    .try_into()
                    .expect("child index overflow"),
            ),
        )
        && first_child.start_byte() == ts_node.start_byte()
        && last_child.end_byte() == ts_node.end_byte()
        && let Some((open, close)) = detect_delimiters(first_child, last_child, source)
    {
        open.hash(hasher);
        close.hash(hasher);
        delimiters[0] = Some((first_child.byte_range(), open));
        delimiters[1] = Some((last_child.byte_range(), close));
        remaining_children -= 2;
    }

    (delimiters, remaining_children)
}

fn infer_leaf_hint<'a>(
    descendant_count: usize,
    ts_node: tree_sitter::Node<'_>,
    source: &'a str,
    delimiters: &Delimiters<'a>,
    hasher: &mut std::hash::DefaultHasher,
) -> Option<SyntaxHint<'a>> {
    if descendant_count != 0 {
        return None;
    }

    let text = source.get(
        delimited_content_range(ts_node.byte_range(), delimiters)
            .filter(|range| range.start < range.end)
            .unwrap_or_else(|| ts_node.byte_range()),
    )?;

    text.hash(hasher);

    if text == "," || text == ";" || text == "." {
        Some(SyntaxHint::Punctuation)
    } else if ts_node.kind().contains("string") {
        Some(SyntaxHint::String)
    } else if ts_node.is_extra() {
        Some(SyntaxHint::Comment(text))
    } else {
        None
    }
}

fn detect_delimiters<'a>(
    first_child: tree_sitter::Node<'_>,
    last_child: tree_sitter::Node<'_>,
    source: &'a str,
) -> Option<(&'a str, &'a str)> {
    if first_child.child_count() != 0 || last_child.child_count() != 0 {
        return None;
    }

    let is_delimiter = |delimiter: &str| {
        !delimiter.is_empty()
            && delimiter.len() <= 2
            && !delimiter.chars().any(char::is_alphanumeric)
    };

    let open = source.get(first_child.byte_range())?;
    let close = source.get(last_child.byte_range())?;

    if !is_delimiter(open) || !is_delimiter(close) {
        return None;
    }

    Some((open, close))
}

fn delimited_content_range(
    byte_range: Range<usize>,
    delimiters: &Delimiters<'_>,
) -> Option<Range<usize>> {
    let open = delimiters[0].as_ref()?.0.clone();
    let close = delimiters[1].as_ref()?.0.clone();
    let start = open.end.max(byte_range.start);
    let end = close.start.min(byte_range.end);
    Some(start..end)
}
