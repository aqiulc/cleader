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
                    if !spans.is_empty() {
                        out.push(Block::Paragraph { spans });
                    }
                }
                // Other tags handled in later tasks; for now descend.
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
        if ch.is_whitespace() {
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
}
