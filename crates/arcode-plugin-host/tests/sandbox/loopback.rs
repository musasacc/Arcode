//! A tiny HTTP/1.1 server on 127.0.0.1 for the `http` tests. Loopback
//! only: no test touches the external network.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// A request the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Seen {
    /// The first header named `name` (case-insensitive).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// What to answer.
#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// Wait this long before answering.
    pub delay: Duration,
}

impl Reply {
    /// `status` with a text body.
    pub fn text(status: u16, body: &str) -> Self {
        Self {
            status,
            headers: vec![("content-type".into(), "text/plain".into())],
            body: body.as_bytes().to_vec(),
            delay: Duration::ZERO,
        }
    }

    /// A redirect to `location`.
    pub fn redirect(status: u16, location: &str) -> Self {
        Self {
            status,
            headers: vec![("location".into(), location.into())],
            body: b"moved".to_vec(),
            delay: Duration::ZERO,
        }
    }
}

type Handler = dyn Fn(&Seen) -> Reply + Send + Sync;

/// A running server.
pub struct Server {
    pub port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    connections: Arc<AtomicUsize>,
}

impl Server {
    /// Listens on an ephemeral loopback port and answers with `handler`.
    pub fn start(handler: impl Fn(&Seen) -> Reply + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().expect("local addr").port();
        let seen: Arc<Mutex<Vec<Seen>>> = Arc::default();
        let connections = Arc::new(AtomicUsize::new(0));
        let handler: Arc<Handler> = Arc::new(handler);
        let (s, c) = (seen.clone(), connections.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                c.fetch_add(1, Ordering::SeqCst);
                let (s, h) = (s.clone(), handler.clone());
                std::thread::spawn(move || serve(stream, &s, &*h));
            }
        });
        Self {
            port,
            seen,
            connections,
        }
    }

    /// `http://127.0.0.1:<port><path>`.
    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// Requests received so far.
    pub fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap().clone()
    }

    /// Connections accepted so far.
    pub fn connections(&self) -> usize {
        self.connections.load(Ordering::SeqCst)
    }
}

fn serve(stream: TcpStream, seen: &Mutex<Vec<Seen>>, handler: &Handler) {
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    if reader.read_line(&mut line).is_err() {
        return;
    }
    let mut parts = line.split_whitespace();
    let (method, path) = (
        parts.next().unwrap_or_default(),
        parts.next().unwrap_or_default(),
    );
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        if reader.read_line(&mut h).is_err() || h.trim_end().is_empty() {
            break;
        }
        if let Some((k, v)) = h.trim_end().split_once(':') {
            headers.push((k.trim().to_owned(), v.trim().to_owned()));
        }
    }
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0; length];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    let request = Seen {
        method: method.to_owned(),
        path: path.to_owned(),
        headers,
        body,
    };
    seen.lock().unwrap().push(request.clone());
    let reply = handler(&request);
    std::thread::sleep(reply.delay);
    let mut head = format!(
        "HTTP/1.1 {} X\r\ncontent-length: {}\r\nconnection: close\r\n",
        reply.status,
        reply.body.len()
    );
    for (k, v) in &reply.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    let mut stream = stream;
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&reply.body);
    let _ = stream.flush();
}
