//! The real [`HttpTransport`]: ureq 3 with rustls (§1 "Network").
//!
//! The host has already checked gesture, master switch, manifest hosts and
//! consent for the first hop. Here:
//! - each hop runs on a short-lived `plugin-http` helper thread while the
//!   plugin thread waits on the result *or* the cancel token, so a cancel
//!   returns within one poll interval (10 ms) even if the server stalls;
//! - redirects are followed here, not by ureq, and `allow_redirect` is asked
//!   before every hop (consent is re-checked per host);
//! - the body is read up to the request's `max-response-bytes`, then
//!   `truncated` is set;
//! - 4xx/5xx are ordinary responses, not errors.
//!
//! `remote-addr` is always `None`: ureq 3 does not expose the peer address.

mod log;
mod redirect;

pub use log::{NetworkEntry, NetworkLog, NetworkOutcome};
pub use redirect::{Hop, is_redirect, next_hop, resolve};

use std::io::Read;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use ureq::RequestExt;
use ureq::http;

use crate::bindings::http::{HttpError, Request, Response, Timing};
use crate::bridge::HttpTransport;
use crate::cancel::CancelToken;
use crate::host_impl::http::parse_origin;

/// Used when a request asks for `timeout-ms = 0`.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Longest request timeout.
pub const MAX_TIMEOUT: Duration = Duration::from_secs(600);
/// Redirects followed per request.
pub const MAX_REDIRECTS: u32 = 10;
/// How often the waiting plugin thread checks its cancel token.
const POLL: Duration = Duration::from_millis(10);
/// Body read chunk.
const CHUNK: usize = 64 * 1024;

/// How the transport is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportOptions {
    /// Use `HTTPS_PROXY`/`HTTP_PROXY`/`ALL_PROXY` (and `NO_PROXY`).
    pub use_env_proxy: bool,
    /// `User-Agent` when the plugin sets none.
    pub user_agent: String,
}

impl Default for TransportOptions {
    fn default() -> Self {
        Self {
            use_env_proxy: true,
            user_agent: format!("Arcode/{}", env!("CARGO_PKG_VERSION")),
        }
    }
}

/// `http.send` over ureq.
#[derive(Debug, Clone)]
pub struct UreqTransport {
    agent: ureq::Agent,
}

impl Default for UreqTransport {
    fn default() -> Self {
        Self::new(&TransportOptions::default())
    }
}

impl UreqTransport {
    /// A transport; creates no connection until the first request.
    pub fn new(options: &TransportOptions) -> Self {
        let mut config = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .max_redirects_will_error(false)
            .allow_non_standard_methods(true)
            .user_agent(options.user_agent.as_str());
        if !options.use_env_proxy {
            config = config.proxy(None);
        }
        Self {
            agent: config.build().new_agent(),
        }
    }

    /// Runs one hop on a helper thread; returns early on cancel.
    fn exchange(&self, job: Job, cancel: &CancelToken) -> Result<Exchanged, HttpError> {
        let (tx, rx) = mpsc::channel();
        let agent = self.agent.clone();
        let token = cancel.clone();
        std::thread::Builder::new()
            .name("plugin-http".into())
            .spawn(move || {
                let _ = tx.send(run(&agent, &job, &token));
            })
            .map_err(|e| HttpError::Io(format!("cannot start the transfer: {e}")))?;
        loop {
            match rx.recv_timeout(POLL) {
                Ok(result) => return result,
                Err(RecvTimeoutError::Timeout) if cancel.is_cancelled() => {
                    return Err(HttpError::Cancelled);
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(HttpError::Io("the transfer stopped".into()));
                }
            }
        }
    }
}

/// What the helper thread needs for one hop.
struct Job {
    hop: Hop,
    timeout: Duration,
    started: Instant,
    max_bytes: u64,
    /// Skip the body of a redirect that will be followed.
    follow: bool,
}

/// One hop's answer.
struct Exchanged {
    status: http::StatusCode,
    version: http::Version,
    headers: Vec<(String, String)>,
    location: Option<String>,
    body: Vec<u8>,
    truncated: bool,
    headers_ms: u32,
}

fn millis(d: Duration) -> u32 {
    u32::try_from(d.as_millis()).unwrap_or(u32::MAX)
}

impl HttpTransport for UreqTransport {
    fn send(
        &self,
        request: Request,
        cancel: &CancelToken,
        allow_redirect: &mut dyn FnMut(&str, u16) -> bool,
    ) -> Result<Response, HttpError> {
        let started = Instant::now();
        let timeout = match request.timeout_ms {
            0 => DEFAULT_TIMEOUT,
            ms => Duration::from_millis(u64::from(ms)).min(MAX_TIMEOUT),
        };
        let deadline = started + timeout;
        let mut hop = Hop {
            method: request.method.to_ascii_uppercase(),
            url: request.url,
            headers: request.headers,
            body: Arc::from(request.body),
        };
        let mut origin = parse_origin(&hop.url).map_err(HttpError::InvalidRequest)?;
        for _ in 0..=MAX_REDIRECTS {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(HttpError::Timeout);
            }
            let job = Job {
                hop: hop.clone(),
                timeout: remaining,
                started,
                max_bytes: request.max_response_bytes,
                follow: request.follow_redirects,
            };
            let ex = self.exchange(job, cancel)?;
            let location = ex.location.as_deref().filter(|_| request.follow_redirects);
            let Some(location) = location.filter(|_| is_redirect(ex.status.as_u16())) else {
                return Ok(Response {
                    status: ex.status.as_u16(),
                    reason: ex.status.canonical_reason().unwrap_or_default().to_owned(),
                    version: format!("{:?}", ex.version),
                    final_url: hop.url,
                    remote_addr: None,
                    headers: ex.headers,
                    body: ex.body,
                    truncated: ex.truncated,
                    timing: Timing {
                        headers_ms: ex.headers_ms,
                        total_ms: millis(started.elapsed()),
                    },
                });
            };
            let url = resolve(&hop.url, location).map_err(HttpError::Protocol)?;
            let next = parse_origin(&url)
                .map_err(|e| HttpError::Protocol(format!("redirect to an unsupported URL: {e}")))?;
            if !allow_redirect(&next.host, next.port) {
                return Err(HttpError::Denied);
            }
            hop = next_hop(&hop, ex.status.as_u16(), url, next == origin);
            origin = next;
        }
        Err(HttpError::Protocol(format!(
            "more than {MAX_REDIRECTS} redirects"
        )))
    }
}

