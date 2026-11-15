//! Unix-socket protocol between the CLI, the shell plugin and the daemon (spec 3.1):
//! one JSON request line in, one JSON response line out; `{"cmd":"watch"}` turns the
//! connection into a state stream.

use crate::engine::{Request, Response};
use crate::events::Event;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Sender};

/// Everything the daemon's single-threaded loop reacts to.
pub enum Msg {
    Event(Event),
    Request(Request, Sender<Response>),
    Watch(UnixStream),
    Tick,
    ConfigChanged,
    Socket2Closed,
}

pub fn runtime_dir() -> PathBuf {
    PathBuf::from(std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| "/tmp".into()))
}

pub fn socket_path() -> PathBuf {
    runtime_dir().join("ymux.sock")
}

pub fn is_watch(line: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(line)
        .ok()
        .and_then(|v| v.get("cmd").and_then(|c| c.as_str()).map(|c| c == "watch"))
        .unwrap_or(false)
}

/// Sends one request and reads one response line.
pub fn send(req: &Request) -> Result<Response> {
    let mut stream = UnixStream::connect(socket_path()).context("connecting to the ymux daemon")?;
    let mut line = serde_json::to_string(req)?;
    line.push('\n');
    stream.write_all(line.as_bytes())?;
    let mut reply = String::new();
    BufReader::new(stream).read_line(&mut reply)?;
    serde_json::from_str(reply.trim()).context("reading the daemon's reply")
}

/// Opens a watch connection: returns a reader of state lines.
pub fn watch() -> Result<BufReader<UnixStream>> {
    let mut stream = UnixStream::connect(socket_path()).context("connecting to the ymux daemon")?;
    stream.write_all(b"{\"cmd\":\"watch\"}\n")?;
    Ok(BufReader::new(stream))
}

/// Accepts connections forever, forwarding each to the daemon loop.
pub fn serve(listener: UnixListener, tx: Sender<Msg>) {
    for stream in listener.incoming().flatten() {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let _ = handle_connection(stream, tx);
        });
    }
}

fn handle_connection(stream: UnixStream, tx: Sender<Msg>) -> Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if is_watch(&line) {
        tx.send(Msg::Watch(stream))?;
        return Ok(());
    }
    let response = match serde_json::from_str::<Request>(line.trim()) {
        Ok(Request::Shutdown) => {
            // reply before the daemon loop exits, or the client would see no answer
            (&stream).write_all(b"{\"ok\":true,\"message\":null}\n")?;
            let (rtx, _rrx) = channel();
            tx.send(Msg::Request(Request::Shutdown, rtx))?;
            return Ok(());
        }
        Ok(req) => {
            let (rtx, rrx) = channel();
            tx.send(Msg::Request(req, rtx))?;
            rrx.recv().unwrap_or_else(|_| Response::err("ymux: daemon stopped".into()))
        }
        Err(e) => Response::err(format!("ymux: bad request: {e}")),
    };
    let mut out = serde_json::to_string(&response)?;
    out.push('\n');
    (&stream).write_all(out.as_bytes())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::engine::Request;

    #[test]
    fn request_json_shape() {
        assert_eq!(serde_json::to_string(&Request::Move { n: 3, follow: false }).unwrap(), r#"{"cmd":"move","n":3,"follow":false}"#);
        assert_eq!(serde_json::to_string(&Request::Next).unwrap(), r#"{"cmd":"next"}"#);
    }

    #[test]
    fn watch_line_is_recognised() {
        assert!(super::is_watch(r#"{"cmd":"watch"}"#));
        assert!(!super::is_watch(r#"{"cmd":"next"}"#));
    }
}
