//! `sockets` fixture: WASI sockets are linked but always denied. Tries a
//! TCP connect, a UDP bind/send and a name lookup, and reports each result
//! as JSON through `host.log`.

use std::net::{TcpStream, ToSocketAddrs, UdpSocket};
use std::time::Duration;

use arcode_plugin::prelude::*;
use serde_json::{Value, json};

#[derive(Default)]
struct Sockets;

fn report<T>(op: &str, r: std::io::Result<T>) {
    let line = match r {
        Ok(_) => json!({"op": op, "ok": true}),
        Err(e) => json!({"op": op, "ok": false, "error": e.to_string()}),
    };
    host::log(host::Level::Info, &line.to_string());
}

impl Plugin for Sockets {
    fn command(&mut self, _id: &str, args: Json) -> Result<()> {
        let args: Value = args.parse()?;
        let addr = args["addr"].as_str().unwrap_or("127.0.0.1:9");
        match addr.parse() {
            Ok(sock) => report(
                "tcp-connect",
                TcpStream::connect_timeout(&sock, Duration::from_secs(2)),
            ),
            Err(_) => report::<()>("tcp-connect", Err(std::io::ErrorKind::InvalidInput.into())),
        }
        report("tcp-connect-plain", TcpStream::connect(addr));
        let udp = UdpSocket::bind("127.0.0.1:0").and_then(|s| s.send_to(b"x", addr));
        report("udp", udp);
        report("lookup", "localhost:80".to_socket_addrs());
        Ok(())
    }
}

arcode_plugin::export_plugin!(Sockets);
