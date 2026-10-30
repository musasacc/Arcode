//! Encoding of pasted text and commands.

/// Bracketed paste start marker.
pub const PASTE_START: &[u8] = b"\x1b[200~";
/// Bracketed paste end marker.
pub const PASTE_END: &[u8] = b"\x1b[201~";

/// Encodes `text` for pasting: newlines (`\r\n`, `\n`) become `\r`; with
/// `bracketed` the text is wrapped in paste markers and ESC characters are
/// removed so the paste cannot terminate the bracket early.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let body = normalized.replace('\x1b', "");
        let mut out = Vec::with_capacity(body.len() + PASTE_START.len() + PASTE_END.len());
        out.extend_from_slice(PASTE_START);
        out.extend_from_slice(body.as_bytes());
        out.extend_from_slice(PASTE_END);
        out
    } else {
        normalized.into_bytes()
    }
}

/// Encodes a command line followed by Enter (`\r`).
pub fn encode_command(cmd: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(cmd.len() + 1);
    out.extend_from_slice(cmd.as_bytes());
    out.push(b'\r');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_paste_normalizes_newlines() {
        assert_eq!(encode_paste("a\nb\r\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(encode_paste("", false), Vec::<u8>::new());
        assert_eq!(encode_paste("x\x1by", false), b"x\x1by".to_vec());
    }

    #[test]
    fn bracketed_paste_wraps_and_strips_escape() {
        assert_eq!(
            encode_paste("ls\n", true),
            b"\x1b[200~ls\r\x1b[201~".to_vec()
        );
        assert_eq!(
            encode_paste("evil\x1b[201~rm", true),
            b"\x1b[200~evil[201~rm\x1b[201~".to_vec()
        );
    }

    #[test]
    fn command_appends_cr() {
        assert_eq!(encode_command("echo hi"), b"echo hi\r".to_vec());
        assert_eq!(encode_command(""), b"\r".to_vec());
    }
}
