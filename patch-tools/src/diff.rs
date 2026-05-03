use std::fmt::Write as _;
use std::ops::Range;
use std::path::Path;

use crate::output::style;
use crate::syndiff::{SmaliSnippetKind, diff_smali_snippets, diff_xml_snippets};
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
}

/// Render diff lines as markdown with `{+added+}` / `[-removed-]` markers.
pub fn render_markdown_diff(original: &str, modified: &str, context_lines: usize) -> String {
    render_line_diff(
        original,
        modified,
        context_lines,
        RenderMode::Markdown,
        false,
    )
}

/// Render diff lines with colors for terminal display.
pub fn render_colored_diff(original: &str, modified: &str, context_lines: usize) -> String {
    render_line_diff(original, modified, context_lines, RenderMode::Human, false)
}

fn render_line_diff(
    original: &str,
    modified: &str,
    context_lines: usize,
    mode: RenderMode,
    show_line_numbers: bool,
) -> String {
    render_line_diff_with_separator(
        original,
        modified,
        context_lines,
        mode,
        show_line_numbers,
        None,
    )
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
    kind: SmaliSnippetKind,
    mode: RenderMode,
) -> String {
    match diff_smali_snippets(original, modified, kind, None) {
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
            },
        ),
        None => render_line_diff(original, modified, context_lines, mode, true),
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

    let semantic_lines: Vec<_> = diff
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

    let rendered_lines = collapse_semantic_replacements(
        &semantic_lines,
        options.mode,
        width,
        options.show_line_numbers,
        options.merge_mode,
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

fn collapse_semantic_replacements(
    lines: &[SemanticLine],
    mode: RenderMode,
    width: usize,
    show_line_numbers: bool,
    merge_mode: SegmentMergeMode,
) -> Vec<RenderedLine> {
    let mut rendered = Vec::with_capacity(lines.len());
    let mut index = 0;

    while index < lines.len() {
        if merge_mode == SegmentMergeMode::Xml && lines[index].tag != ChangeTag::Equal {
            let start = index;
            while index < lines.len() && lines[index].tag != ChangeTag::Equal {
                index += 1;
            }
            rendered.extend(collapse_xml_replacement_block(
                &lines[start..index],
                mode,
                width,
                show_line_numbers,
            ));
            continue;
        }

        if let Some(next) = lines.get(index + 1)
            && let Some(line) =
                combine_semantic_replacement(&lines[index], next, mode, width, show_line_numbers)
        {
            rendered.push(line);
            index += 2;
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

fn collapse_xml_replacement_block(
    lines: &[SemanticLine],
    mode: RenderMode,
    width: usize,
    show_line_numbers: bool,
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

    // At least the inserted side must be active for a combined rendering.
    if !inserted.active {
        return None;
    }

    // When the deleted side has no semantic segments (pure addition on the
    // inserted side), emit only the highlighted insert line.  The old line
    // carries no unique information the combined view would lose.
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

    // Both sides are active — try the standard context-matching combination.

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

    fn line_count_containing(text: &str, needle: &str) -> usize {
        text.lines().filter(|line| line.contains(needle)).count()
    }

    fn line_count_equal(text: &str, needle: &str) -> usize {
        text.lines().filter(|line| *line == needle).count()
    }
}
