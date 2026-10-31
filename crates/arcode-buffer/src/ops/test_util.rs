//! Test helpers: `|` marks a cursor, `«…»` a selection (anchor at `«`,
//! head at `»`), `»…«` a reversed selection.

use crate::buffer::Buffer;
use crate::selection::{Selection, SelectionSet};

/// Parses marked text into a buffer and selections (primary = last).
pub(crate) fn setup(marked: &str) -> (Buffer, SelectionSet) {
    let mut text = String::new();
    let mut sels = Vec::new();
    let mut open: Option<(char, usize)> = None;
    let mut pos = 0;
    for c in marked.chars() {
        match c {
            '|' => sels.push(Selection::cursor(pos)),
            '«' | '»' => match open.take() {
                None => open = Some((c, pos)),
                Some((first, at)) => {
                    let sel = if first == '«' {
                        Selection::new(at, pos)
                    } else {
                        Selection::new(pos, at)
                    };
                    sels.push(sel);
                }
            },
            _ => {
                text.push(c);
                pos += 1;
            }
        }
    }
    let n = sels.len();
    let set = SelectionSet::from_vec(sels, n.saturating_sub(1)).expect("markers");
    (Buffer::new(&text), set)
}

/// Renders text with selection markers.
pub(crate) fn show(buffer: &Buffer, sels: &SelectionSet) -> String {
    let text: Vec<char> = buffer.to_string().chars().collect();
    let mut out = String::new();
    for i in 0..=text.len() {
        for s in sels.iter() {
            if s.is_empty() && s.head == i {
                out.push('|');
            } else if !s.is_empty() && s.end() == i {
                out.push(if s.is_reversed() { '«' } else { '»' });
            }
        }
        for s in sels.iter() {
            if !s.is_empty() && s.start() == i {
                out.push(if s.is_reversed() { '»' } else { '«' });
            }
        }
        if let Some(c) = text.get(i) {
            out.push(*c);
        }
    }
    out
}

#[test]
fn round_trip_markers() {
    for s in ["a|b|c", "«ab»c", "x»ab«", "|", "«a»«b»"] {
        let (b, sels) = setup(s);
        assert_eq!(show(&b, &sels), s);
    }
}
