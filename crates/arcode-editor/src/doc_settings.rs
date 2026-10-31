//! Per-document settings the status bar changes: language, line ending and
//! indentation.

use arcode_buffer::LineEnding;
use arcode_buffer::ops::IndentUnit;
use arcode_syntax::Language;
use gpui::{App, Context};

use crate::Editor;

/// Indentation for one document, overriding `tab_size` / `insert_spaces`
/// of its language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndentOverride {
    /// What Tab inserts and what one indent level is.
    pub unit: IndentUnit,
    /// Display width of a tab character (1-16).
    pub tab_width: usize,
}

impl IndentOverride {
    /// `n` spaces per level (tabs shown `n` wide).
    pub fn spaces(n: usize) -> Self {
        let n = n.clamp(1, 16);
        Self {
            unit: IndentUnit::Spaces(n),
            tab_width: n,
        }
    }

    /// Tab characters shown `width` wide.
    pub fn tabs(width: usize) -> Self {
        Self {
            unit: IndentUnit::Tab,
            tab_width: width.clamp(1, 16),
        }
    }

    /// Whether Tab inserts spaces.
    pub fn uses_spaces(&self) -> bool {
        matches!(self.unit, IndentUnit::Spaces(_))
    }

    /// Columns per indent level.
    pub fn size(&self) -> usize {
        self.unit.width(self.tab_width)
    }
}

impl Editor {
    /// Switches the document's language (e.g. from the language picker or
    /// a file association): highlighting is redone. Language servers are
    /// not told; the caller re-registers the editor with them.
    pub fn set_language(&mut self, language: &'static Language, cx: &mut Context<Self>) {
        if std::ptr::eq(language, self.language) {
            return;
        }
        self.language = language;
        self.syntax = None;
        self.syntax_task = None;
        self.pending_syntax_edits.clear();
        self.highlights = None;
        self.init_syntax(cx);
        cx.notify();
    }

    /// The line ending written when saving.
    pub fn line_ending(&self) -> LineEnding {
        self.buffer.line_ending()
    }

    /// Changes the line ending used when saving. The text is unchanged;
    /// the document becomes dirty until saved (or switched back).
    pub fn set_line_ending(&mut self, line_ending: LineEnding, cx: &mut Context<Self>) {
        if self.read_only || self.buffer.line_ending() == line_ending {
            return;
        }
        self.buffer.set_line_ending(line_ending);
        self.check_dirty(cx);
        cx.notify();
    }

    /// The indentation set for this document, if any.
    pub fn indent_override(&self) -> Option<IndentOverride> {
        self.indent_override
    }

    /// Sets (or with `None` clears) this document's indentation.
    pub fn set_indent_override(&mut self, indent: Option<IndentOverride>, cx: &mut Context<Self>) {
        if self.indent_override != indent {
            self.indent_override = indent;
            cx.notify();
        }
    }

    /// The indentation in effect: the override, else the language's
    /// settings.
    pub fn indentation(&self, cx: &App) -> IndentOverride {
        let cfg = self.ops_config(cx);
        IndentOverride {
            unit: cfg.indent,
            tab_width: cfg.tab_width,
        }
    }
}
