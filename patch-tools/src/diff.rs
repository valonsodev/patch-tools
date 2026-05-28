use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;

use crate::output::style;
use crate::syndiff::{
    diff_smali_snippets, diff_xml_snippets, is_smali_line_directive, is_smali_structural_only_line,
    smali_lines_are_label_rename,
};
use quick_xml::Reader;
use quick_xml::events::{BytesStart, Event};
use similar::{ChangeTag, TextDiff};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Markdown,
    Human,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SegmentMergeMode {
    Standard,
    Xml,
}

#[derive(Debug, Clone)]
struct RenderedLine {
    text: String,
    active: bool,
}

#[derive(Debug, Clone)]
struct SemanticLine {
    line: String,
    segments: Vec<Range<usize>>,
    tag: ChangeTag,
    old_line_number: Option<usize>,
    new_line_number: Option<usize>,
    active: bool,
}

#[derive(Debug, Clone, Copy)]
struct SemanticRenderOptions {
    context_lines: usize,
    mode: RenderMode,
    show_line_numbers: bool,
    merge_mode: SegmentMergeMode,
    group_separator: Option<&'static str>,
    /// Lone unmatched Delete/Insert lines for which this returns `true` are
    /// force-highlighted instead of rendered as silent plain text (Smali
    /// `:cond_0`, `.line 42`).
    structural_only: Option<fn(&str) -> bool>,
    /// Delete+Insert pairs within one change block for which this returns
    /// `true` are silenced as rebuild noise — every `:cond_NN` / `.line N`
    /// shifts when the patch inserts instructions earlier in the method.
    noise_pair: Option<fn(&str, &str) -> bool>,
    /// Lines for which this returns `true` are dropped from the diff
    /// entirely (Smali `.line` debug directives).
    suppress_line: Option<fn(&str) -> bool>,
}

pub fn render_diff(
    original: &str,
    modified: &str,
    context_lines: usize,
    mode: RenderMode,
) -> String {
    render_line_diff_with_separator(original, modified, context_lines, mode, false, None)
}

fn render_line_diff_with_separator(
    original: &str,
    modified: &str,
    context_lines: usize,
    mode: RenderMode,
    show_line_numbers: bool,
    group_separator: Option<&str>,
) -> String {
    let diff = TextDiff::from_lines(original, modified);
    let mut out = String::new();
    let width = line_number_width(original, modified);

    for (index, hunk) in diff
        .unified_diff()
        .context_radius(context_lines)
        .iter_hunks()
        .enumerate()
    {
        if index > 0
            && let Some(separator) = group_separator
        {
            writeln!(out, "{separator}").expect("writing to a String cannot fail");
        }

        for change in hunk.iter_changes() {
            let line = trim_line_terminator(change.value());
            let text = match (mode, change.tag()) {
                (RenderMode::Markdown, ChangeTag::Delete) => format!("[-{line}-]"),
                (RenderMode::Markdown, ChangeTag::Insert) => format!("{{+{line}+}}"),
                (RenderMode::Markdown, ChangeTag::Equal) => format!(" {line}"),
                (RenderMode::Human, ChangeTag::Delete) => style::diff_del(&format!("- {line}")),
                (RenderMode::Human, ChangeTag::Insert) => style::diff_add(&format!("+ {line}")),
                (RenderMode::Human, ChangeTag::Equal) => format!("  {line}"),
            };
            let text = maybe_prefix_line_numbers(
                text,
                change.old_index().map(|index| index + 1),
                change.new_index().map(|index| index + 1),
                width,
                show_line_numbers,
            );
            writeln!(out, "{text}").expect("writing to a String cannot fail");
        }
    }

    out
}

pub fn render_smali_diff(
    original: &str,
    modified: &str,
    context_lines: usize,
    mode: RenderMode,
) -> String {
    match diff_smali_snippets(original, modified, None) {
        Some((original_ranges, modified_ranges)) => render_semantic_diff(
            original,
            modified,
            &original_ranges,
            &modified_ranges,
            SemanticRenderOptions {
                context_lines,
                mode,
                show_line_numbers: true,
                merge_mode: SegmentMergeMode::Standard,
                group_separator: None,
                structural_only: Some(is_smali_structural_only_line),
                noise_pair: Some(smali_lines_are_label_rename),
                suppress_line: Some(is_smali_line_directive),
            },
        ),
        None => render_line_diff_with_separator(original, modified, context_lines, mode, true, None),
    }
}

pub fn render_xml_diff(
    original: &str,
    modified: &str,
    _context_lines: usize,
    mode: RenderMode,
) -> String {
    match normalized_xml_pair(original, modified) {
        Some((original, modified)) => render_xml_diff_inner(&original, &modified, mode),
        None => render_xml_diff_inner(original, modified, mode),
    }
}

fn render_xml_diff_inner(original: &str, modified: &str, mode: RenderMode) -> String {
    let context_lines = 0;

    match diff_xml_snippets(original, modified, None) {
        Some((original_ranges, modified_ranges)) => {
            let semantic = render_semantic_diff(
                original,
                modified,
                &original_ranges,
                &modified_ranges,
                SemanticRenderOptions {
                    context_lines,
                    mode,
                    show_line_numbers: false,
                    merge_mode: SegmentMergeMode::Xml,
                    group_separator: Some("..."),
                    structural_only: None,
                    noise_pair: None,
                    suppress_line: None,
                },
            );
            let line = render_line_diff_with_separator(
                original,
                modified,
                context_lines,
                mode,
                false,
                Some("..."),
            );
            select_xml_rendering(semantic, line)
        }
        None => render_line_diff_with_separator(
            original,
            modified,
            context_lines,
            mode,
            false,
            Some("..."),
        ),
    }
}

