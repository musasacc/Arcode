//! Discord IPC frames: an opcode (u32, little-endian), the payload length
//! (u32, little-endian) and a UTF-8 JSON payload.

use std::io::{self, Read, Write};

/// Largest payload accepted from Discord. Its READY event carries the
/// user and the client configuration, a few kilobytes.
pub const MAX_PAYLOAD: usize = 1 << 20;

/// The kind of a frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    /// The first frame a client sends: `{"v":1,"client_id":"…"}`.
    Handshake = 0,
    /// A command or its response, or an event (`READY`).
    Frame = 1,
    /// The connection ends; the payload says why.
    Close = 2,
    /// Asks for a [`Opcode::Pong`] with the same payload.
    Ping = 3,
    /// The answer to a [`Opcode::Ping`].
    Pong = 4,
}

impl Opcode {
    /// The opcode for a number, if it is one.
    pub fn from_u32(value: u32) -> Option<Self> {
        Some(match value {
            0 => Self::Handshake,
            1 => Self::Frame,
            2 => Self::Close,
            3 => Self::Ping,
            4 => Self::Pong,
            _ => return None,
        })
    }
}

/// The bytes of one frame.
pub fn encode(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    let mut out = Vec::with_capacity(8 + payload.len());
    out.extend_from_slice(&(opcode as u32).to_le_bytes());
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
    out
}

/// Writes one frame and flushes it.
pub fn write_frame(w: &mut impl Write, opcode: Opcode, payload: &[u8]) -> io::Result<()> {
    if payload.len() > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame too large",
        ));
    }
    w.write_all(&encode(opcode, payload))?;
    w.flush()
}

/// Reads one frame. An unknown opcode or an oversized payload is an
/// `InvalidData` error (the stream cannot be trusted after either).
pub fn read_frame(r: &mut impl Read) -> io::Result<(Opcode, Vec<u8>)> {
    let mut header = [0u8; 8];
    r.read_exact(&mut header)?;
    let [a, b, c, d, e, f, g, h] = header;
    let opcode = u32::from_le_bytes([a, b, c, d]);
    let len = u32::from_le_bytes([e, f, g, h]) as usize;
    let opcode = Opcode::from_u32(opcode).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unknown opcode {opcode}"),
        )
    })?;
    if len > MAX_PAYLOAD {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("frame of {len} bytes"),
        ));
    }
    let mut payload = vec![0; len];
    r.read_exact(&mut payload)?;
    Ok((opcode, payload))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn frames_round_trip() {
        let bytes = encode(Opcode::Frame, br#"{"cmd":"SET_ACTIVITY"}"#);
        assert_eq!(&bytes[..8], &[1, 0, 0, 0, 22, 0, 0, 0]);
        let mut both = bytes.clone();
        both.extend(encode(Opcode::Ping, b""));
        let mut r = Cursor::new(both);
        let (op, payload) = read_frame(&mut r).unwrap();
        assert_eq!(op, Opcode::Frame);
        assert_eq!(payload, br#"{"cmd":"SET_ACTIVITY"}"#);
        assert_eq!(read_frame(&mut r).unwrap(), (Opcode::Ping, Vec::new()));
        // Nothing left: a clean end of stream.
        let err = read_frame(&mut r).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn written_frames_are_read_back() {
        let mut out = Vec::new();
        write_frame(&mut out, Opcode::Handshake, b"{}").unwrap();
        assert_eq!(out, [0, 0, 0, 0, 2, 0, 0, 0, b'{', b'}']);
        let mut big = Vec::new();
        let err = write_frame(&mut big, Opcode::Frame, &vec![b' '; MAX_PAYLOAD + 1]).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(big.is_empty());
    }

    #[test]
    fn bad_frames_are_errors() {
        let mut unknown = Cursor::new(encode(Opcode::Frame, b"{}"));
        unknown.get_mut()[0] = 9;
        assert_eq!(
            read_frame(&mut unknown).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        let mut huge = [1u8, 0, 0, 0, 0, 0, 0, 0x7f].to_vec();
        huge.extend_from_slice(b"{}");
        assert_eq!(
            read_frame(&mut Cursor::new(huge)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
        // A payload cut short.
        let mut short = encode(Opcode::Frame, b"{\"a\":1}");
        short.truncate(10);
        assert_eq!(
            read_frame(&mut Cursor::new(short)).unwrap_err().kind(),
            io::ErrorKind::UnexpectedEof
        );
        for n in 0..=4 {
            assert_eq!(Opcode::from_u32(n).map(|o| o as u32), Some(n));
        }
        assert_eq!(Opcode::from_u32(5), None);
    }
}
