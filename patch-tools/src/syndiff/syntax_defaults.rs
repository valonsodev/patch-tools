//! Default implementations of the [`AtomClassifier`] and [`DelimiterDetector`]
//! hooks on [`BuildTreeOptions`]. These encode C-family conventions
//! (`,`/`;`/`.` punctuation, single-character non-alphanumeric delimiters) and
//! are *opt-in* — the core build pipeline calls whatever hook the options
//! carry, so a language that doesn't follow these conventions can override
//! them via [`BuildTreeOptions::with_classify_atom`] /
//! [`BuildTreeOptions::with_detect_delimiters`].
//!
//! [`AtomClassifier`]: super::syntax_tree::AtomClassifier
//! [`DelimiterDetector`]: super::syntax_tree::DelimiterDetector
//! [`BuildTreeOptions`]: super::syntax_tree::BuildTreeOptions
//! [`BuildTreeOptions::with_classify_atom`]: super::syntax_tree::BuildTreeOptions::with_classify_atom
//! [`BuildTreeOptions::with_detect_delimiters`]: super::syntax_tree::BuildTreeOptions::with_detect_delimiters

use super::syntax_tree::SyntaxHint;

/// Default atom classifier: flags Western-style punctuation (`,`, `;`, `.`)
/// and tree-sitter `extra` nodes (comments).
pub(crate) fn default_classify_atom<'a>(
    ts_node: tree_sitter::Node<'_>,
    text: &'a str,
) -> Option<SyntaxHint<'a>> {
    if text == "," || text == ";" || text == "." {
        Some(SyntaxHint::Punctuation)
    } else if ts_node.is_extra() {
        Some(SyntaxHint::Comment(text))
    } else {
        None
    }
}

/// Default delimiter detector: a node's first and last children look like
/// delimiters if both are single tokens (no children of their own), at most
/// 2 bytes long, and contain no alphanumeric characters — i.e. `{`/`}`,
/// `(`/`)`, `[`/`]`, `<`/`>`, etc. Languages with multi-character or keyword
/// delimiters (Smali `.method`/`.end method`, XML `<tag>`/`</tag>`) get
/// `None` from this and the node is treated as a flat list.
pub(crate) fn default_detect_delimiters<'a>(
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
