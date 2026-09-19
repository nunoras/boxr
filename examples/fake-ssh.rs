use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("fake-ssh: host required");
        process::exit(2);
    }
    let host = &args[0];
    let command = args[1..].join(" ");
    if let Some(path) = env::var_os("BOXR_FAKE_SSH_ARGS") {
        use std::io::Write;
        let path = PathBuf::from(path);
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .expect("open ssh args log");
        let _ = writeln!(file, "{host}\n{command}");
    }
    if command.contains("--version") {
        let version = env::var("BOXR_FAKE_SSH_VERSION")
            .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string());
        if env::var_os("BOXR_FAKE_SSH_NO_BOXR").is_some() {
            eprintln!("bash: boxr: command not found");
            process::exit(127);
        }
        println!("boxr {version}");
        process::exit(0);
    }
    if env::var_os("BOXR_FAKE_SSH_FAIL_LAUNCH").is_some() {
        eprintln!("remote harness missing");
        process::exit(3);
    }
    let id = env::var("BOXR_FAKE_SSH_SESSION").unwrap_or_else(|_| "remote-session-1".to_string());
    println!("session:");
    println!("  id: {id}");
    println!("  status: running");
    println!("  harness: claude");
    println!("  model: opus");
    println!("  effort: harness-default");
    println!("  pid: 4242");
    println!("help[3]:");
    println!("  Run `boxr wait {id}` to block until it finishes");
    println!("  Run `boxr status {id}` to check on it");
    println!("  Run `boxr stop {id}` to end it");
}
