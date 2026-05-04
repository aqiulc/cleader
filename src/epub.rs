use std::collections::HashMap;
use std::path::{Path, PathBuf};

use scraper::{ElementRef, Html, Node};

use crate::error::EpubError;
use ::epub::doc::{EpubDoc, NavPoint};

/// Inline styling for a span of text.
///
/// v1 does not combine bold and italic. When the source markup nests them
/// (e.g. `<b><i>foo</i></b>`), the HTML walker keeps the outermost style
/// — the inner tag is parsed but its style is overridden. Real fiction
/// almost never relies on bold+italic; bitflags are a v2 concern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanStyle {
    Plain,
    Bold,
    Italic,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub text: String,
    pub style: SpanStyle,
}

impl Span {
    pub fn plain<S: Into<String>>(s: S) -> Self {
        Self { text: s.into(), style: SpanStyle::Plain }
    }
    pub fn bold<S: Into<String>>(s: S) -> Self {
        Self { text: s.into(), style: SpanStyle::Bold }
    }
    pub fn italic<S: Into<String>>(s: S) -> Self {
        Self { text: s.into(), style: SpanStyle::Italic }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    Heading { level: u8, spans: Vec<Span> },
    Paragraph { spans: Vec<Span> },
    Blank,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChapterKind {
    /// Counts toward the chapter numbering displayed to the reader.
    Main,
    /// Excluded from chapter numbering — typically a cover or other
    /// image-only TOC-listed page that has no real prose. Still
    /// reachable via page-by-page navigation.
    FrontMatter,
}

#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
    pub kind: ChapterKind,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub title: String,
    pub author: String,
    pub path: PathBuf,
    pub chapters: Vec<Chapter>,
}

/// Convert a chapter's XHTML body into a `Vec<Block>`.
pub fn html_to_blocks(xhtml: &str) -> Vec<Block> {
    let doc = Html::parse_document(xhtml);
    let root = doc.root_element();
    let mut out = Vec::new();
    walk_block_level(&root, &mut out);
    out
}

fn walk_block_level(el: &ElementRef, out: &mut Vec<Block>) {
    for child in el.children() {
        if let Node::Element(e) = child.value() {
            let Some(child_el) = ElementRef::wrap(child) else { continue };
            let tag = e.name();
            match tag {
                "p" => {
                    let mut spans = collect_spans(&child_el, SpanStyle::Plain);
                    trim_span_edges(&mut spans);
                    if spans.is_empty() {
                        out.push(Block::Blank);
                    } else {
                        out.push(Block::Paragraph { spans });
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    // Match arm guarantees tag is "h1".."h6", all 2-byte ASCII; byte 1 is the digit.
                    let level = tag.as_bytes()[1] - b'0';
                    let mut spans = collect_spans(&child_el, SpanStyle::Plain);
                    trim_span_edges(&mut spans);
                    // Empty heading: drop (no spacer convention for headings, unlike <p>).
                    if !spans.is_empty() {
                        out.push(Block::Heading { level, spans });
                        out.push(Block::Blank);
                    }
                }
                "blockquote" => {
                    // Italic + 4-space indent; nested children flattened to one paragraph.
                    let mut inner = collect_spans(&child_el, SpanStyle::Italic);
                    trim_span_edges(&mut inner);
                    if !inner.is_empty() {
                        let mut spans = vec![Span::plain("    ")];
                        spans.extend(inner);
                        out.push(Block::Paragraph { spans });
                    }
                }
                "ul" => emit_list(&child_el, out, ListKind::Unordered),
                "ol" => emit_list(&child_el, out, ListKind::Ordered),
                "hr" => {
                    out.push(Block::Paragraph {
                        spans: vec![Span::plain("─ ─ ─")],
                    });
                }
                "img" => {
                    let alt = child_el.value().attr("alt").unwrap_or("").trim();
                    if !alt.is_empty() {
                        out.push(Block::Paragraph {
                            spans: vec![Span::plain(format!("[image: {alt}]"))],
                        });
                    }
                }
                "table" => emit_table(&child_el, out),
                "br" => {
                    // Stand-alone <br> at block level: emit Block::Blank.
                    // Rare in real EPUBs (most use <p></p> for spacing) and
                    // consecutive <br/><br/> would produce consecutive Blanks
                    // — acceptable for v1; the renderer can collapse runs if
                    // it ever matters.
                    out.push(Block::Blank);
                }
                _ => walk_block_level(&child_el, out),
            }
        }
    }
}

/// Trim leading whitespace from the first span and trailing whitespace from
/// the last span. Inter-span whitespace is preserved.
fn trim_span_edges(spans: &mut Vec<Span>) {
    while let Some(first) = spans.first_mut() {
        let trimmed = first.text.trim_start();
        if trimmed.len() != first.text.len() {
            first.text = trimmed.to_string();
        }
        if first.text.is_empty() {
            spans.remove(0);
        } else {
            break;
        }
    }
    while let Some(last) = spans.last_mut() {
        let trimmed = last.text.trim_end();
        if trimmed.len() != last.text.len() {
            last.text = trimmed.to_string();
        }
        if last.text.is_empty() {
            spans.pop();
        } else {
            break;
        }
    }
}

#[derive(Clone, Copy)]
enum ListKind {
    Unordered,
    Ordered,
}

fn emit_list(el: &ElementRef, out: &mut Vec<Block>, kind: ListKind) {
    let mut idx = 1usize;
    for li in el.children() {
        let Node::Element(e) = li.value() else { continue };
        if e.name() != "li" { continue; }
        let Some(li_el) = ElementRef::wrap(li) else { continue };
        let mut inner = collect_spans(&li_el, SpanStyle::Plain);
        trim_span_edges(&mut inner);
        if inner.is_empty() { continue; }
        let prefix = match kind {
            ListKind::Unordered => "• ".to_string(),
            ListKind::Ordered => format!("{idx}. "),
        };
        let mut spans = vec![Span::plain(prefix)];
        spans.extend(inner);
        out.push(Block::Paragraph { spans });
        idx += 1;
    }
}

fn emit_table(el: &ElementRef, out: &mut Vec<Block>) {
    // descendants() handles the common <tbody> wrapping case. Caveat: a
    // nested <table> inside a <td> would have its rows emitted twice
    // (once as part of the outer cell's flattened text, once as their
    // own row paragraphs). v1 fiction never nests tables; v2 may want
    // to filter to direct-table descendants.
    for row in el.descendants() {
        let Node::Element(e) = row.value() else { continue };
        if e.name() != "tr" { continue; }
        let Some(tr_el) = ElementRef::wrap(row) else { continue };
        let mut cells = Vec::new();
        for cell in tr_el.children() {
            let Node::Element(ce) = cell.value() else { continue };
            if ce.name() != "td" && ce.name() != "th" { continue; }
            let Some(cell_el) = ElementRef::wrap(cell) else { continue };
            let mut cell_spans = collect_spans(&cell_el, SpanStyle::Plain);
            trim_span_edges(&mut cell_spans);
            let text: String = cell_spans
                .into_iter()
                .map(|s| s.text)
                .collect::<Vec<_>>()
                .join("");
            cells.push(text);
        }
        if !cells.is_empty() {
            out.push(Block::Paragraph {
                spans: vec![Span::plain(cells.join("  "))],
            });
        }
    }
}

fn collect_spans(el: &ElementRef, current_style: SpanStyle) -> Vec<Span> {
    let mut spans = Vec::new();
    for child in el.children() {
        match child.value() {
            Node::Text(t) => {
                let text = collapse_whitespace(t);
                if !text.is_empty() {
                    spans.push(Span { text, style: current_style });
                }
            }
            Node::Element(e) => {
                let Some(child_el) = ElementRef::wrap(child) else { continue };
                let tag = e.name();
                if tag == "br" {
                    // Inline <br>: treat as a soft space; the wrapper handles visual breaks.
                    spans.push(Span { text: " ".into(), style: current_style });
                    continue;
                }
                let next_style = match (current_style, tag) {
                    (SpanStyle::Plain, "b" | "strong") => SpanStyle::Bold,
                    (SpanStyle::Plain, "i" | "em") => SpanStyle::Italic,
                    _ => current_style,
                };
                spans.extend(collect_spans(&child_el, next_style));
            }
            _ => {}
        }
    }
    spans
}

fn collapse_whitespace(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_space = false;
    for ch in s.chars() {
        if ch.is_ascii_whitespace() {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out
}

/// Recursively collect TOC entries into a path-to-label map.
///
/// Walks NCX `navPoint` (or EPUB3 `nav doc`) trees, normalizing each
/// content path so it can be matched against the spine's resource paths.
/// The first label seen for a given path wins, which means top-level
/// labels take precedence over duplicate nested entries (rare, but it
/// happens with deeply-nested TOCs that re-link the parent's anchor).
fn collect_toc(toc: &[NavPoint], out: &mut HashMap<PathBuf, String>) {
    for point in toc {
        let path = strip_fragment(&point.content);
        out.entry(path).or_insert_with(|| point.label.clone());
        collect_toc(&point.children, out);
    }
}

/// Normalize a TOC content path so it can be matched against spine paths.
///
/// Two transformations:
/// 1. Strip any `#fragment` suffix (NCX entries may deep-link via
///    `chapter1.xhtml#section-2`; the spine never carries fragments).
/// 2. Percent-decode the remaining path. Some converters (notably
///    Calibre on default settings) emit `Chapter%201.xhtml` in the NCX
///    while the spine lists `Chapter 1.xhtml`. Without decoding the
///    chapter would silently fail to match and disappear from the
///    book's chapter list.
///
/// Order matters: strip the real `#` before decoding, so an encoded
/// `%23` in the path doesn't get mistaken for a fragment marker.
fn strip_fragment(p: &Path) -> PathBuf {
    let s = p.to_string_lossy();
    let without_fragment = match s.find('#') {
        Some(idx) => &s[..idx],
        None => &s,
    };
    PathBuf::from(percent_decode(without_fragment))
}

/// Percent-decode an ASCII path string. Unrecognized `%XX` sequences
/// (non-hex digits, truncated at end-of-string) are passed through
/// verbatim — better to over-keep than to silently drop bytes.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(hex) = hex {
                if let Ok(byte) = u8::from_str_radix(hex, 16) {
                    out.push(byte);
                    i += 3;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    // If the decoded bytes aren't valid UTF-8, fall back to the raw
    // input. NCX paths are virtually always UTF-8 in practice.
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

/// Classify a chapter as Main (real prose) or FrontMatter (image-only).
///
/// A chapter is "front matter" if its only meaningful content is a
/// single image placeholder. The cover is the canonical example.
/// FrontMatter chapters stay in `book.chapters` (so navigation can
/// reach them and a future image renderer can read them) but they
/// are excluded from the user-visible chapter numbering.
fn classify(blocks: &[Block]) -> ChapterKind {
    let mut saw_real = false;
    let mut saw_image_only = false;
    for b in blocks {
        match b {
            Block::Blank => {}
            Block::Heading { .. } => {
                saw_real = true;
            }
            Block::Paragraph { spans } => {
                let is_image = spans.len() == 1
                    && spans[0].text.starts_with("[image:");
                if is_image {
                    saw_image_only = true;
                } else {
                    saw_real = true;
                }
            }
        }
    }
    if !saw_real && saw_image_only {
        ChapterKind::FrontMatter
    } else {
        ChapterKind::Main
    }
}

impl Book {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, EpubError> {
        let path = path.as_ref();
        if !path.exists() {
            return Err(EpubError::NotFound(path.to_path_buf()));
        }

        let mut doc = EpubDoc::new(path)
            .map_err(|e| EpubError::Malformed { reason: e.to_string() })?;

        let title = doc
            .mdata("title")
            .map(|m| m.value.clone())
            .unwrap_or_else(|| {
                path.file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Untitled".into())
            });
        let author = doc
            .mdata("creator")
            .map(|m| m.value.clone())
            .unwrap_or_else(|| "Unknown".into());

        // First pass: collect every non-empty spine item as (path, blocks).
        // Silent skip on get_current_str() == None: a single corrupt spine
        // item shouldn't kill a 30-chapter book.
        let mut all: Vec<(PathBuf, Vec<Block>)> = Vec::new();
        loop {
            let current_path = doc.get_current_path();
            if let Some((content, _mime)) = doc.get_current_str() {
                let blocks = html_to_blocks(&content);
                if !blocks.is_empty() {
                    if let Some(p) = current_path {
                        all.push((p, blocks));
                    }
                }
            }
            if !doc.go_next() {
                break;
            }
        }

        // Second pass: filter via the TOC. Front-matter (cover, title page,
        // copyright, etc.) is in the spine but typically NOT referenced by
        // the TOC, so this drops it without us having to hard-code names.
        // When the TOC is empty (rare hand-rolled EPUBs lack one), keep all
        // spine items so we still load the book.
        let mut toc_labels: HashMap<PathBuf, String> = HashMap::new();
        collect_toc(&doc.toc, &mut toc_labels);

        let chapters: Vec<Chapter> = if toc_labels.is_empty() {
            all.into_iter()
                .map(|(_, blocks)| {
                    let kind = classify(&blocks);
                    Chapter { title: None, blocks, kind }
                })
                .collect()
        } else {
            all.into_iter()
                .filter_map(|(p, blocks)| {
                    toc_labels.get(&p).map(|label| {
                        let kind = classify(&blocks);
                        Chapter {
                            title: Some(label.clone()),
                            blocks,
                            kind,
                        }
                    })
                })
                .collect()
        };

        if chapters.is_empty() {
            return Err(EpubError::NoChapters);
        }

        Ok(Book {
            title,
            author,
            path: path.to_path_buf(),
            chapters,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_helpers_produce_expected_styles() {
        assert_eq!(Span::plain("a").style, SpanStyle::Plain);
        assert_eq!(Span::bold("a").style, SpanStyle::Bold);
        assert_eq!(Span::italic("a").style, SpanStyle::Italic);
    }

    #[test]
    fn strip_fragment_removes_anchor_suffix() {
        let p = strip_fragment(Path::new("chapter1.xhtml#section-2"));
        assert_eq!(p, PathBuf::from("chapter1.xhtml"));
    }

    #[test]
    fn strip_fragment_passes_through_unfragmented_paths() {
        let p = strip_fragment(Path::new("chapter1.xhtml"));
        assert_eq!(p, PathBuf::from("chapter1.xhtml"));
    }

    #[test]
    fn strip_fragment_decodes_percent_encoded_spaces() {
        // Calibre default conversion produces this pattern: NCX has
        // "Chapter%201.xhtml" while the spine carries the literal-space
        // "Chapter 1.xhtml". Without decoding the chapter is silently
        // dropped from book.chapters.
        let p = strip_fragment(Path::new("Chapter%201.xhtml"));
        assert_eq!(p, PathBuf::from("Chapter 1.xhtml"));
    }

    #[test]
    fn strip_fragment_strips_then_decodes() {
        // Order matters: an encoded %23 (the literal '#' character) in
        // the path must NOT be mistaken for a fragment marker. Strip
        // the real # first, then decode.
        let p = strip_fragment(Path::new("foo%23bar.xhtml#anchor"));
        assert_eq!(p, PathBuf::from("foo#bar.xhtml"));
    }

    #[test]
    fn percent_decode_passes_through_malformed_sequences() {
        // % followed by non-hex or truncated at EOS should be kept
        // verbatim, not silently dropped.
        assert_eq!(percent_decode("100%done"), "100%done");
        assert_eq!(percent_decode("trailing%"), "trailing%");
        assert_eq!(percent_decode("trailing%2"), "trailing%2");
    }

    #[test]
    fn percent_decode_handles_uppercase_and_lowercase_hex() {
        assert_eq!(percent_decode("a%20b"), "a b");
        assert_eq!(percent_decode("a%2Fb"), "a/b");
        assert_eq!(percent_decode("a%2fb"), "a/b");
    }

    #[test]
    fn single_paragraph_extracts_one_block() {
        let blocks = html_to_blocks("<html><body><p>hello world</p></body></html>");
        assert_eq!(blocks.len(), 1);
        match &blocks[0] {
            Block::Paragraph { spans } => {
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].text, "hello world");
                assert_eq!(spans[0].style, SpanStyle::Plain);
            }
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn empty_body_extracts_zero_blocks() {
        let blocks = html_to_blocks("<html><body></body></html>");
        assert!(blocks.is_empty());
    }

    #[test]
    fn whitespace_around_text_is_collapsed() {
        let blocks = html_to_blocks(
            "<html><body><p>  hello\n   world   </p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => assert_eq!(spans[0].text, "hello world"),
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn multiple_paragraphs_become_multiple_blocks() {
        let blocks = html_to_blocks(
            "<html><body><p>one</p><p>two</p></body></html>",
        );
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn unknown_wrapping_tags_are_descended_into() {
        let blocks = html_to_blocks(
            "<html><body><div><section><p>nested</p></section></div></body></html>",
        );
        assert_eq!(blocks.len(), 1);
    }

    #[test]
    fn nbsp_is_preserved_through_collapse() {
        let blocks = html_to_blocks(
            "<html><body><p>Mr.\u{00A0}Smith\u{00A0}lives here.</p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                let text = &spans[0].text;
                assert!(text.contains('\u{00A0}'), "NBSP must survive whitespace collapse");
                assert_eq!(text, "Mr.\u{00A0}Smith\u{00A0}lives here.");
            }
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn empty_paragraph_becomes_blank_block() {
        let blocks = html_to_blocks(
            "<html><body><p>before</p><p></p><p>after</p></body></html>",
        );
        assert_eq!(blocks.len(), 3);
        assert!(matches!(blocks[0], Block::Paragraph { .. }));
        assert!(matches!(blocks[1], Block::Blank));
        assert!(matches!(blocks[2], Block::Paragraph { .. }));
    }

    #[test]
    fn h1_extracts_heading_block_with_blank_after() {
        let blocks = html_to_blocks("<html><body><h1>Chapter One</h1></body></html>");
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::Heading { level, spans } => {
                assert_eq!(*level, 1);
                assert_eq!(spans[0].text, "Chapter One");
            }
            _ => panic!("expected heading"),
        }
        assert!(matches!(blocks[1], Block::Blank));
    }

    #[test]
    fn all_heading_levels_extract_correct_level_with_trailing_blank() {
        for n in 1..=6u8 {
            let html = format!("<html><body><h{n}>x</h{n}></body></html>");
            let blocks = html_to_blocks(&html);
            assert_eq!(blocks.len(), 2, "level {n}: expected heading + blank");
            match &blocks[0] {
                Block::Heading { level, .. } => assert_eq!(*level, n),
                _ => panic!("expected heading at level {n}"),
            }
            assert!(matches!(blocks[1], Block::Blank), "level {n}: expected trailing Blank");
        }
    }

    #[test]
    fn empty_heading_is_dropped() {
        let blocks = html_to_blocks(
            "<html><body><p>before</p><h1></h1><p>after</p></body></html>",
        );
        assert_eq!(blocks.len(), 2, "empty heading should produce no block (no Blank)");
    }

    #[test]
    fn heading_with_inline_em_renders_italic_span() {
        // Task 10 pinned the pre-Task-11 flat-plain behavior. Task 11 now
        // maps <em> to italic, so the heading has a mix of Plain and Italic
        // spans. The cross-span-space-loss bug (see comment in this test
        // before Task 11) is also addressed here as part of restructuring
        // collect_spans — text nodes no longer trim individually.
        let blocks = html_to_blocks(
            "<html><body><h1>Part <em>One</em></h1></body></html>",
        );
        match &blocks[0] {
            Block::Heading { spans, .. } => {
                let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(joined, "Part One", "inter-span space must survive");
                let italic_spans: Vec<_> = spans.iter().filter(|s| s.style == SpanStyle::Italic).collect();
                assert_eq!(italic_spans.len(), 1, "exactly one italic span (the <em>)");
                assert_eq!(italic_spans[0].text, "One");
            }
            _ => panic!("expected heading"),
        }
    }

    #[test]
    fn bold_tag_produces_bold_span_and_preserves_whitespace() {
        let blocks = html_to_blocks(
            "<html><body><p>plain <b>bold</b> plain</p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                assert_eq!(spans.len(), 3);
                assert_eq!(spans[0].style, SpanStyle::Plain);
                assert_eq!(spans[1].style, SpanStyle::Bold);
                assert_eq!(spans[1].text, "bold");
                assert_eq!(spans[2].style, SpanStyle::Plain);
                let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(joined, "plain bold plain", "inter-span whitespace must survive");
            }
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn strong_tag_is_treated_as_bold() {
        let blocks = html_to_blocks(
            "<html><body><p><strong>x</strong></p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => assert_eq!(spans[0].style, SpanStyle::Bold),
            _ => panic!("expected paragraph for <strong>"),
        }
    }

    #[test]
    fn em_and_i_tags_produce_italic() {
        for tag in ["em", "i"] {
            let html = format!("<html><body><p><{tag}>x</{tag}></p></body></html>");
            let blocks = html_to_blocks(&html);
            match &blocks[0] {
                Block::Paragraph { spans } => assert_eq!(spans[0].style, SpanStyle::Italic),
                _ => panic!("expected paragraph for tag <{tag}>"),
            }
        }
    }

    #[test]
    fn nested_emphasis_outermost_style_wins() {
        // Per SpanStyle's doc comment: outermost style wins. Inner tag is
        // parsed but its style is overridden.
        let blocks = html_to_blocks(
            "<html><body><p><b><i>foo</i></b></p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].text, "foo");
                assert_eq!(spans[0].style, SpanStyle::Bold, "outer <b> wins over inner <i>");
            }
            _ => panic!("expected paragraph"),
        }

        // Symmetric case: outer <i>, inner <b>.
        let blocks = html_to_blocks(
            "<html><body><p><i><b>bar</b></i></p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                assert_eq!(spans.len(), 1);
                assert_eq!(spans[0].style, SpanStyle::Italic, "outer <i> wins over inner <b>");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn edge_trim_preserves_inter_span_whitespace() {
        let blocks = html_to_blocks(
            "<html><body><p>  start <em>middle</em> end  </p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(joined, "start middle end");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn blockquote_renders_italic_with_indent() {
        let blocks = html_to_blocks(
            "<html><body><blockquote>quoted</blockquote></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                assert!(spans[0].text.starts_with("    "));
                assert_eq!(spans[1].style, SpanStyle::Italic);
                assert_eq!(spans[1].text, "quoted");
            }
            _ => panic!("expected paragraph"),
        }
    }

    #[test]
    fn unordered_list_uses_bullet_prefix() {
        let blocks = html_to_blocks(
            "<html><body><ul><li>a</li><li>b</li></ul></body></html>",
        );
        assert_eq!(blocks.len(), 2);
        match &blocks[0] {
            Block::Paragraph { spans } => assert!(spans[0].text.starts_with("• ")),
            _ => panic!(),
        }
    }

    #[test]
    fn ordered_list_uses_numeric_prefix() {
        let blocks = html_to_blocks(
            "<html><body><ol><li>a</li><li>b</li></ol></body></html>",
        );
        match (&blocks[0], &blocks[1]) {
            (Block::Paragraph { spans: a }, Block::Paragraph { spans: b }) => {
                assert!(a[0].text.starts_with("1. "));
                assert!(b[0].text.starts_with("2. "));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn hr_renders_as_separator_line() {
        let blocks = html_to_blocks("<html><body><hr/></body></html>");
        match &blocks[0] {
            Block::Paragraph { spans } => assert_eq!(spans[0].text, "─ ─ ─"),
            _ => panic!(),
        }
    }

    #[test]
    fn img_with_alt_renders_as_placeholder() {
        let blocks = html_to_blocks(
            r#"<html><body><img alt="ship in flight" /></body></html>"#,
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                assert_eq!(spans[0].text, "[image: ship in flight]")
            }
            _ => panic!(),
        }
    }

    #[test]
    fn img_without_alt_is_skipped() {
        let blocks = html_to_blocks(r#"<html><body><img src="x.jpg"/></body></html>"#);
        assert!(blocks.is_empty());
    }

    #[test]
    fn anchor_renders_text_only() {
        let blocks = html_to_blocks(
            r##"<html><body><p>see <a href="#x">page 7</a></p></body></html>"##,
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                let text: String = spans.iter().map(|s| s.text.as_str()).collect();
                assert!(text.contains("see") && text.contains("page 7"));
                assert!(!text.contains("#x"), "URL fragment must not leak into rendered text");
            }
            _ => panic!(),
        }
    }

    #[test]
    fn br_inside_paragraph_becomes_space() {
        let blocks = html_to_blocks(
            "<html><body><p>line1<br/>line2</p></body></html>",
        );
        match &blocks[0] {
            Block::Paragraph { spans } => {
                let combined: String = spans.iter().map(|s| s.text.clone()).collect();
                assert!(combined.contains("line1") && combined.contains("line2"));
            }
            _ => panic!(),
        }
    }

    #[test]
    fn table_renders_one_row_per_paragraph() {
        let html = "<html><body><table>\
            <tr><td>a</td><td>b</td></tr>\
            <tr><td>c</td><td>d</td></tr>\
        </table></body></html>";
        let blocks = html_to_blocks(html);
        assert_eq!(blocks.len(), 2);
    }

    #[test]
    fn table_with_tbody_wrapper_still_renders_rows() {
        // Most EPUB tooling auto-wraps <tr> in <tbody>; descendants() is
        // what makes that work. Without this test, swapping to children()
        // would silently break tbody-wrapped tables.
        let html = "<html><body><table><tbody>\
            <tr><td>x</td><td>y</td></tr>\
            <tr><td>z</td></tr>\
        </tbody></table></body></html>";
        let blocks = html_to_blocks(html);
        assert_eq!(blocks.len(), 2, "tbody wrapper must not hide rows");
    }
}