pub fn is_xml_path(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("xml"))
}

fn normalized_xml_pair(original: &str, modified: &str) -> Option<(String, String)> {
    Some((
        normalize_xml_for_diff(original)?,
        normalize_xml_for_diff(modified)?,
    ))
}

fn select_xml_rendering(semantic: String, line: String) -> String {
    if semantic.is_empty() || line.is_empty() {
        return semantic;
    }

    let semantic_has_insertions = semantic.contains("{+");
    let line_insertions = line.lines().filter(|line| line.contains("{+")).count();

    if !semantic_has_insertions && line_insertions > 1 {
        return line;
    }

    semantic
}

#[derive(Debug)]
enum CanonicalXmlNode {
    Declaration(Vec<(String, String)>),
    Element {
        name: String,
        attrs: Vec<(String, String)>,
        children: Vec<CanonicalXmlNode>,
    },
    Text(String),
    Comment(String),
    Raw(String),
}

#[derive(Debug)]
struct OpenXmlElement {
    name: String,
    attrs: Vec<(String, String)>,
    children: Vec<CanonicalXmlNode>,
}

fn normalize_xml_for_diff(source: &str) -> Option<String> {
    let nodes = parse_xml_nodes(source)?;
    let mut out = String::new();

    for node in &nodes {
        render_canonical_xml_node(node, 0, &mut out);
    }

    if !out.ends_with('\n') {
        out.push('\n');
    }

    Some(out)
}

fn parse_xml_nodes(source: &str) -> Option<Vec<CanonicalXmlNode>> {
    let mut reader = Reader::from_str(source);
    let mut roots = Vec::new();
    let mut stack: Vec<OpenXmlElement> = Vec::new();

    loop {
        let keep_reading = handle_xml_event(reader.read_event().ok()?, &mut roots, &mut stack)?;
        if !keep_reading {
            break;
        }
    }

    if !stack.is_empty() {
        return None;
    }

    Some(roots)
}

fn handle_xml_event(
    event: Event<'_>,
    roots: &mut Vec<CanonicalXmlNode>,
    stack: &mut Vec<OpenXmlElement>,
) -> Option<bool> {
    match event {
        Event::Decl(decl) => {
            push_xml_node(
                roots,
                stack,
                CanonicalXmlNode::Declaration(parse_xml_decl_attrs(&decl)),
            );
        }
        Event::Start(tag) => {
            stack.push(OpenXmlElement {
                name: xml_name(tag.name().as_ref())?,
                attrs: parse_xml_tag_attrs(&tag)?,
                children: Vec::new(),
            });
        }
        Event::Empty(tag) => {
            push_xml_node(
                roots,
                stack,
                CanonicalXmlNode::Element {
                    name: xml_name(tag.name().as_ref())?,
                    attrs: parse_xml_tag_attrs(&tag)?,
                    children: Vec::new(),
                },
            );
        }
        Event::End(tag) => close_xml_element(tag.name().as_ref(), roots, stack)?,
        Event::Text(text) => {
            let text = String::from_utf8_lossy(text.as_ref());
            push_xml_text(roots, stack, text.as_ref());
        }
        Event::CData(text) => {
            let text = String::from_utf8_lossy(text.as_ref());
            push_xml_text(roots, stack, text.as_ref());
        }
        Event::Comment(comment) => {
            push_xml_node(
                roots,
                stack,
                CanonicalXmlNode::Comment(
                    String::from_utf8_lossy(comment.as_ref()).trim().to_owned(),
                ),
            );
        }
        Event::DocType(doc_type) => {
            push_xml_raw(
                roots,
                stack,
                format!(
                    "<!DOCTYPE {}>",
                    String::from_utf8_lossy(doc_type.as_ref()).trim()
                ),
            );
        }
        Event::PI(instruction) => {
            push_xml_raw(
                roots,
                stack,
                format!(
                    "<?{}?>",
                    String::from_utf8_lossy(instruction.as_ref()).trim()
                ),
            );
        }
        Event::GeneralRef(reference) => {
            let text = format!("&{};", String::from_utf8_lossy(reference.as_ref()));
            push_xml_text(roots, stack, &text);
        }
        Event::Eof => return Some(false),
    }

    Some(true)
}

fn parse_xml_decl_attrs(decl: &quick_xml::events::BytesDecl<'_>) -> Vec<(String, String)> {
    let mut attrs = Vec::new();
    if let Ok(version) = decl.version() {
        attrs.push((
            "version".to_owned(),
            String::from_utf8_lossy(&version).into_owned(),
        ));
    }
    if let Some(Ok(encoding)) = decl.encoding() {
        attrs.push((
            "encoding".to_owned(),
            String::from_utf8_lossy(&encoding).into_owned(),
        ));
    }
    if let Some(Ok(standalone)) = decl.standalone() {
        attrs.push((
            "standalone".to_owned(),
            String::from_utf8_lossy(&standalone).into_owned(),
        ));
    }
    attrs
}

fn close_xml_element(
    name: &[u8],
    roots: &mut Vec<CanonicalXmlNode>,
    stack: &mut Vec<OpenXmlElement>,
) -> Option<()> {
    let name = xml_name(name)?;
    let element = stack.pop()?;
    if element.name != name {
        return None;
    }
    push_xml_node(
        roots,
        stack,
        CanonicalXmlNode::Element {
            name: element.name,
            attrs: element.attrs,
            children: element.children,
        },
    );
    Some(())
}

