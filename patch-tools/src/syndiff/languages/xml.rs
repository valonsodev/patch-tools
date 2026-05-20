use super::super::syntax_tree::{BuildTreeOptions, SyntaxTree, build_tree};

pub(crate) fn parse(source: &str) -> Option<SyntaxTree<'_>> {
    let mut parser = tree_sitter::Parser::new();
    let language = tree_sitter_xml::LANGUAGE_XML.into();
    parser.set_language(&language).ok()?;

    let tree = parser.parse(source, None)?;
    if tree.root_node().has_error() {
        return None;
    }

    Some(build_tree(
        &tree.walk(),
        source,
        BuildTreeOptions::default()
            .with_skip_node(skip_insignificant_node)
            .with_sort_children(sort_children),
    ))
}

fn skip_insignificant_node(node: tree_sitter::Node<'_>, source: &str) -> bool {
    node.kind() == "CharData"
        && source
            .get(node.byte_range())
            .is_some_and(|text| text.trim().is_empty())
}

fn sort_children(
    parent: tree_sitter::Node<'_>,
    source: &str,
    children: &mut Vec<tree_sitter::Node<'_>>,
) {
    match parent.kind() {
        "STag" | "EmptyElemTag" => {
            children.sort_by(|lhs, rhs| compare_tag_children(*lhs, *rhs, source));
        }
        _ => {}
    }
}

fn compare_tag_children(
    lhs: tree_sitter::Node<'_>,
    rhs: tree_sitter::Node<'_>,
    source: &str,
) -> std::cmp::Ordering {
    tag_child_sort_key(lhs, source).cmp(&tag_child_sort_key(rhs, source))
}

fn tag_child_sort_key<'a>(
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
            attribute_name(node, source).unwrap_or_default(),
            node.start_byte(),
        ),
        _ => (
            2,
            source.get(node.byte_range()).unwrap_or_default(),
            node.start_byte(),
        ),
    }
}

fn attribute_name<'a>(node: tree_sitter::Node<'_>, source: &'a str) -> Option<&'a str> {
    (0..node.child_count()).find_map(|index| {
        let child = node.child(u32::try_from(index).expect("child index overflow"))?;
        (child.kind() == "Name")
            .then(|| source.get(child.byte_range()))
            .flatten()
    })
}
