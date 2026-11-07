//! The Discord IPC conversation on one connection: the handshake, setting
//! the activity, answering pings and noticing that Discord went away.
//!
//! Every operation ends within the connection's timeout, however Discord
//! behaves (silent, flooding, or stopping mid-frame), so the presence
//! thread always gets back to its messages.

use std::io;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::frame::{Opcode, read_frame, write_frame};
use crate::socket::IpcStream;

/// How often the handshake checks for Discord's answer.
const ANSWER_POLL: Duration = Duration::from_millis(20);

/// Most frames one check reads; the rest wait for the next check.
const MAX_FRAMES_PER_CHECK: usize = 16;

/// Discord's close code for an application ID it does not know.
pub const INVALID_CLIENT_ID: i64 = 4000;

/// Discord closed the connection; the payload says why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Closed {
    /// The close code (4000: unknown application ID).
    pub code: Option<i64>,
    /// The payload as text.
    pub message: String,
}

impl std::fmt::Display for Closed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Discord closed the connection: {}", self.message)
    }
}

impl std::error::Error for Closed {}

impl Closed {
    /// The close behind `err`, if Discord sent one.
    pub fn of(err: &io::Error) -> Option<&Self> {
        err.get_ref()?.downcast_ref()
    }
}

/// A connection that finished the handshake.
pub struct Client<S: IpcStream> {
    stream: S,
    nonces: u64,
    /// The longest one operation may take.
    timeout: Duration,
}