fn xml_name(raw: &[u8]) -> Option<String> {
    Some(std::str::from_utf8(raw).ok()?.to_owned())
}

fn parse_xml_tag_attrs(tag: &BytesStart<'_>) -> Option<Vec<(String, String)>> {
    let mut attrs = Vec::new();

    for attr in tag.attributes().with_checks(false) {
        let attr = attr.ok()?;
        attrs.push((
            xml_name(attr.key.as_ref())?,
            String::from_utf8_lossy(attr.value.as_ref()).into_owned(),
        ));
    }

    attrs.sort_by(|lhs, rhs| lhs.0.cmp(&rhs.0));
    Some(attrs)
}

fn push_xml_node(
    roots: &mut Vec<CanonicalXmlNode>,
    stack: &mut [OpenXmlElement],
    node: CanonicalXmlNode,
) {
    if let Some(parent) = stack.last_mut() {
        parent.children.push(node);
    } else {
        roots.push(node);
    }
}

fn push_xml_text(roots: &mut Vec<CanonicalXmlNode>, stack: &mut [OpenXmlElement], text: &str) {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return;
    }
    push_xml_node(roots, stack, CanonicalXmlNode::Text(trimmed.to_owned()));
}

fn push_xml_raw(roots: &mut Vec<CanonicalXmlNode>, stack: &mut [OpenXmlElement], text: String) {
    if text.trim().is_empty() {
        return;
    }
    push_xml_node(roots, stack, CanonicalXmlNode::Raw(text));
}

fn render_canonical_xml_node(node: &CanonicalXmlNode, depth: usize, out: &mut String) {
    let indent = "  ".repeat(depth);

    match node {
        CanonicalXmlNode::Declaration(attrs) => {
            out.push_str("<?xml");
            for (name, value) in attrs {
                out.push(' ');
                out.push_str(name);
                out.push_str("=\"");
                out.push_str(value);
                out.push('"');
            }
            out.push_str("?>\n");
        }
        CanonicalXmlNode::Element {
            name,
            attrs,
            children,
        } => {
            let attrs = render_xml_attrs(attrs);
            if children.is_empty() {
                out.push_str(&indent);
                out.push('<');
                out.push_str(name);
                out.push_str(&attrs);
                out.push_str(" />\n");
                return;
            }

            if let [CanonicalXmlNode::Text(text)] = children.as_slice() {
                out.push_str(&indent);
                out.push('<');
                out.push_str(name);
                out.push_str(&attrs);
                out.push('>');
                out.push_str(text);
                out.push_str("</");
                out.push_str(name);
                out.push_str(">\n");
                return;
            }

            out.push_str(&indent);
            out.push('<');
            out.push_str(name);
            out.push_str(&attrs);
            out.push_str(">\n");

            for child in children {
                render_canonical_xml_node(child, depth + 1, out);
            }

            out.push_str(&indent);
            out.push_str("</");
            out.push_str(name);
            out.push_str(">\n");
        }
        CanonicalXmlNode::Text(text) => {
            out.push_str(&indent);
            out.push_str(text);
            out.push('\n');
        }
        CanonicalXmlNode::Comment(text) => {
            out.push_str(&indent);
            out.push_str("<!--");
            out.push_str(text);
            out.push_str("-->\n");
        }
        CanonicalXmlNode::Raw(text) => {
            out.push_str(&indent);
            out.push_str(text.trim());
            out.push('\n');
        }
    }
}

fn render_xml_attrs(attrs: &[(String, String)]) -> String {
    let mut out = String::new();

    for (name, value) in attrs {
        out.push(' ');
        out.push_str(name);
        out.push_str("=\"");
        out.push_str(value);
        out.push('"');
    }

    out
}

fn render_semantic_diff(
    original: &str,
    modified: &str,
    original_ranges: &[Range<usize>],
    modified_ranges: &[Range<usize>],
    options: SemanticRenderOptions,
) -> String {
    if original_ranges.is_empty() && modified_ranges.is_empty() {
        return String::new();
    }

    let diff = TextDiff::from_lines(original, modified);
    let original_line_spans = line_spans(original);
    let modified_line_spans = line_spans(modified);
    let width = line_number_width(original, modified);

    let mut semantic_lines: Vec<_> = diff
        .iter_all_changes()
        .map(|change| {
            let line = trim_line_terminator(change.value());
            match change.tag() {
                ChangeTag::Delete => {
                    let line_idx = change
                        .old_index()
                        .expect("deleted line should have an old index");
                    let segments = meaningful_line_segments(
                        line,
                        intersect_line_ranges(&original_line_spans[line_idx], original_ranges),
                        options.merge_mode,
                    );
                    SemanticLine {
                        line: line.to_owned(),
                        segments: segments.clone(),
                        tag: ChangeTag::Delete,
                        old_line_number: Some(line_idx + 1),
                        new_line_number: None,
                        active: !segments.is_empty(),
                    }
                }
                ChangeTag::Insert => {
                    let line_idx = change
                        .new_index()
                        .expect("inserted line should have a new index");
                    let segments = meaningful_line_segments(
                        line,
                        intersect_line_ranges(&modified_line_spans[line_idx], modified_ranges),
                        options.merge_mode,
                    );
                    SemanticLine {
                        line: line.to_owned(),
                        segments: segments.clone(),
                        tag: ChangeTag::Insert,
                        old_line_number: None,
                        new_line_number: Some(line_idx + 1),
                        active: !segments.is_empty(),
                    }
                }
                ChangeTag::Equal => SemanticLine {
                    line: line.to_owned(),
                    segments: Vec::new(),
                    tag: ChangeTag::Equal,
                    old_line_number: change.old_index().map(|index| index + 1),
                    new_line_number: change.new_index().map(|index| index + 1),
                    active: false,
                },
            }
        })
        .collect();

    apply_structural_fixup(
        &mut semantic_lines,
        options.structural_only,
        options.noise_pair,
    );

    if let Some(suppress_line) = options.suppress_line {
        semantic_lines.retain(|line| !suppress_line(&line.line));
    }

    let rendered_lines = collapse_semantic_replacements(
        &semantic_lines,
        options.mode,
        width,
        options.show_line_numbers,
        options.merge_mode,
        options.structural_only,
    );

    let groups = active_change_groups(&rendered_lines, options.context_lines);
    let mut out = String::new();

    for (index, group) in groups.into_iter().enumerate() {
        if index > 0
            && let Some(separator) = options.group_separator
        {
            writeln!(out, "{separator}").expect("writing to a String cannot fail");
        }

        for line in &rendered_lines[group] {
            writeln!(out, "{}", line.text).expect("writing to a String cannot fail");
        }
    }

    out
}

