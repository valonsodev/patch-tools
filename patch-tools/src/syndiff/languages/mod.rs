use std::ops::Range;

use super::syntax_tree::SyntaxTree;

pub(crate) mod smali;
pub(crate) mod xml;

#[derive(Debug, Clone, Copy)]
pub(crate) enum Language {
    Smali,
    Xml,
}

pub(crate) fn parse(lang: Language, source: &str) -> Option<SyntaxTree<'_>> {
    match lang {
        Language::Smali => smali::parse(source),
        Language::Xml => xml::parse(source),
    }
}

pub(crate) fn post_filter(
    lang: Language,
    source: &str,
    ranges: Vec<Range<usize>>,
) -> Vec<Range<usize>> {
    match lang {
        Language::Smali => smali::post_filter(source, ranges),
        Language::Xml => ranges,
    }
}
