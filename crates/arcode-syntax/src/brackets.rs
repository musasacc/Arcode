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

/// A bracket token: its bracket byte and how many times it repeats.
///
/// Most grammars use one-byte tokens (`(`), but some lex runs of the same
/// bracket as one token, like TOML's `[[` and `]]` around array-of-tables
/// headers. Each byte of such a token pairs with the mirrored byte of the
/// partner token (the outer `[` with the outer `]`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Token {
    byte: u8,
    width: usize,
}

impl Token {
    /// The bracket token `node` stands for, if it is one.
    ///
    /// Named, missing and zero-width nodes never count, and the kind must
    /// spell exactly the bytes the node covers.
    fn of(node: Node) -> Option<Token> {
        if node.is_named() || node.is_missing() {
            return None;
        }
        let kind = node.kind().as_bytes();
        let (&byte, rest) = kind.split_first()?;
        partner(byte)?;
        if rest.iter().any(|&b| b != byte) || kind.len() != node.byte_range().len() {
            return None;
        }
        Some(Token {
            byte,
            width: kind.len(),
        })
    }

    /// The token this one pairs with, and whether this one opens.
    fn partner(self) -> Option<(Token, bool)> {
        let (byte, opens) = partner(self.byte)?;
        Some((
            Token {
                byte,
                width: self.width,
            },
            opens,
        ))
    }
}

impl Syntax {
    /// Finds the bracket matching the one at `byte_offset`.
    ///
    /// Only bracket *tokens* in the tree count, so brackets inside strings
    /// and comments are ignored. Multi-byte tokens such as TOML's `[[` work
    /// byte by byte: the outer `[` matches the outer `]` of `]]`. Returns
    /// the partner's byte offset, or `None` if there is no bracket at
    /// `byte_offset` or it is unmatched.
    ///
    /// Well-formed pairs that delimit their parent node (the common case,
    /// including huge JSON arrays) are found without visiting the siblings
    /// in between; other cases scan the siblings.
    ///
    /// `text` must be the text this tree was last parsed from.
    pub fn matching_bracket(&self, text: &Rope, byte_offset: usize) -> Option<usize> {
        if byte_offset >= text.len_bytes() {
            return None;
        }
        let root = self.tree().root_node();
        let node = root.descendant_for_byte_range(byte_offset, byte_offset + 1)?;
        if !node.byte_range().contains(&byte_offset) {
            return None;
        }
        let token = Token::of(node)?;
        // The tree may be stale; never trust it without checking the text.
        if text.get_byte(byte_offset) != Some(token.byte) {
            return None;
        }
        let (target, opens) = token.partner()?;
        let parent = node.parent()?;
        let found = delimiting_partner(node, parent, target, opens)
            .or_else(|| scan_siblings(node, parent, token, target, opens))?;
        // Mirror the byte inside multi-byte tokens: `[[` at +0 pairs with
        // the last byte of `]]`.
        let index = byte_offset - node.start_byte();
        Some(found.start_byte() + (token.width - 1 - index))
    }
}

/// Fast path: `node` is the first (or last) child of an error-free `parent`
/// whose last (or first) child is the partner token.
///
/// Grammars wrap a bracketed construct in its own node, so such a parent is
/// delimited by this one pair. This needs no walk over the siblings between,
/// which matters for huge arrays and objects.
fn delimiting_partner<'t>(
    node: Node<'t>,
    parent: Node<'t>,
    target: Token,
    opens: bool,
) -> Option<Node<'t>> {
    if parent.has_error() {
        return None;
    }
    let first = parent.child(0)?;
    let mut cursor = parent.walk();
    if !cursor.goto_last_child() {
        return None;
    }
    let last = cursor.node();
    let (own, other) = if opens { (first, last) } else { (last, first) };
    (own == node && other != node && Token::of(other) == Some(target)).then_some(other)
}

/// Slow path: scans the siblings of `node` for its partner, counting nesting
/// of the same token pair.
fn scan_siblings<'t>(
    node: Node<'t>,
    parent: Node<'t>,
    token: Token,
    target: Token,
    opens: bool,
) -> Option<Node<'t>> {
    let mut cursor = parent.walk();
    if opens {
        // Jump to `node` (the first child ending past its start), then
        // count nesting forwards.
        cursor.goto_first_child_for_byte(node.start_byte())?;
        while cursor.node() != node {
            if !cursor.goto_next_sibling() {
                return None;
            }
        }
        let mut depth = 0usize;
        while cursor.goto_next_sibling() {
            let sibling = cursor.node();
            match Token::of(sibling) {
                Some(t) if t == token => depth += 1,
                Some(t) if t == target => match depth.checked_sub(1) {
                    Some(d) => depth = d,
                    None => return Some(sibling),
                },
                _ => {}
            }
        }
        None
    } else {
        // Walk forwards to `node`, keeping a stack of unmatched openers;
        // the innermost one is the partner.
        if !cursor.goto_first_child() {
            return None;
        }
        let mut open = Vec::new();
        loop {
            let sibling = cursor.node();
            if sibling == node {
                return open.pop();
            }
            match Token::of(sibling) {
                Some(t) if t == target => open.push(sibling),
                Some(t) if t == token => {
                    open.pop();
                }
                _ => {}
            }
            if !cursor.goto_next_sibling() {
                return None;
            }
        }
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