/// Sends one hop and reads its body (helper thread).
fn run(agent: &ureq::Agent, job: &Job, cancel: &CancelToken) -> Result<Exchanged, HttpError> {
    let hop = &job.hop;
    let method = http::Method::from_bytes(hop.method.as_bytes())
        .map_err(|_| HttpError::InvalidRequest(format!("invalid method `{}`", hop.method)))?;
    let mut builder = http::Request::builder().method(method).uri(&hop.url);
    for (name, value) in &hop.headers {
        builder = builder.header(name.as_str(), value.as_str());
    }
    let response = if hop.body.is_empty() {
        let request = builder
            .body(())
            .map_err(|e| HttpError::InvalidRequest(e.to_string()))?;
        call(agent, request, job.timeout)
    } else {
        let request = builder
            .body(&*hop.body)
            .map_err(|e| HttpError::InvalidRequest(e.to_string()))?;
        call(agent, request, job.timeout)
    }
    .map_err(|e| map_error(e, &hop.url))?;
    let headers_ms = millis(job.started.elapsed());
    let (parts, body) = response.into_parts();
    let location = parts
        .headers
        .get(http::header::LOCATION)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let headers = parts
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_owned(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let skip_body = job.follow && location.is_some() && is_redirect(parts.status.as_u16());
    let (body, truncated) = if skip_body {
        (Vec::new(), false)
    } else {
        read_body(body.into_reader(), job.max_bytes, cancel)?
    };
    Ok(Exchanged {
        status: parts.status,
        version: parts.version,
        headers,
        location,
        body,
        truncated,
        headers_ms,
    })
}

fn call(
    agent: &ureq::Agent,
    request: http::Request<impl ureq::AsSendBody>,
    timeout: Duration,
) -> Result<http::Response<ureq::Body>, ureq::Error> {
    request
        .with_agent(agent)
        .configure()
        .timeout_global(Some(timeout))
        .build()
        .run()
}

/// Reads at most `max` bytes; `truncated` if more were available.
fn read_body(
    mut reader: impl Read,
    max: u64,
    cancel: &CancelToken,
) -> Result<(Vec<u8>, bool), HttpError> {
    let max = usize::try_from(max).unwrap_or(usize::MAX);
    let mut body = Vec::new();
    let mut chunk = vec![0u8; CHUNK];
    loop {
        if cancel.is_cancelled() {
            return Err(HttpError::Cancelled);
        }
        let want = (max - body.len()).min(CHUNK);
        if want == 0 {
            // At the cap: one more byte decides `truncated`.
            let more = reader.read(&mut chunk[..1]).map_err(map_io)?;
            return Ok((body, more > 0));
        }
        match reader.read(&mut chunk[..want]).map_err(map_io)? {
            0 => return Ok((body, false)),
            n => body.extend_from_slice(&chunk[..n]),
        }
    }
}

fn map_io(e: std::io::Error) -> HttpError {
    use std::io::ErrorKind as K;
    match e.kind() {
        K::TimedOut => HttpError::Timeout,
        K::ConnectionReset | K::ConnectionAborted | K::UnexpectedEof => {
            HttpError::Protocol(e.to_string())
        }
        _ => HttpError::Io(e.to_string()),
    }
}

fn map_error(e: ureq::Error, url: &str) -> HttpError {
    use ureq::Error as E;
    match e {
        E::Timeout(_) => HttpError::Timeout,
        E::HostNotFound => {
            let host = parse_origin(url).map(|o| o.host).unwrap_or_default();
            HttpError::Dns(format!("cannot resolve `{host}`"))
        }
        E::ConnectionFailed => HttpError::Connect("connection failed".into()),
        E::Io(io) => match io.kind() {
            std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::AddrNotAvailable => {
                HttpError::Connect(io.to_string())
            }
            _ => map_io(io),
        },
        E::Tls(m) => HttpError::Tls(m.to_owned()),
        E::Rustls(e) => HttpError::Tls(e.to_string()),
        E::Pem(e) => HttpError::Tls(e.to_string()),
        E::Protocol(e) => HttpError::Protocol(e.to_string()),
        E::BadUri(m) => HttpError::InvalidRequest(m),
        E::Http(e) => HttpError::InvalidRequest(e.to_string()),
        other => HttpError::Io(other.to_string()),
    }
}