impl<S: IpcStream> Client<S> {
    /// Sends the handshake for `application_id` and waits up to `timeout`
    /// for Discord's `READY`. A close frame (an unknown application ID,
    /// for one) is a `ConnectionRefused` error carrying [`Closed`]. Later
    /// operations also end within `timeout`.
    pub fn handshake(stream: S, application_id: &str, timeout: Duration) -> io::Result<Self> {
        let mut client = Self {
            stream,
            nonces: 0,
            timeout,
        };
        let deadline = Instant::now() + timeout;
        client.stream.set_deadline(Some(deadline));
        let hello = json!({ "v": 1, "client_id": application_id });
        write_frame(
            &mut client.stream,
            Opcode::Handshake,
            hello.to_string().as_bytes(),
        )?;
        loop {
            // Checked before every frame: a Discord that keeps sending
            // other frames does not hold the handshake either.
            let now = Instant::now();
            if now >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Discord did not answer the handshake",
                ));
            }
            if !client.stream.has_data()? {
                std::thread::sleep(ANSWER_POLL.min(deadline - now));
                continue;
            }
            if let Some(event) = client.read_one()?
                && event.get("evt").and_then(Value::as_str) == Some("READY")
            {
                return Ok(client);
            }
        }
    }

    /// Sets the activity (`None` clears it). Answers what arrived first.
    pub fn set_activity(&mut self, activity: Option<&Value>) -> io::Result<()> {
        self.check()?;
        self.nonces += 1;
        let command = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": format!("{}-{}", std::process::id(), self.nonces),
        });
        self.arm();
        write_frame(
            &mut self.stream,
            Opcode::Frame,
            command.to_string().as_bytes(),
        )
    }

    /// Reads what arrived without waiting for more: answers pings, drops
    /// command responses. An error means the connection is gone.
    pub fn check(&mut self) -> io::Result<()> {
        self.arm();
        for _ in 0..MAX_FRAMES_PER_CHECK {
            if !self.stream.has_data()? {
                break;
            }
            self.read_one()?;
        }
        Ok(())
    }

    /// Says goodbye; Discord clears the activity of a closed connection.
    pub fn close(mut self) {
        self.arm();
        let _ = write_frame(&mut self.stream, Opcode::Close, b"{}");
    }

    /// Gives the next operation [`Client::timeout`] to finish.
    fn arm(&mut self) {
        self.stream
            .set_deadline(Some(Instant::now() + self.timeout));
    }

    /// Reads one frame and handles it; returns the payload of a command
    /// frame.
    fn read_one(&mut self) -> io::Result<Option<Value>> {
        let (opcode, payload) = read_frame(&mut self.stream)?;
        match opcode {
            Opcode::Ping => {
                write_frame(&mut self.stream, Opcode::Pong, &payload)?;
                Ok(None)
            }
            Opcode::Close => {
                let code = serde_json::from_slice::<Value>(&payload)
                    .ok()
                    .and_then(|v| v.get("code").and_then(Value::as_i64));
                let closed = Closed {
                    code,
                    message: String::from_utf8_lossy(&payload).into_owned(),
                };
                Err(io::Error::new(io::ErrorKind::ConnectionRefused, closed))
            }
            Opcode::Frame => {
                let value: Value = serde_json::from_slice(&payload)
                    .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
                if value.get("evt").and_then(Value::as_str) == Some("ERROR") {
                    log::debug!("discord: {}", value.get("data").unwrap_or(&Value::Null));
                }
                Ok(Some(value))
            }
            Opcode::Pong | Opcode::Handshake => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame::encode;
    use std::io::{Cursor, Read, Write};

    /// Frames from "Discord" waiting to be read, and what the client wrote.
    struct Mem {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Mem {
        fn new(frames: &[(Opcode, &str)]) -> Self {
            let input = frames
                .iter()
                .flat_map(|(op, payload)| encode(*op, payload.as_bytes()))
                .collect();
            Self {
                input: Cursor::new(input),
                output: Vec::new(),
            }
        }

        fn written(&self) -> Vec<(Opcode, Value)> {
            let mut r = Cursor::new(self.output.clone());
            let mut frames = Vec::new();
            while let Ok((op, payload)) = read_frame(&mut r) {
                frames.push((op, serde_json::from_slice(&payload).unwrap_or(Value::Null)));
            }
            frames
        }
    }

    impl Read for Mem {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for Mem {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl IpcStream for Mem {
        fn has_data(&mut self) -> io::Result<bool> {
            Ok((self.input.position() as usize) < self.input.get_ref().len())
        }

        fn set_deadline(&mut self, _: Option<Instant>) {}
    }

    const READY: &str = r#"{"cmd":"DISPATCH","evt":"READY","data":{"v":1}}"#;

    #[test]
    fn handshake_waits_for_ready_and_answers_pings() {
        let mem = Mem::new(&[
            (Opcode::Ping, r#"{"n":7}"#),
            (Opcode::Frame, READY),
            (Opcode::Ping, r#"{"n":8}"#),
        ]);
        let mut client = Client::handshake(mem, "42", Duration::from_secs(1)).unwrap();
        client
            .set_activity(Some(&json!({"details":"Editing"})))
            .unwrap();
        client.set_activity(None).unwrap();
        let written = client.stream.written();
        assert_eq!(
            written[0],
            (Opcode::Handshake, json!({"v":1,"client_id":"42"}))
        );
        assert_eq!(written[1], (Opcode::Pong, json!({"n":7})));
        // The ping after READY is answered before the next command.
        assert_eq!(written[2], (Opcode::Pong, json!({"n":8})));
        let (op, set) = &written[3];
        assert_eq!(*op, Opcode::Frame);
        assert_eq!(set["cmd"], "SET_ACTIVITY");
        assert_eq!(set["args"]["pid"], std::process::id());
        assert_eq!(set["args"]["activity"]["details"], "Editing");
        let (_, clear) = &written[4];
        assert!(clear["args"]["activity"].is_null());
        assert_ne!(set["nonce"], clear["nonce"], "nonces are unique");
        client.close();
    }

    #[test]
    fn a_close_frame_or_silence_fails_the_handshake() {
        let refused = Mem::new(&[(
            Opcode::Close,
            r#"{"code":4000,"message":"Invalid Client ID"}"#,
        )]);
        let err = Client::handshake(refused, "1", Duration::from_secs(1))
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused);
        let closed = Closed::of(&err).expect("the close is kept");
        assert_eq!(closed.code, Some(INVALID_CLIENT_ID));
        let silent = Mem::new(&[]);
        let err = Client::handshake(silent, "1", Duration::from_millis(50))
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        let garbage = Mem::new(&[(Opcode::Frame, "not json")]);
        let err = Client::handshake(garbage, "1", Duration::from_secs(1))
            .err()
            .unwrap();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    /// A Discord that sends pings without end and never READY.
    struct Flood {
        frame: Vec<u8>,
        pos: usize,
    }

    impl Read for Flood {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            let mut n = 0;
            while n < buf.len() {
                buf[n] = self.frame[self.pos];
                self.pos = (self.pos + 1) % self.frame.len();
                n += 1;
            }
            Ok(n)
        }
    }

    impl Write for Flood {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl IpcStream for Flood {
        fn has_data(&mut self) -> io::Result<bool> {
            Ok(true)
        }

        fn set_deadline(&mut self, _: Option<Instant>) {}
    }

    fn flood() -> Flood {
        Flood {
            frame: encode(Opcode::Ping, br#"{"n":1}"#),
            pos: 0,
        }
    }

    #[test]
    fn an_endless_stream_of_other_frames_ends_the_handshake_in_time() {
        let started = Instant::now();
        let err = Client::handshake(flood(), "1", Duration::from_millis(100))
            .err()
            .expect("no READY");
        assert_eq!(err.kind(), io::ErrorKind::TimedOut);
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "{:?}",
            started.elapsed()
        );
        // After the handshake, one check reads a bounded number of frames.
        let mut client = Client {
            stream: flood(),
            nonces: 0,
            timeout: Duration::from_secs(1),
        };
        client.check().unwrap();
        client.set_activity(None).unwrap();
    }
}
