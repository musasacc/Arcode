//! Test harness: `"text with |cursor"` notation, `«` `»` around selections.
//!
//! `|` marks a cursor (an empty selection; in Normal mode the block cursor
//! sits on the char after `|`). `«...»` marks a non-empty selection as the
//! editor sees it (half-open range). Several markers make several cursors.
//! `[` and `]` are not used as markers so bracket text objects can be
//! tested.

#![allow(dead_code)]

use arcode_buffer::{Buffer, Selection, SelectionSet};
use arcode_vim::{IndentUnit, Key, Mode, Vim, VimAction, VimContext};

/// The context used by all tests.
pub fn ctx() -> VimContext {
    VimContext {
        tab_size: 4,
        indent: IndentUnit::Spaces(4),
        viewport_top: 0,
        viewport_lines: 10,
        line_comment: Some("//".to_string()),
    }
}

/// Parses marked text into plain text and selections.
pub fn parse(marked: &str) -> (String, SelectionSet) {
    let mut text = String::new();
    let mut sels = Vec::new();
    let mut open: Option<usize> = None;
    let mut n = 0;
    for c in marked.chars() {
        match c {
            '|' => sels.push(Selection::cursor(n)),
            '«' => open = Some(n),
            '»' => {
                let start = open.take().expect("» without «");
                sels.push(Selection::new(start, n));
            }
            _ => {
                text.push(c);
                n += 1;
            }
        }
    }
    let set = SelectionSet::from_vec(sels, 0).unwrap_or_default();
    (text, set)
}

/// Renders text with selection markers.
pub fn render(buffer: &Buffer, sels: &SelectionSet) -> String {
    let text: Vec<char> = buffer.to_string().chars().collect();
    let mut out = String::new();
    for i in 0..=text.len() {
        for s in sels.iter() {
            if s.is_empty() && s.head == i {
                out.push('|');
            } else if !s.is_empty() && s.end() == i {
                out.push('»');
            }
        }
        for s in sels.iter() {
            if !s.is_empty() && s.start() == i {
                out.push('«');
            }
        }
        if let Some(c) = text.get(i) {
            out.push(*c);
        }
    }
    out
}

/// A Vim session over one buffer.
pub struct Session {
    pub vim: Vim,
    pub buffer: Buffer,
    pub sels: SelectionSet,
    pub actions: Vec<VimAction>,
}

impl Session {
    pub fn new(marked: &str) -> Self {
        let (text, sels) = parse(marked);
        Session {
            vim: Vim::new(),
            buffer: Buffer::new(&text),
            sels,
            actions: Vec::new(),
        }
    }

    /// Sends keys in Vim notation; collects non-`None` actions.
    pub fn keys(&mut self, keys: &str) -> &mut Self {
        let ctx = ctx();
        for k in Key::parse_keys(keys) {
            let a = self
                .vim
                .handle_key(&k, &mut self.buffer, &mut self.sels, &ctx);
            if a != VimAction::None {
                self.actions.push(a);
            }
        }
        self
    }

    pub fn state(&self) -> String {
        render(&self.buffer, &self.sels)
    }

    pub fn mode(&self) -> Mode {
        self.vim.mode()
    }

    pub fn reg(&self, name: char) -> Option<String> {
        self.vim.register(name).map(|r| r.text())
    }

    pub fn last_action(&self) -> Option<&VimAction> {
        self.actions.last()
    }
}

/// Runs `keys` on `initial` and compares with `expected`.
pub fn assert_vim(initial: &str, keys: &str, expected: &str) {
    let mut s = Session::new(initial);
    s.keys(keys);
    assert_eq!(
        s.state(),
        expected,
        "\n  initial: {initial:?}\n  keys:    {keys:?}"
    );
}

/// Checks a table of `(initial, keys, expected)` cases, reporting every
/// failure at once. Returns the number of cases.
pub fn check_table(cases: &[(&str, &str, &str)]) -> usize {
    let mut failures = Vec::new();
    for (initial, keys, expected) in cases {
        let mut s = Session::new(initial);
        s.keys(keys);
        let got = s.state();
        if got != *expected {
            failures.push(format!(
                "  {initial:?} + {keys:?}\n    expected {expected:?}\n    got      {got:?}"
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} cases failed:\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n")
    );
    cases.len()
}
