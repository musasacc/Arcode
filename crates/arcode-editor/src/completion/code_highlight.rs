//! Syntax coloring for fenced code blocks in documentation popups (hover,
//! completion docs, signature help). The fence's language tag picks the
//! grammar (`rust`, `ts`, `py`, ...). Results are cached per thread, so a
//! popup that re-renders on every keystroke parses each block once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::rc::Rc;

use arcode_buffer::Rope;
use arcode_syntax::{HighlightSpan, Language, Syntax};
use arcode_ui_kit::theme;
use gpui::{App, SharedString, StyledText};

/// Blocks larger than this are shown uncolored.
const MAX_BYTES: usize = 64 * 1024;
/// Cached blocks per thread before the cache is cleared.
const MAX_CACHED: usize = 64;

/// Highlight spans per line of a block.
pub(crate) type BlockSpans = Rc<Vec<Vec<HighlightSpan>>>;

thread_local! {
    static CACHE: RefCell<HashMap<u64, Option<BlockSpans>>> = RefCell::new(HashMap::new());
}

/// The language a fence tag names: an LSP id (`rust`, `typescript`) or a
/// file extension (`rs`, `ts`). `None` for unknown tags and plain text.
pub(crate) fn language_for_tag(tag: &str) -> Option<&'static Language> {
    let tag = tag.trim();
    if tag.is_empty() {
        return None;
    }
    let language = arcode_syntax::language_by_id(tag)
        .unwrap_or_else(|| arcode_syntax::language_for_path(Path::new(&format!("x.{tag}"))));
    (language.id != arcode_syntax::plain_text().id).then_some(language)
}

/// Spans for each line of `text` (as split by `str::lines`), or `None`
/// when the language is unknown or has no grammar.
pub(crate) fn highlight(language: Option<&str>, text: &str) -> Option<BlockSpans> {
    let language = language_for_tag(language?)?;
    if text.len() > MAX_BYTES {
        return None;
    }
    let mut hasher = DefaultHasher::new();
    (language.id, text).hash(&mut hasher);
    let key = hasher.finish();
    if let Some(hit) = CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let rope = Rope::from_str(text);
    let spans = Syntax::new(language, &rope).map(|syntax| {
        let lines = text.lines().count();
        Rc::new(syntax.highlight_lines(&rope, 0..lines))
    });
    CACHE.with(|c| {
        let mut cache = c.borrow_mut();
        if cache.len() >= MAX_CACHED {
            cache.clear();
        }
        cache.insert(key, spans.clone());
    });
    spans
}

/// One code line colored with the theme's syntax styles.
pub(crate) fn styled_line(line: &str, spans: Option<&Vec<HighlightSpan>>, cx: &App) -> StyledText {
    let text = SharedString::from(line.to_string());
    let Some(spans) = spans else {
        return StyledText::new(text);
    };
    let theme = theme(cx);
    let highlights: Vec<_> = spans
        .iter()
        .filter(|s| s.range.end <= line.len() && line.is_char_boundary(s.range.start))
        .filter(|s| line.is_char_boundary(s.range.end))
        .map(|s| (s.range.clone(), theme.syntax(s.name)))
        .collect();
    StyledText::new(text).with_highlights(highlights)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_resolve_by_id_or_extension() {
        assert_eq!(language_for_tag("rust").map(|l| l.id), Some("rust"));
        assert_eq!(language_for_tag("rs").map(|l| l.id), Some("rust"));
        assert!(language_for_tag("").is_none());
        assert!(language_for_tag("no-such-language").is_none());
    }

    #[test]
    fn rust_code_gets_keyword_spans_per_line() {
        let spans = highlight(Some("rust"), "pub fn add(a: i32) -> i32\n\nlet x = 1;").unwrap();
        assert_eq!(spans.len(), 3);
        assert!(spans[0].iter().any(|s| s.name.starts_with("keyword")));
        assert!(spans[1].is_empty());
        // A second call is served from the cache (same allocation).
        let again = highlight(Some("rust"), "pub fn add(a: i32) -> i32\n\nlet x = 1;").unwrap();
        assert!(Rc::ptr_eq(&spans, &again));
        assert!(highlight(None, "fn x() {}").is_none());
    }
}