/// Silence Delete+Insert pairs that `noise_pair` flags as rebuild noise
/// (`similar` splits any line difference into Delete+Insert, so a patch that
/// shifts bytecode offsets floods the diff with `:cond_NN`/`.line N` renames).
/// As a fallback, promote a lone structural-only Delete or Insert into a
/// highlighted whole-line segment so it doesn't read as silent plain text
/// in the middle of a Delete cluster (Busuu-style branch deletions).
fn apply_structural_fixup(
    semantic_lines: &mut [SemanticLine],
    is_structural: Option<fn(&str) -> bool>,
    noise_pair: Option<fn(&str, &str) -> bool>,
) {
    let mut paired = vec![false; semantic_lines.len()];

    if let Some(noise_pair) = noise_pair {
        let mut index = 0;
        while index < semantic_lines.len() {
            if semantic_lines[index].tag == ChangeTag::Equal {
                index += 1;
                continue;
            }

            let start = index;
            while index < semantic_lines.len() && semantic_lines[index].tag != ChangeTag::Equal {
                index += 1;
            }
            let end = index;

            for delete_idx in start..end {
                if semantic_lines[delete_idx].tag != ChangeTag::Delete || paired[delete_idx] {
                    continue;
                }
                for insert_idx in start..end {
                    if semantic_lines[insert_idx].tag != ChangeTag::Insert || paired[insert_idx] {
                        continue;
                    }
                    if noise_pair(
                        &semantic_lines[delete_idx].line,
                        &semantic_lines[insert_idx].line,
                    ) {
                        paired[delete_idx] = true;
                        paired[insert_idx] = true;
                        break;
                    }
                }
            }
        }
    }

    for (index, line) in semantic_lines.iter_mut().enumerate() {
        if !matches!(line.tag, ChangeTag::Delete | ChangeTag::Insert) {
            continue;
        }
        if paired[index] {
            line.segments.clear();
            line.active = false;
            continue;
        }
        let Some(is_structural) = is_structural else {
            continue;
        };
        if !is_structural(&line.line) {
            continue;
        }
        if line.segments.is_empty() {
            let leading = line
                .line
                .bytes()
                .position(|byte| byte != b' ' && byte != b'\t')
                .unwrap_or(line.line.len());
            if leading < line.line.len() {
                line.segments.push(leading..line.line.len());
                line.active = true;
            }
        }
    }
}

fn collapse_semantic_replacements(
    lines: &[SemanticLine],
    mode: RenderMode,
    width: usize,
    show_line_numbers: bool,
    merge_mode: SegmentMergeMode,
    structural_only: Option<fn(&str) -> bool>,
) -> Vec<RenderedLine> {
    let mut rendered = Vec::with_capacity(lines.len());
    let mut index = 0;

    let _ = merge_mode;

    while index < lines.len() {
        if lines[index].tag != ChangeTag::Equal {
            let start = index;
            while index < lines.len() && lines[index].tag != ChangeTag::Equal {
                index += 1;
            }
            rendered.extend(collapse_replacement_block(
                &lines[start..index],
                mode,
                width,
                show_line_numbers,
                structural_only,
            ));
            continue;
        }

        rendered.push(render_semantic_line(
            &lines[index],
            mode,
            width,
            show_line_numbers,
        ));
        index += 1;
    }

    rendered
}

