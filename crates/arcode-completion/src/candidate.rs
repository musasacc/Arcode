//! The [`CompletionCandidate`] model shared by every completion source.

use std::ops::Range;

/// What a candidate is; mirrors LSP `CompletionItemKind` plus a few local kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default)]
pub enum CandidateKind {
    /// Plain text.
    #[default]
    Text,
    /// A method.
    Method,
    /// A free function.
    Function,
    /// A constructor.
    Constructor,
    /// A struct/class field.
    Field,
    /// A variable or binding.
    Variable,
    /// A class.
    Class,
    /// An interface or trait.
    Interface,
    /// A module or namespace.
    Module,
    /// A property.
    Property,
    /// A unit (e.g. CSS unit).
    Unit,
    /// A literal value.
    Value,
    /// An enum.
    Enum,
    /// A language keyword.
    Keyword,
    /// A snippet.
    Snippet,
    /// A color.
    Color,
    /// A file.
    File,
    /// A reference.
    Reference,
    /// A folder.
    Folder,
    /// An enum variant.
    EnumMember,
    /// A constant.
    Constant,
    /// A struct.
    Struct,
    /// An event.
    Event,
    /// An operator.
    Operator,
    /// A type parameter.
    TypeParameter,
    /// A type (alias or otherwise unspecified type).
    Type,
    /// A word collected from the buffer.
    Word,
}

impl CandidateKind {
    /// Maps an LSP `CompletionItemKind` number (1-based) to a kind.
    /// Unknown numbers become [`CandidateKind::Text`].
    pub fn from_lsp(kind: u32) -> Self {
        use CandidateKind::*;
        const TABLE: [CandidateKind; 25] = [
            Text,
            Method,
            Function,
            Constructor,
            Field,
            Variable,
            Class,
            Interface,
            Module,
            Property,
            Unit,
            Value,
            Enum,
            Keyword,
            Snippet,
            Color,
            File,
            Reference,
            Folder,
            EnumMember,
            Constant,
            Struct,
            Event,
            Operator,
            TypeParameter,
        ];
        kind.checked_sub(1)
            .and_then(|i| TABLE.get(i as usize).copied())
            .unwrap_or(Text)
    }
}

/// Where a candidate came from.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CandidateSource {
    /// A language server, identified by its name.
    Lsp {
        /// Server name as configured.
        server: String,
    },
    /// A word found in the open buffer(s).
    BufferWord,
    /// A file system entry.
    Path,
    /// A built-in or user snippet.
    Snippet,
}

impl CandidateSource {
    /// Ranking priority; higher wins (LSP > Snippet > BufferWord > Path).
    pub fn priority(&self) -> u8 {
        match self {
            CandidateSource::Lsp { .. } => 3,
            CandidateSource::Snippet => 2,
            CandidateSource::BufferWord => 1,
            CandidateSource::Path => 0,
        }
    }

    /// Whether this is a language server source.
    pub fn is_lsp(&self) -> bool {
        matches!(self, CandidateSource::Lsp { .. })
    }
}

/// What accepting a candidate inserts.
///
/// LSP `textEdit`s with line/character ranges are applied by the app; this
/// type only keeps the text and whether it is snippet syntax.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InsertAction {
    /// Insert this text verbatim (replacing the typed word).
    PlainText(String),
    /// Expand this snippet (LSP/TextMate syntax).
    Snippet(String),
    /// Replace an explicit buffer char range.
    ReplaceRange {
        /// Char range in the buffer to replace.
        range: Range<usize>,
        /// Replacement text.
        text: String,
        /// Whether `text` is snippet syntax.
        is_snippet: bool,
    },
}

impl InsertAction {
    /// The text to insert (snippet syntax if [`InsertAction::is_snippet`]).
    pub fn text(&self) -> &str {
        match self {
            InsertAction::PlainText(t) | InsertAction::Snippet(t) => t,
            InsertAction::ReplaceRange { text, .. } => text,
        }
    }

    /// Whether the text must be expanded as a snippet.
    pub fn is_snippet(&self) -> bool {
        match self {
            InsertAction::PlainText(_) => false,
            InsertAction::Snippet(_) => true,
            InsertAction::ReplaceRange { is_snippet, .. } => *is_snippet,
        }
    }

    /// The explicit replace range, if any.
    pub fn range(&self) -> Option<Range<usize>> {
        match self {
            InsertAction::ReplaceRange { range, .. } => Some(range.clone()),
            _ => None,
        }
    }
}

/// One completion item from any source.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CompletionCandidate {
    /// Text shown in the list.
    pub label: String,
    /// Text matched against the query (defaults to `label`).
    pub filter_text: Option<String>,
    /// Text used for ordering among LSP items (defaults to `label`).
    pub sort_text: Option<String>,
    /// Kind (icon).
    pub kind: CandidateKind,
    /// Origin.
    pub source: CandidateSource,
    /// Secondary text (signature, path, description).
    pub detail: Option<String>,
    /// What accepting inserts.
    pub insert: InsertAction,
    /// Whether the server asked to preselect this item.
    pub preselect: bool,
    /// Opaque id for the caller (e.g. index into its LSP item list).
    pub data: u64,
}

impl CompletionCandidate {
    /// A candidate inserting `label` verbatim, with no detail.
    pub fn new(label: impl Into<String>, kind: CandidateKind, source: CandidateSource) -> Self {
        let label = label.into();
        CompletionCandidate {
            insert: InsertAction::PlainText(label.clone()),
            label,
            filter_text: None,
            sort_text: None,
            kind,
            source,
            detail: None,
            preselect: false,
            data: 0,
        }
    }

    /// The text matched against the query.
    pub fn filter_text(&self) -> &str {
        self.filter_text.as_deref().unwrap_or(&self.label)
    }

    /// The text used for LSP ordering.
    pub fn sort_text(&self) -> &str {
        self.sort_text.as_deref().unwrap_or(&self.label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsp_kind_mapping() {
        assert_eq!(CandidateKind::from_lsp(1), CandidateKind::Text);
        assert_eq!(CandidateKind::from_lsp(3), CandidateKind::Function);
        assert_eq!(CandidateKind::from_lsp(25), CandidateKind::TypeParameter);
        assert_eq!(CandidateKind::from_lsp(0), CandidateKind::Text);
        assert_eq!(CandidateKind::from_lsp(99), CandidateKind::Text);
    }

    #[test]
    fn insert_action_accessors() {
        let a = InsertAction::ReplaceRange {
            range: 1..3,
            text: "x$0".into(),
            is_snippet: true,
        };
        assert_eq!(a.text(), "x$0");
        assert!(a.is_snippet());
        assert_eq!(a.range(), Some(1..3));
        assert!(!InsertAction::PlainText("a".into()).is_snippet());
    }

    #[test]
    fn defaults_fall_back_to_label() {
        let c = CompletionCandidate::new("foo", CandidateKind::Word, CandidateSource::BufferWord);
        assert_eq!(c.filter_text(), "foo");
        assert_eq!(c.sort_text(), "foo");
        assert_eq!(c.insert.text(), "foo");
    }
}
