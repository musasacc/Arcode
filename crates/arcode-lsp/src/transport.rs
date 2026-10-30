//! LSP base protocol framing: `Content-Length` headers followed by a JSON body.

use std::io;

use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt};

/// Upper bound for a single message body; protects against garbage headers.
pub const MAX_MESSAGE_SIZE: usize = 512 * 1024 * 1024;

/// Upper bound for a single header line.
const MAX_HEADER_LINE: usize = 8 * 1024;

/// Reads one framed message and returns its body.
///
/// Returns `Ok(None)` on a clean end of stream (EOF before any header byte).
/// Header names are matched case-insensitively; `Content-Type` and unknown
/// headers are accepted and ignored. Lines may end in `\r\n` or `\n`.
pub async fn read_message<R>(reader: &mut R) -> io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin + ?Sized,
{
    let mut content_length: Option<usize> = None;
    let mut line = Vec::new();
    let mut first = true;
    loop {
        line.clear();
        let n = read_header_line(reader, &mut line).await?;
        if n == 0 {
            if first {
                return Ok(None);
            }
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "stream ended inside message headers",
            ));
        }
        first = false;
        let text = std::str::from_utf8(&line)
            .map_err(|_| invalid("header is not valid UTF-8"))?
            .trim_end_matches(['\r', '\n']);
        if text.is_empty() {
            if content_length.is_some() {
                break;
            }
            // Tolerate stray blank lines between messages.
            first = true;
            continue;
        }
        let (name, value) = text
            .split_once(':')
            .ok_or_else(|| invalid(format!("malformed header line: {text:?}")))?;
        if name.trim().eq_ignore_ascii_case("content-length") {
            let len: usize = value
                .trim()
                .parse()
                .map_err(|_| invalid(format!("bad Content-Length: {:?}", value.trim())))?;
            if len > MAX_MESSAGE_SIZE {
                return Err(invalid(format!("message too large: {len} bytes")));
            }
            content_length = Some(len);
        }
    }
    let len = content_length.unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).await?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|_| invalid("message body is not valid UTF-8"))
}

/// Reads one header line (including its terminator) with a length limit.
async fn read_header_line<R>(reader: &mut R, line: &mut Vec<u8>) -> io::Result<usize>
where
    R: AsyncBufRead + Unpin + ?Sized,
{
    let mut limited = (&mut *reader).take(MAX_HEADER_LINE as u64);
    let n = limited.read_until(b'\n', line).await?;
    if n == MAX_HEADER_LINE && line.last() != Some(&b'\n') {
        return Err(invalid("header line too long"));
    }
    Ok(n)
}

/// Writes one framed message and flushes the writer.
pub async fn write_message<W>(writer: &mut W, body: &str) -> io::Result<()>
where
    W: AsyncWrite + Unpin + ?Sized,
{
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes()).await?;
    writer.write_all(body.as_bytes()).await?;
    writer.flush().await
}

fn invalid(msg: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    async fn read_all(input: &[u8]) -> io::Result<Vec<String>> {
        let mut reader = BufReader::new(input);
        let mut out = Vec::new();
        while let Some(msg) = read_message(&mut reader).await? {
            out.push(msg);
        }
        Ok(out)
    }

    #[tokio::test]
    async fn reads_single_message() {
        let msgs = read_all(b"Content-Length: 2\r\n\r\n{}").await.unwrap();
        assert_eq!(msgs, vec!["{}"]);
    }

    #[tokio::test]
    async fn reads_multiple_messages_in_one_buffer() {
        let input = b"Content-Length: 7\r\n\r\n{\"a\":1}Content-Length: 7\r\n\r\n{\"b\":2}";
        let msgs = read_all(input).await.unwrap();
        assert_eq!(msgs, vec!["{\"a\":1}", "{\"b\":2}"]);
    }

    #[tokio::test]
    async fn header_names_are_case_insensitive_and_content_type_ignored() {
        let input = b"content-type: application/vscode-jsonrpc; charset=utf-8\r\n\
CONTENT-LENGTH:   4\r\n\r\nnull";
        assert_eq!(read_all(input).await.unwrap(), vec!["null"]);
    }

    #[tokio::test]
    async fn accepts_bare_newlines() {
        let input = b"Content-Length: 4\n\ntrue";
        assert_eq!(read_all(input).await.unwrap(), vec!["true"]);
    }

    #[tokio::test]
    async fn length_counts_bytes_not_chars() {
        let body = "\"h\u{e9}\u{1F600}\"";
        let input = format!("Content-Length: {}\r\n\r\n{body}", body.len());
        assert_eq!(read_all(input.as_bytes()).await.unwrap(), vec![body]);
    }

    #[tokio::test]
    async fn clean_eof_is_none() {
        assert!(read_all(b"").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn eof_inside_headers_or_body_is_error() {
        let err = read_all(b"Content-Length: 10\r\n").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
        let err = read_all(b"Content-Length: 10\r\n\r\n{}").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn bad_headers_are_invalid_data() {
        let err = read_all(b"Content-Length: abc\r\n\r\n").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = read_all(b"garbage\r\n\r\n").await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let huge = format!("Content-Length: {}\r\n\r\n", MAX_MESSAGE_SIZE + 1);
        let err = read_all(huge.as_bytes()).await.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn handles_partial_reads_across_chunks() {
        let (mut tx, rx) = tokio::io::duplex(4);
        let writer = tokio::spawn(async move {
            let data = b"Content-Length: 10\r\n\r\n{\"x\":\"yz\"}Content-Length: 2\r\n\r\n[]";
            for chunk in data.chunks(3) {
                tx.write_all(chunk).await.unwrap();
                tokio::task::yield_now().await;
            }
        });
        let mut reader = BufReader::new(rx);
        assert_eq!(
            read_message(&mut reader).await.unwrap().as_deref(),
            Some("{\"x\":\"yz\"}")
        );
        assert_eq!(
            read_message(&mut reader).await.unwrap().as_deref(),
            Some("[]")
        );
        writer.await.unwrap();
        assert_eq!(read_message(&mut reader).await.unwrap(), None);
    }

    #[tokio::test]
    async fn write_then_read_roundtrip() {
        let mut buf = Vec::new();
        write_message(&mut buf, "{\"jsonrpc\":\"2.0\"}")
            .await
            .unwrap();
        write_message(&mut buf, "\u{e4}").await.unwrap();
        assert!(buf.starts_with(b"Content-Length: 17\r\n\r\n"));
        let msgs = read_all(&buf).await.unwrap();
        assert_eq!(msgs, vec!["{\"jsonrpc\":\"2.0\"}", "\u{e4}"]);
    }
}