/// Pair Delete/Insert lines within a non-Equal block by best inline match
/// rather than by adjacency, so a `const-string v0, "[-free-]{+standard+}"`
/// rename pairs inline even when `similar`'s change stream interleaves
/// structural-only lines or XML attribute insertions between the two sides.
fn collapse_replacement_block(
    lines: &[SemanticLine],
    mode: RenderMode,
    width: usize,
    show_line_numbers: bool,
    structural_only: Option<fn(&str) -> bool>,
) -> Vec<RenderedLine> {
    let delete_indices: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.tag == ChangeTag::Delete).then_some(index))
        .collect();
    let insert_indices: Vec<_> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| (line.tag == ChangeTag::Insert).then_some(index))
        .collect();

    if delete_indices.is_empty() || insert_indices.is_empty() {
        return lines
            .iter()
            .map(|line| render_semantic_line(line, mode, width, show_line_numbers))
            .collect();
    }

    let mut paired_deletes = vec![None; lines.len()];
    let mut paired_inserts = vec![false; lines.len()];

    for delete_index in delete_indices {
        let Some((insert_index, line)) = insert_indices
            .iter()
            .copied()
            .filter(|insert_index| !paired_inserts[*insert_index])
            .filter(|insert_index| {
                can_cross_pair(lines, delete_index, *insert_index, structural_only)
            })
            .find_map(|insert_index| {
                combine_semantic_replacement(
                    &lines[delete_index],
                    &lines[insert_index],
                    mode,
                    width,
                    show_line_numbers,
                )
                .map(|line| (insert_index, line))
            })
        else {
            continue;
        };

        paired_deletes[delete_index] = Some(line);
        paired_inserts[insert_index] = true;
    }

    lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| {
            if let Some(rendered) = paired_deletes[index].clone() {
                return Some(rendered);
            }
            if line.tag == ChangeTag::Insert && paired_inserts[index] {
                return None;
            }
            Some(render_semantic_line(line, mode, width, show_line_numbers))
        })
        .collect()
}

/// In Smali mode, every line strictly between the Delete and Insert must
/// be structural-only — otherwise `return-object v0` at the top of an old
/// method pairs inline with `return-object v1` at the bottom of a rewritten
/// one, jumbling the line-number column (Merriam-Webster fixture). XML mode
/// has no classifier and allows any intermediate content.
fn can_cross_pair(
    lines: &[SemanticLine],
    delete_index: usize,
    insert_index: usize,
    structural_only: Option<fn(&str) -> bool>,
) -> bool {
    let Some(is_structural) = structural_only else {
        return true;
    };
    let (low, high) = if delete_index < insert_index {
        (delete_index, insert_index)
    } else {
        (insert_index, delete_index)
    };
    lines[low + 1..high]
        .iter()
        .all(|line| is_structural(&line.line))
}

fn render_semantic_line(
    line: &SemanticLine,
    mode: RenderMode,
    width: usize,
    show_line_numbers: bool,
) -> RenderedLine {
    let text = match line.tag {
        ChangeTag::Equal => render_plain_text(&line.line, mode),
        ChangeTag::Delete | ChangeTag::Insert => {
            highlight_line(&line.line, &line.segments, line.tag, mode)
        }
    };

    RenderedLine {
        text: maybe_prefix_line_numbers(
            text,
            line.old_line_number,
            line.new_line_number,
            width,
            show_line_numbers,
        ),
        active: line.active,
    }
}

fn combine_semantic_replacement(
    deleted: &SemanticLine,
    inserted: &SemanticLine,
    mode: RenderMode,
    width: usize,
    show_line_numbers: bool,
) -> Option<RenderedLine> {
    if deleted.tag != ChangeTag::Delete || inserted.tag != ChangeTag::Insert {
        return None;
    }

    if !inserted.active {
        return None;
    }

    // Pure addition on the inserted side: emit only the highlighted insert
    // line — the old line's content is fully reproduced as inserted context.
    if !deleted.active {
        let inserted_contexts = line_context_segments(&inserted.line, &inserted.segments);
        let joined_context: String = inserted_contexts.concat();
        let strip_ws = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
        if strip_ws(&joined_context) == strip_ws(&deleted.line) {
            let text = highlight_line(&inserted.line, &inserted.segments, ChangeTag::Insert, mode);
            return Some(RenderedLine {
                text: maybe_prefix_line_numbers(
                    text,
                    deleted.old_line_number,
                    inserted.new_line_number,
                    width,
                    show_line_numbers,
                ),
                active: true,
            });
        }
        return None;
    }

    let deleted_contexts = line_context_segments(&deleted.line, &deleted.segments);
    let inserted_contexts = line_context_segments(&inserted.line, &inserted.segments);
    if deleted_contexts != inserted_contexts {
        return None;
    }
    if !deleted_contexts
        .iter()
        .any(|context| context.chars().any(|ch| !ch.is_whitespace()))
    {
        return None;
    }

    let deleted_chunks = line_changed_segments(&deleted.line, &deleted.segments);
    let inserted_chunks = line_changed_segments(&inserted.line, &inserted.segments);
    if deleted_chunks.len() != inserted_chunks.len() {
        return None;
    }

    let mut text = String::new();

    for (index, context) in deleted_contexts.iter().enumerate() {
        text.push_str(&render_plain_text(context, mode));

        let Some((deleted_chunk, inserted_chunk)) =
            deleted_chunks.get(index).zip(inserted_chunks.get(index))
        else {
            continue;
        };

        text.push_str(&render_diff_chunk(deleted_chunk, ChangeTag::Delete, mode));
        text.push_str(&render_diff_chunk(inserted_chunk, ChangeTag::Insert, mode));
    }

    Some(RenderedLine {
        text: maybe_prefix_line_numbers(
            text,
            deleted.old_line_number,
            inserted.new_line_number,
            width,
            show_line_numbers,
        ),
        active: true,
    })
}

fn trim_line_terminator(line: &str) -> &str {
    line.trim_end_matches(['\r', '\n'])
}

fn line_spans(text: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut start = 0;

    for line in text.split_inclusive('\n') {
        let end = start + line.len();
        spans.push(start..line_content_end(start, line));
        start = end;
    }

    if !text.is_empty() && !text.ends_with('\n') {
        return spans;
    }

    spans
}

