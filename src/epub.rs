use std::path::PathBuf;

use scraper::{ElementRef, Html, Node};

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

#[derive(Debug, Clone)]
pub struct Chapter {
    pub title: Option<String>,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone)]
pub struct Book {
    pub title: String,
    pub author: String,
    pub path: PathBuf,
    pub chapters: Vec<Chapter>,
}

/// Convert a chapter's XHTML body into Vec<Block>.
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
                    let spans = collect_spans(&child_el, SpanStyle::Plain);
                    if spans.is_empty() {
                        out.push(Block::Blank);
                    } else {
                        out.push(Block::Paragraph { spans });
                    }
                }
                "h1" | "h2" | "h3" | "h4" | "h5" | "h6" => {
                    // Match arm guarantees tag is "h1".."h6", all 2-byte ASCII; byte 1 is the digit.
                    let level = tag.as_bytes()[1] - b'0';
                    let spans = collect_spans(&child_el, SpanStyle::Plain);
                    // Empty heading: drop (no spacer convention for headings, unlike <p>).
                    if !spans.is_empty() {
                        out.push(Block::Heading { level, spans });
                        out.push(Block::Blank);
                    }
                }
                _ => walk_block_level(&child_el, out),
            }
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
            Node::Element(_) => {
                if let Some(child_el) = ElementRef::wrap(child) {
                    spans.extend(collect_spans(&child_el, current_style));
                }
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
    out.trim().to_string()
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
    fn heading_with_inline_em_is_currently_flat_plain() {
        // Pre-Task-11 / Pre-Task-14 behavior pin. Two things will change in
        // future tasks; this test fails deliberately when either lands:
        //   (1) <em> inside a heading is currently rendered as a plain span;
        //       Task 11 will switch to SpanStyle::Italic.
        //   (2) Adjacent text nodes lose their separating space because
        //       collapse_whitespace trims each text node individually
        //       ("Part " + "One" -> "Part" + "One" -> "PartOne"). This is a
        //       known cross-span-space-loss bug to be fixed alongside Task 11
        //       (when collect_spans is restructured) or in Task 14's wrap
        //       layer if it survives that long.
        let blocks = html_to_blocks(
            "<html><body><h1>Part <em>One</em></h1></body></html>",
        );
        match &blocks[0] {
            Block::Heading { spans, .. } => {
                assert!(
                    spans.iter().all(|s| s.style == SpanStyle::Plain),
                    "pre-Task-11: <em> not yet styled"
                );
                let joined: String = spans.iter().map(|s| s.text.as_str()).collect();
                assert_eq!(
                    joined, "PartOne",
                    "pre-fix: cross-span space is lost; Task 11 or Task 14 should fix"
                );
            }
            _ => panic!("expected heading"),
        }
    }
}
