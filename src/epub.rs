use std::path::PathBuf;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn span_helpers_produce_expected_styles() {
        assert_eq!(Span::plain("a").style, SpanStyle::Plain);
        assert_eq!(Span::bold("a").style, SpanStyle::Bold);
        assert_eq!(Span::italic("a").style, SpanStyle::Italic);
    }
}