fn line_content_end(line_start: usize, line: &str) -> usize {
    let trimmed_len = if line.ends_with("\r\n") {
        line.len() - 2
    } else if line.ends_with(['\r', '\n']) {
        line.len() - 1
    } else {
        line.len()
    };

    line_start + trimmed_len
}

fn intersect_line_ranges(line_range: &Range<usize>, ranges: &[Range<usize>]) -> Vec<Range<usize>> {
    let mut intersections = Vec::new();

    for range in ranges {
        if range.end <= line_range.start {
            continue;
        }
        if range.start >= line_range.end {
            break;
        }

        intersections.push(
            (range.start.max(line_range.start) - line_range.start)
                ..(range.end.min(line_range.end) - line_range.start),
        );
    }

    intersections
}

fn meaningful_line_segments(
    line: &str,
    segments: Vec<Range<usize>>,
    merge_mode: SegmentMergeMode,
) -> Vec<Range<usize>> {
    let filtered: Vec<_> = segments
        .into_iter()
        .filter(|segment| segment.start < segment.end && !line[segment.clone()].trim().is_empty())
        .collect();

    let merged = merge_nearby_segments(line, filtered, merge_mode);
    match merge_mode {
        SegmentMergeMode::Standard => merged,
        SegmentMergeMode::Xml => expand_xml_attribute_segments(line, merged),
    }
}

fn merge_nearby_segments(
    line: &str,
    segments: Vec<Range<usize>>,
    merge_mode: SegmentMergeMode,
) -> Vec<Range<usize>> {
    let mut merged: Vec<Range<usize>> = Vec::new();

    for segment in segments {
        if let Some(last) = merged.last_mut()
            && should_merge_segment_gap(&line[last.end..segment.start], merge_mode)
        {
            last.end = segment.end;
        } else {
            merged.push(segment);
        }
    }

    merged
}

fn should_merge_segment_gap(gap: &str, merge_mode: SegmentMergeMode) -> bool {
    match merge_mode {
        SegmentMergeMode::Standard => gap.chars().all(|ch| ch == ' ' || ch == '\t'),
        SegmentMergeMode::Xml => gap
            .chars()
            .all(|ch| ch.is_whitespace() || matches!(ch, '=' | '"' | '\'')),
    }
}

fn expand_xml_attribute_segments(line: &str, segments: Vec<Range<usize>>) -> Vec<Range<usize>> {
    let bytes = line.as_bytes();

    segments
        .into_iter()
        .map(|mut segment| {
            let segment_text = &line[segment.clone()];
            let has_unclosed_quote = |quote| {
                segment_text.bytes().fold(
                    false,
                    |unclosed, byte| if byte == quote { !unclosed } else { unclosed },
                )
            };

            if segment_text.contains('=')
                && let Some(quote @ (b'\'' | b'"')) = bytes.get(segment.end).copied()
                && has_unclosed_quote(quote)
            {
                segment.end += 1;
            }

            segment
        })
        .collect()
}

fn highlight_line(
    line: &str,
    segments: &[Range<usize>],
    tag: ChangeTag,
    mode: RenderMode,
) -> String {
    if segments.is_empty() {
        return render_plain_text(line, mode);
    }

    let mut out = String::new();
    let mut cursor = 0;

    for segment in segments {
        out.push_str(&render_plain_text(&line[cursor..segment.start], mode));

        let highlighted = &line[segment.clone()];
        out.push_str(&render_diff_chunk(highlighted, tag, mode));

        cursor = segment.end;
    }

    out.push_str(&render_plain_text(&line[cursor..], mode));
    out
}

fn render_diff_chunk(text: &str, tag: ChangeTag, mode: RenderMode) -> String {
    match (mode, tag) {
        (RenderMode::Markdown, ChangeTag::Delete) => format!("[-{text}-]"),
        (RenderMode::Markdown, ChangeTag::Insert) => format!("{{+{text}+}}"),
        (_, ChangeTag::Delete) => style::diff_del(text),
        (_, ChangeTag::Insert) => style::diff_add(text),
        (_, ChangeTag::Equal) => text.to_owned(),
    }
}

fn line_context_segments<'a>(line: &'a str, segments: &[Range<usize>]) -> Vec<&'a str> {
    let mut contexts = Vec::with_capacity(segments.len() + 1);
    let mut cursor = 0;

    for segment in segments {
        contexts.push(&line[cursor..segment.start]);
        cursor = segment.end;
    }

    contexts.push(&line[cursor..]);
    contexts
}

fn line_changed_segments<'a>(line: &'a str, segments: &[Range<usize>]) -> Vec<&'a str> {
    segments
        .iter()
        .map(|segment| &line[segment.clone()])
        .collect()
}

fn active_change_groups(lines: &[RenderedLine], context_lines: usize) -> Vec<Range<usize>> {
    let mut groups: Vec<Range<usize>> = Vec::new();

    for (idx, _) in lines.iter().enumerate().filter(|(_, line)| line.active) {
        let start = idx.saturating_sub(context_lines);
        let end = (idx + context_lines + 1).min(lines.len());

        if let Some(last) = groups.last_mut()
            && start <= last.end
        {
            last.end = last.end.max(end);
        } else {
            groups.push(start..end);
        }
    }

    groups
}

fn render_plain_text(text: &str, mode: RenderMode) -> String {
    let _ = mode;
    text.to_owned()
}

fn line_number_width(original: &str, modified: &str) -> usize {
    line_spans(original)
        .len()
        .max(line_spans(modified).len())
        .max(1)
        .to_string()
        .len()
}

