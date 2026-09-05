//! The binary serving a scratch store over a socket: the port it reports
//! is the one it answers on, and a page is what the reads say.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use assert_cmd::cargo::cargo_bin;

struct Served {
    child: Child,
    address: String,
}

impl Drop for Served {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn run(root: &Path, args: &[&str]) -> String {
    let out = Command::new(cargo_bin("worklog"))
        .env("WORKLOG_HOME", root)
        .env("HOME", root.join("home"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("the binary runs");
    assert!(
        out.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).expect("utf-8 stdout")
}

fn seeded(root: &Path) {
    fs::create_dir_all(root.join("home")).unwrap();
    run(root, &["init", "desk"]);
    let path = run(root, &["new", "topic", "lantern"]);
    let path = Path::new(path.trim());
    let text = fs::read_to_string(path).unwrap();
    fs::write(
        path,
        text.replace("summary:\n", "summary: A Rust app that dims a lamp\n"),
    )
    .unwrap();
    run(root, &["save", "lantern"]);
}

fn serve(root: &Path) -> Served {
    let mut child = Command::new(cargo_bin("worklog"))
        .env("WORKLOG_HOME", root)
        .env("HOME", root.join("home"))
        .current_dir(root)
        .args(["serve", "--bind", "127.0.0.1:0"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("the binary starts");
    let mut line = String::new();
    BufReader::new(child.stderr.take().expect("stderr is piped"))
        .read_line(&mut line)
        .expect("a line on stderr");
    let address = line
        .trim()
        .strip_prefix("worklog: serving on http://")
        .and_then(|rest| rest.strip_suffix('/'))
        .unwrap_or_else(|| panic!("the address is announced: {line:?}"))
        .to_owned();
    Served { child, address }
}

fn get(served: &Served, path: &str) -> (u16, String) {
    let mut stream = TcpStream::connect(&served.address).expect("the port answers");
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        served.address
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    let status = response
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .expect("a status line");
    let body = response
        .split_once("\r\n\r\n")
        .map(|(_, body)| body.to_owned())
        .unwrap_or_default();
    (status, body)
}

#[test]
fn the_served_pages_are_the_store() {
    let root = tempfile::tempdir().expect("a temp dir");
    seeded(root.path());
    let served = serve(root.path());

    let (status, body) = get(&served, "/");
    assert_eq!(status, 200);
    assert!(body.contains("A Rust app that dims a lamp"), "{body}");
    assert!(body.contains("href=\"/topic/lantern\""));

    let (status, body) = get(&served, "/topic/lantern");
    assert_eq!(status, 200);
    assert!(body.contains("<title>lantern"), "{body}");

    let (status, _) = get(&served, "/topic/nothing");
    assert_eq!(status, 404);

    let usage = run(root.path(), &["usage"]);
    assert!(
        usage.contains("serve"),
        "one usage line for the start: {usage}"
    );
}
