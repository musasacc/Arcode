//! Bracket matching for `()`, `[]` and `{}`.

use ropey::Rope;
use tree_sitter::Node;

use crate::Syntax;

/// How far [`matching_bracket_plain`] scans before giving up, in bytes.
pub const PLAIN_SCAN_LIMIT: usize = 1 << 20;

/// Returns the partner of `bracket` and whether `bracket` opens.
fn partner(bracket: u8) -> Option<(u8, bool)> {
    match bracket {
        b'(' => Some((b')', true)),
        b'[' => Some((b']', true)),
        b'{' => Some((b'}', true)),
        b')' => Some((b'(', false)),
        b']' => Some((b'[', false)),
        b'}' => Some((b'{', false)),
        _ => None,
    }
}

/// The single bracket byte a node kind stands for, if any.
fn bracket_kind(kind: &str) -> Option<u8> {
    match kind.as_bytes() {
        [byte] if partner(*byte).is_some() => Some(*byte),
        _ => None,
    }
}

impl Syntax {
    /// Finds the bracket matching the one starting at `byte_offset`.
    ///
    /// Only bracket *tokens* in the tree count, so brackets inside strings
    /// and comments are ignored. Returns the partner's byte offset, or `None`
    /// if there is no bracket at `byte_offset` or it is unmatched.
    ///
    /// `text` must be the text this tree was last parsed from.
    pub fn matching_bracket(&self, text: &Rope, byte_offset: usize) -> Option<usize> {
        if byte_offset >= text.len_bytes() {
            return None;
        }
        let root = self.tree().root_node();
        let node = root.descendant_for_byte_range(byte_offset, byte_offset + 1)?;
        if node.start_byte() != byte_offset || node.is_named() || node.is_missing() {
            return None;
        }
        let bracket = bracket_kind(node.kind())?;
        // The tree may be stale; never trust it without checking the text.
        if text.get_byte(byte_offset) != Some(bracket) {
            return None;
        }
        let (target, opens) = partner(bracket)?;
        let parent = node.parent()?;
        find_partner_among_siblings(node, parent, bracket, target, opens)
    }
}

/// Scans the siblings of `node` (children of `parent`) for its partner.
fn find_partner_among_siblings(
    node: Node,
    parent: Node,
    bracket: u8,
    target: u8,
    opens: bool,
) -> Option<usize> {
    let mut cursor = parent.walk();
    let siblings: Vec<Node> = parent.children(&mut cursor).collect();
    let index = siblings.iter().position(|sibling| *sibling == node)?;
    let mut depth = 0usize;
    let mut check = |sibling: &Node| -> Option<usize> {
        if sibling.is_named() || sibling.is_missing() {
            return None;
        }
        let kind = bracket_kind(sibling.kind())?;
        if kind == bracket {
            depth += 1;
        } else if kind == target {
            if depth == 0 {
                return Some(sibling.start_byte());
            }
            depth -= 1;
        }
        None
    };
    if opens {
        siblings[index + 1..].iter().find_map(&mut check)
    } else {
        siblings[..index].iter().rev().find_map(&mut check)
    }
}

/// Text-only bracket matching for languages without a grammar.
///
/// Counts nesting of the same bracket pair; strings and comments are not
/// understood, so brackets inside them count too.
/// Scans at most [`PLAIN_SCAN_LIMIT`] bytes.
pub fn matching_bracket_plain(text: &Rope, byte_offset: usize) -> Option<usize> {
    let bracket = text.get_byte(byte_offset)?;
    let (target, opens) = partner(bracket)?;
    let mut depth = 0usize;
    let mut step = |byte: u8| -> bool {
        if byte == bracket {
            depth += 1;
        } else if byte == target {
            if depth == 0 {
                return true;
            }
            depth -= 1;
        }
        false
    };
    if opens {
        let mut bytes = text.bytes_at(byte_offset + 1);
        let mut offset = byte_offset + 1;
        while offset - byte_offset <= PLAIN_SCAN_LIMIT {
            if step(bytes.next()?) {
                return Some(offset);
            }
            offset += 1;
        }
    } else {
        let mut bytes = text.bytes_at(byte_offset);
        let mut offset = byte_offset;
        while byte_offset - offset < PLAIN_SCAN_LIMIT {
            let byte = bytes.prev()?;
            offset -= 1;
            if step(byte) {
                return Some(offset);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_matches_nested() {
        let text = Rope::from_str("a(b(c)d)e");
        assert_eq!(matching_bracket_plain(&text, 1), Some(7));
        assert_eq!(matching_bracket_plain(&text, 7), Some(1));
        assert_eq!(matching_bracket_plain(&text, 3), Some(5));
        assert_eq!(matching_bracket_plain(&text, 5), Some(3));
    }

    #[test]
    fn plain_handles_misses() {
        let text = Rope::from_str("(é[");
        assert_eq!(matching_bracket_plain(&text, 0), None);
        assert_eq!(matching_bracket_plain(&text, 1), None);
        assert_eq!(matching_bracket_plain(&text, 3), None);
        assert_eq!(matching_bracket_plain(&text, 99), None);
        let text = Rope::from_str("x)");
        assert_eq!(matching_bracket_plain(&text, 1), None);
    }

    #[test]
    fn plain_crosses_chunks() {
        let inner = "x".repeat(50_000);
        let text = Rope::from_str(&format!("{{{inner}}}"));
        assert_eq!(matching_bracket_plain(&text, 0), Some(50_001));
        assert_eq!(matching_bracket_plain(&text, 50_001), Some(0));
    }
}
