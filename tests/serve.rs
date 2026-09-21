mod common;

use common::*;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
    host: String,
    port: u16,
    token: Option<String>,
    listen: String,
    stderr: Option<std::process::ChildStderr>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

impl Server {
    fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }

    fn socket_host(&self) -> &str {
        match self.host.as_str() {
            "0.0.0.0" | "::" => "127.0.0.1",
            host => host,
        }
    }

    fn request(&self, method: &str, path: &str, authorization: Option<&str>) -> (u16, String) {
        let host = self.socket_host();
        let mut stream = TcpStream::connect((host, self.port)).expect("connect");
        stream.set_read_timeout(Some(Duration::from_secs(2))).ok();
        let mut request =
            format!("{method} {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n");
        if let Some(value) = authorization {
            request.push_str(&format!("Authorization: {value}\r\n"));
        }
        request.push_str("\r\n");
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

    fn authenticated(&self, method: &str, path: &str) -> (u16, String) {
        let value = self.token.as_ref().map(|token| format!("Bearer {token}"));
        self.request(method, path, value.as_deref())
    }

    fn stderr_text(&mut self) -> String {
        self.stop();
        let Some(mut stderr) = self.stderr.take() else {
            return String::new();
        };
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    }
}

fn field(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(name)
            .and_then(|rest| rest.strip_prefix(':'))
            .map(|value| value.trim().trim_matches('"').to_string())
    })
}

fn token_of(args: &[&str]) -> Option<String> {
    let index = args.iter().position(|arg| *arg == "--token")?;
    args.get(index + 1).map(|value| value.to_string())
}

fn port_from_serve_stdout(text: &str) -> Option<u16> {
    field(text, "port").and_then(|value| value.parse().ok())
}

fn start_serve(harness: &Harness, args: &[&str]) -> Server {
    let mut child = harness.spawn(args);
    let stdout = child.stdout.as_mut().expect("serve stdout");
    let mut buf = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(5);
    let listen = loop {
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
                let text = String::from_utf8_lossy(&buf).to_string();
                if port_from_serve_stdout(&text).is_some() {
                    break text;
                }
            }
            Err(error) => panic!("reading serve stdout: {error}"),
        }
    };
    let port = port_from_serve_stdout(&listen).expect("serve port");
    let host = field(&listen, "host").expect("serve host");
    let stderr = child.stderr.take();
    Server {
        child,
        host,
        port,
        token: token_of(args),
        listen,
        stderr,
    }
}

fn start_server(harness: &Harness) -> Server {
    start_server_with(harness, &["serve", "--port", "0"])
}

fn start_server_with(harness: &Harness, args: &[&str]) -> Server {
    let server = start_serve(harness, args);
    let socket_host = server.socket_host().to_string();
    let ready = Instant::now() + Duration::from_secs(2);
    while Instant::now() < ready {
        if TcpStream::connect((socket_host.as_str(), server.port)).is_ok() {
            break;
        }
        sleep(Duration::from_millis(10));
    }
    server
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

    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");
    let ps = json_body(&body);
    assert_eq!(ps["running"], 1, "{ps}");
    assert_eq!(ps["sessions"][0]["id"], id, "{ps}");
    assert_eq!(ps["sessions"][0]["state"], "running", "{ps}");
    assert_eq!(ps["sessions"][0]["harness"], "claude", "{ps}");
    assert_eq!(ps["sessions"][0]["model"], "opus", "{ps}");

    let (status, body) = server.authenticated("GET", &format!("/status/{id}"));
    assert_eq!(status, 200, "{body}");
    let running = json_body(&body);
    assert_eq!(running["id"], id, "{running}");
    assert_eq!(running["state"], "running", "{running}");
    assert_eq!(running["status"], "running", "{running}");

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let (status, body) = server.authenticated("GET", &format!("/status/{id}"));
    assert_eq!(status, 200, "{body}");
    let finished = json_body(&body);
    assert_eq!(finished["state"], "finished", "{finished}");
    assert_eq!(finished["status"], "ok", "{finished}");

    let recorded = harness.run(&["outcome", &id, "success", "--note", "looks good"]);
    assert_eq!(recorded.status.code(), Some(0), "{}", stderr_of(&recorded));

    let (status, body) = server.authenticated("GET", &format!("/outcome/{id}"));
    assert_eq!(status, 200, "{body}");
    let outcome = json_body(&body);
    assert_eq!(outcome["id"], id, "{outcome}");
    assert_eq!(outcome["verdict"], "success", "{outcome}");
    assert_eq!(outcome["verdictNote"], "looks good", "{outcome}");

    let (status, body) = server.authenticated("POST", &format!("/outcome/{id}"));
    assert_eq!(status, 405, "{body}");
    assert!(
        json_body(&body)["error"]
            .as_str()
            .unwrap()
            .contains("read-only"),
        "{body}"
    );

    let (status, body) = server.authenticated("DELETE", &format!("/status/{id}"));
    assert_eq!(status, 405, "{body}");

    let (status, body) = server.authenticated("GET", "/status/does-not-exist");
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
        let (status, body) = server.authenticated("GET", &format!("/status/{id}"));
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

    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");
    let ps = json_body(&body);
    assert!(ps["sessions"][0].get("currentTool").is_none(), "{ps}");
    assert!(ps["sessions"][0].get("lastActivity").is_none(), "{ps}");

    let waited = harness.run(&["wait", &id]);
    assert_eq!(waited.status.code(), Some(0), "{}", stderr_of(&waited));

    let (_, body) = server.authenticated("GET", &format!("/status/{id}"));
    let finished = json_body(&body);
    assert_eq!(finished["state"], "finished", "{finished}");
    assert!(finished.get("currentTool").is_none(), "{finished}");
    assert!(finished.get("lastActivity").is_none(), "{finished}");
}

