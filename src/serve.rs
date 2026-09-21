use crate::clock;
use crate::detached::{self, State};
use crate::home;
use crate::ledger;
use crate::output::Toon;
use crate::report;
use crate::session::Session;
use anyhow::{Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::thread;
use std::time::Duration;

pub const DEFAULT_PORT: u16 = 4035;

#[derive(Serialize)]
struct SessionRow {
    id: String,
    state: String,
    harness: String,
    model: String,
}

pub fn run(port: u16) -> Result<i32> {
    let home = home::boxr_home()?;
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    let listener = TcpListener::bind(addr)
        .with_context(|| format!("binding the ledger HTTP server on {addr}"))?;
    listener
        .set_nonblocking(false)
        .context("configuring the ledger HTTP listener")?;
    let bound = listener
        .local_addr()
        .context("reading the ledger HTTP listen address")?;
    print!("{}", render_listen(bound));
    let _ = std::io::stdout().flush();
    loop {
        match listener.accept() {
            Ok((stream, _)) => {
                let home = home.clone();
                thread::spawn(move || {
                    if let Err(error) = handle_connection(&home, stream) {
                        let _ = error;
                    }
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return Err(anyhow::Error::from(error).context("accepting a ledger HTTP connection"))
            }
        }
    }
}

fn render_listen(addr: SocketAddr) -> String {
    let mut toon = Toon::new();
    toon.section("serve")
        .field("host", &addr.ip().to_string())
        .number("port", addr.port())
        .field("url", &format!("http://{addr}"));
    toon.list(
        "help",
        &[
            "GET /ps for running sessions".to_string(),
            "GET /status/<id> for one session".to_string(),
            "GET /outcome/<id> for verdict and git evidence".to_string(),
        ],
    );
    toon.render()
}

fn handle_connection(home: &Path, mut stream: TcpStream) -> Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_secs(5))).ok();
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).context("reading the HTTP request")?;
    if n == 0 {
        return Ok(());
    }
    let text = String::from_utf8_lossy(&buf[..n]);
    let request = match parse_request(&text) {
        Ok(request) => request,
        Err(message) => {
            write_response(&mut stream, 400, "application/json", &error_body(&message))?;
            return Ok(());
        }
    };
    let (status, body) = dispatch(home, &request);
    write_response(&mut stream, status, "application/json", &body)
}

struct Request {
    method: String,
    path: String,
}

fn parse_request(text: &str) -> std::result::Result<Request, String> {
    let line = text
        .lines()
        .next()
        .ok_or_else(|| "empty request".to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "missing method".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "missing path".to_string())?
        .to_string();
    if parts.next().is_none() {
        return Err("missing HTTP version".to_string());
    }
    let path = path.split('?').next().unwrap_or(path.as_str()).to_string();
    Ok(Request { method, path })
}

fn dispatch(home: &Path, request: &Request) -> (u16, String) {
    if request.method != "GET" && request.method != "HEAD" {
        return (
            405,
            error_body("ledger HTTP is read-only; only GET is allowed"),
        );
    }
    let path = request.path.as_str();
    let result = match path {
        "/" => Ok(json!({
            "service": "boxr",
            "readOnly": true,
            "endpoints": ["/ps", "/status/<id>", "/outcome/<id>"],
        })),
        "/ps" => ps_json(home),
        _ => {
            if let Some(id) = path.strip_prefix("/status/") {
                status_json(home, id)
            } else if let Some(id) = path.strip_prefix("/outcome/") {
                outcome_json(home, id)
            } else {
                Err((404, format!("unknown path `{path}`")))
            }
        }
    };
    match result {
        Ok(value) => (200, value.to_string()),
        Err((code, message)) => (code, error_body(&message)),
    }
}

fn ps_json(home: &Path) -> std::result::Result<Value, (u16, String)> {
    let mut sessions = Vec::new();
    let ids = Session::ids(home).map_err(|error| (500, format!("{error:#}")))?;
    for id in ids {
        let Ok(state) = detached::listing(home, &id) else {
            continue;
        };
        if let State::Running(running) = state {
            sessions.push(SessionRow {
                id,
                state: "running".to_string(),
                harness: running.launch.harness.clone(),
                model: running.launch.model.clone(),
            });
        }
    }
    Ok(json!({
        "running": sessions.len(),
        "sessions": sessions,
    }))
}

fn status_json(home: &Path, id: &str) -> std::result::Result<Value, (u16, String)> {
    let id = validate_id(id)?;
    match detached::state(home, id) {
        Ok(State::Running(running)) => Ok(json!({
            "id": id,
            "status": "running",
            "state": "running",
            "harness": running.launch.harness,
            "model": running.launch.model,
            "effort": running.launch.effort,
            "startedMillis": running.launch.started_millis,
            "lastActivity": clock::iso8601(running.last_activity_millis),
            "currentTool": running.current_tool,
            "steps": running.steps,
            "pid": running.pid,
        })),
        Ok(State::Finished(report)) => Ok(json!({
            "id": report.id,
            "status": report.status,
            "state": report::state_of(&report.status),
            "harness": report.harness,
            "model": report.model,
            "effort": report.effort,
            "profile": report.profile,
            "mode": report.mode,
            "resumedFrom": report.resumed_from,
            "kind": report.kind,
            "start": report.start,
            "end": report.end,
            "durationMs": report.duration_ms,
            "exitCode": report.exit_code,
            "steps": report.steps,
            "promptTokens": report.prompt_tokens,
            "completionTokens": report.completion_tokens,
            "cachedTokens": report.cached_tokens,
            "reasoningTokens": report.reasoning_tokens,
            "apiEquivalentCost": report.api_equivalent_cost,
            "currency": report.currency,
            "error": report.error,
            "limitHit": report.limit_hit,
            "interrupted": report.interrupted,
            "finalMessage": report.final_message,
        })),
        Err(error) => {
            let message = format!("{error:#}");
            if message.contains("no session") {
                Err((404, message))
            } else {
                Err((500, message))
            }
        }
    }
}

fn outcome_json(home: &Path, id: &str) -> std::result::Result<Value, (u16, String)> {
    let id = validate_id(id)?;
    let summary = ledger::read_summary(home, id).map_err(|error| (404, format!("{error:#}")))?;
    Ok(json!({
        "id": summary.id,
        "verdict": summary.verdict,
        "verdictNote": summary.verdict_note,
        "git": summary.git,
        "status": summary.status,
        "state": report::state_of(&summary.status),
    }))
}

fn validate_id(id: &str) -> std::result::Result<&str, (u16, String)> {
    if id.is_empty() || id.contains('/') || id.contains('\\') || id.contains("..") {
        return Err((400, "invalid session id".to_string()));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err((400, "invalid session id".to_string()));
    }
    Ok(id)
}

fn error_body(message: &str) -> String {
    json!({ "error": message }).to_string()
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(header.as_bytes())
        .context("writing the HTTP response headers")?;
    stream
        .write_all(body.as_bytes())
        .context("writing the HTTP response body")?;
    stream.flush().context("flushing the HTTP response")?;
    Ok(())
}