fn maybe_prefix_line_numbers(
    text: String,
    _old_line_number: Option<usize>,
    new_line_number: Option<usize>,
    width: usize,
    show_line_numbers: bool,
) -> String {
    if !show_line_numbers {
        return text;
    }

    let new = new_line_number.map_or_else(|| " ".repeat(width), |line| format!("{line:>width$}"));

    format!("{new} | {text}")
}

#[cfg(test)]
mod tests {
    use super::{RenderMode, render_xml_diff};

    #[test]
    fn xml_diff_pairs_repeated_attribute_replacements() {
        let original = r#"<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
  <application>
    <activity android:name="org.acra.dialog.CrashReportDialog" />
    <service android:enabled="@bool/acra_enable_legacy_service" android:name="org.acra.sender.LegacySenderService" />
    <service android:enabled="@bool/acra_enable_job_service" android:name="org.acra.sender.JobSenderService" />
    <provider android:name="org.acra.attachment.AcraContentProvider" />
  </application>
</manifest>
"#;
        let modified = r#"<?xml version="1.0" encoding="utf-8" standalone="no"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android">
  <application>
    <activity android:enabled="false" android:name="org.acra.dialog.CrashReportDialog" />
    <service android:enabled="false" android:name="org.acra.sender.LegacySenderService" />
    <service android:enabled="false" android:name="org.acra.sender.JobSenderService" />
    <provider android:enabled="false" android:name="org.acra.attachment.AcraContentProvider" />
  </application>
</manifest>
"#;

        let diff = render_xml_diff(original, modified, 3, RenderMode::Markdown);

        assert!(
            diff.contains(
                r#"<service android:enabled="[-@bool/acra_enable_legacy_service-]{+false+}" android:name="org.acra.sender.LegacySenderService" />"#
            ),
            "{diff}"
        );
        assert!(
            diff.contains(
                r#"<service android:enabled="[-@bool/acra_enable_job_service-]{+false+}" android:name="org.acra.sender.JobSenderService" />"#
            ),
            "{diff}"
        );
        assert_eq!(
            line_count_containing(&diff, "LegacySenderService"),
            1,
            "{diff}"
        );
        assert_eq!(
            line_count_containing(&diff, "JobSenderService"),
            1,
            "{diff}"
        );
        assert_eq!(
            line_count_containing(&diff, "CrashReportDialog"),
            1,
            "{diff}"
        );
        assert_eq!(
            line_count_containing(&diff, "AcraContentProvider"),
            1,
            "{diff}"
        );
        assert_eq!(line_count_containing(&diff, "<application>"), 0, "{diff}");
        assert_eq!(line_count_containing(&diff, "</application>"), 0, "{diff}");
        assert_eq!(line_count_containing(&diff, "</manifest>"), 0, "{diff}");
        assert_eq!(line_count_equal(&diff, "..."), 1, "{diff}");
    }