#[test]
fn serve_binds_loopback_by_default() {
    let harness = Harness::new();
    let mut server = start_server(&harness);
    assert_eq!(field(&server.listen, "host").as_deref(), Some("127.0.0.1"));
    assert!(
        server.listen.contains("http://127.0.0.1:"),
        "{}",
        server.listen
    );

    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");

    let stderr = server.stderr_text();
    assert!(!stderr.contains("warning"), "{stderr}");
}

#[test]
fn serve_binds_an_explicit_ipv4_address() {
    let harness = Harness::new();
    let server = start_server_with(&harness, &["serve", "--bind", "127.0.0.1", "--port", "0"]);
    assert_eq!(field(&server.listen, "host").as_deref(), Some("127.0.0.1"));

    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");
}

#[test]
fn serve_binds_an_explicit_ipv6_address_when_the_host_supports_it() {
    if TcpListener::bind(("::1", 0)).is_err() {
        return;
    }
    let harness = Harness::new();
    let server = start_server_with(&harness, &["serve", "--bind", "::1", "--port", "0"]);
    assert_eq!(field(&server.listen, "host").as_deref(), Some("::1"));
    assert!(server.listen.contains("http://[::1]:"), "{}", server.listen);

    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");
}

#[test]
fn serve_refuses_a_bind_that_is_not_an_ip_address() {
    let harness = Harness::new();
    let output = harness.run(&["serve", "--bind", "example.com", "--port", "0"]);
    assert_eq!(output.status.code(), Some(2), "{}", stderr_of(&output));
    assert!(
        stderr_of(&output).contains("IPv4"),
        "{}",
        stderr_of(&output)
    );
}

#[test]
fn serve_rejects_an_empty_token() {
    let harness = Harness::new();
    let mut child = harness.spawn(&["serve", "--port", "0", "--token", ""]);
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            panic!("boxr serve accepted an empty token");
        }
        sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(2));
    let mut stderr = String::new();
    child
        .stderr
        .as_mut()
        .expect("stderr")
        .read_to_string(&mut stderr)
        .expect("read stderr");
    assert!(stderr.contains("token"), "{stderr}");
}

#[test]
fn serve_requires_the_bearer_token_on_every_route_and_method() {
    let harness = Harness::new();
    let server = start_server_with(&harness, &["serve", "--port", "0", "--token", "s3cret"]);

    let cases: Vec<(&str, String)> = vec![
        ("GET", "/".to_string()),
        ("GET", "/ps".to_string()),
        ("GET", "/status/s-abc-def".to_string()),
        ("GET", "/outcome/s-abc-def".to_string()),
        ("GET", "/unknown".to_string()),
        ("HEAD", "/ps".to_string()),
        ("POST", "/ps".to_string()),
        ("DELETE", "/status/s-abc-def".to_string()),
    ];
    for (method, path) in &cases {
        let (status, body) = server.request(method, path, None);
        assert_eq!(status, 401, "{method} {path} without a token: {body}");
        assert_eq!(
            json_body(&body)["error"],
            "authentication required",
            "{body}"
        );
        assert!(!body.contains("s-abc-def"), "{body}");

        let (status, body) = server.request(method, path, Some("Bearer wrong"));
        assert_eq!(status, 401, "{method} {path} with a wrong token: {body}");

        let (status, body) = server.request(method, path, Some("s3cret"));
        assert_eq!(status, 401, "{method} {path} without a scheme: {body}");
    }

    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");
    assert_eq!(json_body(&body)["running"], 0, "{body}");

    let (status, body) = server.authenticated("GET", "/status/s-abc-def");
    assert_eq!(status, 404, "{body}");

    let (status, body) = server.authenticated("GET", "/unknown");
    assert_eq!(status, 404, "{body}");

    let (status, body) = server.authenticated("POST", "/ps");
    assert_eq!(status, 405, "{body}");
}

#[test]
fn serve_warns_on_a_non_loopback_bind_without_a_token() {
    let harness = Harness::new();
    let mut server = start_serve(&harness, &["serve", "--bind", "0.0.0.0", "--port", "0"]);
    let stderr = server.stderr_text();
    assert!(stderr.contains("warning"), "{stderr}");
    assert!(stderr.contains("token"), "{stderr}");
}

#[test]
fn serve_never_echoes_the_token() {
    let harness = Harness::new();
    let mut server = start_serve(
        &harness,
        &[
            "serve",
            "--bind",
            "0.0.0.0",
            "--port",
            "0",
            "--token",
            "hunter2-secret",
        ],
    );
    let (status, body) = server.authenticated("GET", "/ps");
    assert_eq!(status, 200, "{body}");

    let stderr = server.stderr_text();
    assert!(
        !server.listen.contains("hunter2-secret"),
        "{}",
        server.listen
    );
    assert!(!stderr.contains("hunter2-secret"), "{stderr}");
    assert!(!stderr.contains("warning"), "{stderr}");
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
    let (status, body) = server.authenticated("GET", &format!("/status/{id}"));
    assert_eq!(status, 200, "{body}");
    assert!(!body.contains("failing on purpose"), "{body}");
}
