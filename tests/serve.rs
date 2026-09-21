mod common;

use common::*;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::process::Child;
use std::thread::sleep;
use std::time::{Duration, Instant};

fn detach(harness: &Harness, prompt: &str) -> String {
    let output = harness.run(&["--detach", "--harness", "claude", "--model", "opus", prompt]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    session_id_of(&stdout)
}

struct Server {
    child: Child,
    port: u16,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn port_from_serve_stdout(text: &str) -> Option<u16> {
    text.lines().find_map(|line| {
        let line = line.trim();
        line.strip_prefix("port:")
            .and_then(|value| value.trim().parse().ok())
    })
}

fn start_server(harness: &Harness) -> Server {
    let mut child = harness.spawn(&["serve", "--port", "0"]);
    let stdout = child.stdout.as_mut().expect("serve stdout");
    let mut buf = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let port = loop {
        assert!(
            Instant::now() < deadline,
            "serve never printed a port: {}",
            String::from_utf8_lossy(&buf)
        );
        let mut chunk = [0u8; 256];
        match stdout.read(&mut chunk) {
            Ok(0) => panic!(
                "serve exited before printing a port: {}",
                String::from_utf8_lossy(&buf)
            ),
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if let Some(port) = port_from_serve_stdout(&String::from_utf8_lossy(&buf)) {
                    break port;
                }
            }
            Err(error) => panic!("reading serve stdout: {error}"),
        }
    };
    let ready = Instant::now() + Duration::from_secs(2);
    while Instant::now() < ready {
        if TcpStream::connect(("127.0.0.1", port)).is_ok() {
            break;
        }
        sleep(Duration::from_millis(10));
    }
    Server { child, port }
}

fn http(port: u16, method: &str, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
    let request =
        format!("{method} {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).expect("write");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read");
    let status = response
        .split_whitespace()
        .nth(1)
        .expect("status")
        .parse()
        .expect("status code");
    let body = response
        .split("\r\n\r\n")
        .nth(1)
        .unwrap_or_default()
        .to_string();
    (status, body)
}

fn json_body(body: &str) -> Value {
    serde_json::from_str(body).unwrap_or_else(|error| panic!("{error}: {body}"))
}

#[test]
fn help_names_serve() {
    let harness = Harness::new();
    let output = harness.run(&["--help"]);
    let stdout = stdout_of(&output);
    assert_eq!(output.status.code(), Some(0), "{}", stderr_of(&output));
    assert!(stdout.contains("serve"), "{stdout}");
}

#[test]
fn ps_status_and_outcome_are_json_and_mutations_are_refused() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.slow_harness(300);

    let id = detach(&harness, "review");
    let server = start_server(&harness);

    let (status, body) = http(server.port, "GET", "/ps");
    assert_eq!(status, 200, "{body}");
    let ps = json_body(&body);
    assert_eq!(ps["running"], 1, "{ps}");
    assert_eq!(ps["sessions"][0]["id"], id, "{ps}");
    assert_eq!(ps["sessions"][0]["state"], "running", "{ps}");
    assert_eq!(ps["sessions"][0]["harness"], "claude", "{ps}");
    assert_eq!(ps["sessions"][0]["model"], "opus", "{ps}");

    let (status, body) = http(server.port, "GET", &format!("/status/{id}"));
    assert_eq!(status, 200, "{body}");
    let running = json_body(&body);
    assert_eq!(running["id"], id, "{running}");
    assert_eq!(running["state"], "running", "{running}");
    assert_eq!(running["status"], "running", "{running}");

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let (status, body) = http(server.port, "GET", &format!("/status/{id}"));
    assert_eq!(status, 200, "{body}");
    let finished = json_body(&body);
    assert_eq!(finished["state"], "finished", "{finished}");
    assert_eq!(finished["status"], "ok", "{finished}");

    let recorded = harness.run(&["outcome", &id, "success", "--note", "looks good"]);
    assert_eq!(recorded.status.code(), Some(0), "{}", stderr_of(&recorded));

    let (status, body) = http(server.port, "GET", &format!("/outcome/{id}"));
    assert_eq!(status, 200, "{body}");
    let outcome = json_body(&body);
    assert_eq!(outcome["id"], id, "{outcome}");
    assert_eq!(outcome["verdict"], "success", "{outcome}");
    assert_eq!(outcome["verdictNote"], "looks good", "{outcome}");

    let (status, body) = http(server.port, "POST", &format!("/outcome/{id}"));
    assert_eq!(status, 405, "{body}");
    assert!(
        json_body(&body)["error"]
            .as_str()
            .unwrap()
            .contains("read-only"),
        "{body}"
    );

    let (status, body) = http(server.port, "DELETE", &format!("/status/{id}"));
    assert_eq!(status, 405, "{body}");

    let (status, body) = http(server.port, "GET", "/status/does-not-exist");
    assert_eq!(status, 404, "{body}");
}

#[test]
fn running_status_reports_the_last_activity_and_the_current_tool() {
    let mut harness = Harness::new();
    harness.use_fixture("tools");
    harness.hang_for(8, 3000);

    let id = detach(&harness, "review");
    let server = start_server(&harness);

    let deadline = Instant::now() + Duration::from_secs(15);
    let running = loop {
        let (status, body) = http(server.port, "GET", &format!("/status/{id}"));
        assert_eq!(status, 200, "{body}");
        let value = json_body(&body);
        let activity = value["lastActivity"].as_str().unwrap_or_default();
        if value["currentTool"] == "Read" && activity >= "2026-09-02T03:04:53.666Z" {
            break value;
        }
        assert!(Instant::now() < deadline, "{value}");
        sleep(Duration::from_millis(20));
    };
    assert_eq!(running["state"], "running", "{running}");

    let (status, body) = http(server.port, "GET", "/ps");
    assert_eq!(status, 200, "{body}");
    let ps = json_body(&body);
    assert!(ps["sessions"][0].get("currentTool").is_none(), "{ps}");
    assert!(ps["sessions"][0].get("lastActivity").is_none(), "{ps}");

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let (_, body) = http(server.port, "GET", &format!("/status/{id}"));
    let finished = json_body(&body);
    assert_eq!(finished["state"], "finished", "{finished}");
    assert!(finished.get("currentTool").is_none(), "{finished}");
    assert!(finished.get("lastActivity").is_none(), "{finished}");
}

#[test]
fn a_failed_session_keeps_its_stderr_out_of_the_summary_and_the_http_status() {
    let mut harness = Harness::new();
    harness.fail_with("7");
    let id = detach(&harness, "hello");
    let waited = harness.run(&["wait", &id]);
    let stdout = stdout_of(&waited);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));
    assert!(
        stdout.contains("stderrTail: \"fake claude failing on purpose with exit code 7\""),
        "{stdout}"
    );

    let summary =
        std::fs::read_to_string(harness.boxr_home().join("summary.jsonl")).expect("summary ledger");
    assert!(!summary.contains("failing on purpose"), "{summary}");

    let server = start_server(&harness);
    let (status, body) = http(server.port, "GET", &format!("/status/{id}"));
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("failing on purpose"), "{body}");
}