    #[test]
    fn smali_pairs_across_structural_lines() {
        // Busuu shape: structural-only `:cond_0` sits between the
        // `const-string v0, "free"` Delete and the `const-string v0,
        // "standard"` Insert. Should pair inline despite the intervening
        // line.
        let before = r#".method public getTier()Ljava/lang/String;
    .registers 3
    iget-object v0, p0, Lcom/busuu/Account;->tier:Ljava/lang/String;
    if-nez v0, :cond_0
    const-string v0, "free"
    :cond_0
    return-object v0
.end method"#;
        let after = r#".method public getTier()Ljava/lang/String;
    .registers 3
    const-string v0, "standard"
    return-object v0
.end method"#;
        let md = super::render_smali_diff(before, after, 3, RenderMode::Markdown);
        assert!(
            md.contains(r#"const-string v0, "[-free-]{+standard+}""#),
            "expected inline const-string pairing across :cond_0, got:\n{md}"
        );
    }

    #[test]
    fn smali_does_not_pair_across_unrelated_content() {
        // Merriam-Webster shape: `return-object v0` at the top of the old
        // body should NOT pair with `return-object v1` at the bottom of a
        // much-longer rewritten body, because the seven inserts between
        // them aren't structural anchors. Without this guard the line-
        // number column jumped from 10 down to 3 and back up to 11.
        let before = r".method public getSubscription()Lcom/merriamwebster/Subscription;
    .registers 4
    iget-object v0, p0, Lcom/merriamwebster/Account;->sub:Lcom/merriamwebster/Subscription;
    return-object v0
.end method";
        let after = r#".method public getSubscription()Lcom/merriamwebster/Subscription;
    .registers 4
    new-instance v0, Ljava/util/Date;
    const-wide v1, 0x7fffffffffffffffL
    invoke-direct {v0, v1, v2}, Ljava/util/Date;-><init>(J)V
    new-instance v1, Lcom/merriamwebster/Subscription;
    const-string v2, ""
    const/4 v3, 0x1
    invoke-direct {v1, v2, v0, v2, v3}, Lcom/merriamwebster/Subscription;-><init>(Ljava/lang/String;Ljava/util/Date;Ljava/lang/String;Z)V
    return-object v1
.end method"#;
        let md = super::render_smali_diff(before, after, 3, RenderMode::Markdown);

        // The old bug: line-number column jumped 10 → 3 → 4 → … → 11 because
        // the paired Delete was rendered at the Delete's position. Assert
        // RHS line numbers (left of `|`) appear monotonically.
        let rhs_numbers: Vec<usize> = md
            .lines()
            .filter_map(|line| {
                let (prefix, _) = line.split_once('|')?;
                prefix.trim().parse().ok()
            })
            .collect();
        assert!(
            rhs_numbers.windows(2).all(|w| w[0] < w[1]),
            "RHS line numbers should be monotonically increasing, got {rhs_numbers:?} in:\n{md}"
        );

        // Sanity: both return-object lines appear, neither paired inline.
        assert!(
            md.contains("return-object [-v0-]"),
            "expected deleted `v0` highlighted on the old return-object line, got:\n{md}"
        );
        assert!(
            md.contains("return-object {+v1+}"),
            "expected inserted `v1` highlighted on the new return-object line, got:\n{md}"
        );
    }

    #[test]
    fn smali_offset_shift_noise_is_collapsed_to_real_changes() {
        // Real-world shape from rootbeer / navionics: a patch inserts two
        // instructions at the top of a method, which shifts every
        // bytecode offset in the rest of the body. `:cond_NN`,
        // `:try_end_NN`, `.line N`, and instructions like
        // `if-nez v0, :cond_NN` / `.catch … {:try_start_NN … :try_end_NN}
        // :catch_NN` all rename in lockstep. The diff should display ONLY
        // the inserted instructions; all the rename pairs should silence.
        let before = r".method public g()Z
    .registers 1
    :try_start_0
    invoke-static {}, Lit/navionics/nativelib/auth/SSOAuthController$LOGIN_STATUS;->getValue()I
    move-result v0
    :try_end_e
    .catch Ljava/lang/Exception; {:try_start_0 .. :try_end_e} :catch_12
    if-eqz v0, :cond_12
    return v0
    :catch_12
    :cond_12
    const/4 v0, 0x0
    return v0
.end method";
        let after = r".method public g()Z
    .registers 1
    const/4 v0, 0x1
    return v0
    :try_start_2
    invoke-static {}, Lit/navionics/nativelib/auth/SSOAuthController$LOGIN_STATUS;->getValue()I
    move-result v0
    :try_end_10
    .catch Ljava/lang/Exception; {:try_start_2 .. :try_end_10} :catch_14
    if-eqz v0, :cond_14
    return v0
    :catch_14
    :cond_14
    const/4 v0, 0x0
    return v0
.end method";
        let md = super::render_smali_diff(before, after, 3, RenderMode::Markdown);

        // The only "real" change is the two new instructions at the top.
        // Every other line is rebuild noise (labels and offset numbers
        // shifted, but the actual instructions, registers, and types are
        // unchanged).
        assert!(
            md.contains("{+    const/4 v0, 0x1+}"),
            "expected the genuine insertion to be highlighted, got:\n{md}"
        );
        assert!(
            md.contains("{+    return v0+}"),
            "expected the genuine insertion to be highlighted, got:\n{md}"
        );

        // No `[-…-]` Deletes anywhere — every Delete in this diff is a
        // rename pair and should have been silenced.
        assert!(
            !md.contains("[-"),
            "expected zero Delete markers (all changes are noise pairs), got:\n{md}"
        );

        // The only `{+…+}` Inserts should be the two genuine new lines.
        // Anything else (a `{+:try_start_2+}`, `{+.catch …+}` etc.) means
        // the rename pair wasn't recognized.
        let insert_count = md.matches("{+").count();
        assert_eq!(
            insert_count, 2,
            "expected exactly 2 inserts (const/4 + return v0), got {insert_count} in:\n{md}"
        );
    }

    #[test]
    fn smali_label_rename_is_suppressed() {
        // Pure label rename (and `.line` renumbering) is pure noise — the
        // assembler regenerates these on every rebuild. The diff should
        // collapse to nothing.
        let before = r#".method public foo()V
    .registers 2
    .line 5
    if-nez v0, :cond_0
    const-string v0, "x"
    :cond_0
    return-void
.end method"#;
        let after = r#".method public foo()V
    .registers 2
    .line 42
    if-nez v0, :cond_99
    const-string v0, "x"
    :cond_99
    return-void
.end method"#;
        let md = super::render_smali_diff(before, after, 3, RenderMode::Markdown);
        assert!(
            !md.contains("[-") && !md.contains("{+"),
            "expected no diff markers for pure label/line rename, got:\n{md}"
        );
    }

    #[test]
    fn smali_unpaired_label_in_structural_delete_is_highlighted() {
        // The standalone `:cond_0` line in a wholly-deleted branch should
        // render as a marked deletion, not as silent context.
        let before = r#".method public getTier()Ljava/lang/String;
    .registers 3
    iget-object v0, p0, Lcom/busuu/Account;->tier:Ljava/lang/String;
    if-nez v0, :cond_0
    const-string v0, "free"
    :cond_0
    return-object v0
.end method"#;
        let after = r#".method public getTier()Ljava/lang/String;
    .registers 3
    const-string v0, "standard"
    return-object v0
.end method"#;
        let md = super::render_smali_diff(before, after, 3, RenderMode::Markdown);
        assert!(
            md.contains("[-:cond_0-]"),
            "expected standalone :cond_0 to be highlighted, got:\n{md}"
        );
        assert!(
            md.contains("[-    if-nez v0, :cond_0-]"),
            "expected if-nez line including :cond_0 reference to highlight as a whole, got:\n{md}"
        );
    }

    fn line_count_containing(text: &str, needle: &str) -> usize {
        text.lines().filter(|line| line.contains(needle)).count()
    }

    fn line_count_equal(text: &str, needle: &str) -> usize {
        text.lines().filter(|line| *line == needle).count()
    }
}
