//! The HTTP loop: a listening socket whose connections are each read and written on a thread
//! of their own, every page made on the thread that runs the loop.

use std::fmt::Write as _;
use std::io::{self, ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use super::{Page, Problem};
use crate::app::Failure;
use crate::domain::ports::StoreError;

fn names_this_server(host: &str, names: &[String]) -> bool {
    let (name, port) = match host.strip_prefix('[') {
        Some(bracketed) => {
            let Some((address, port)) = bracketed.split_once(']') else {
                return false;
            };
            if address.parse::<Ipv6Addr>().is_err() {
                return false;
            }
            (
                None,
                port.strip_prefix(':').or(port.is_empty().then_some("0")),
            )
        }
        None => match host.split_once(':') {
            Some((name, port)) => (Some(name), Some(port)),
            None => (Some(host), Some("0")),
        },
    };
    let known = |name: &str| {
        name.parse::<Ipv4Addr>().is_ok()
            || name.eq_ignore_ascii_case("localhost")
            || names.iter().any(|own| own.eq_ignore_ascii_case(name))
    };
    let numbered = |port: &str| {
        !port.is_empty()
            && port.bytes().all(|byte| byte.is_ascii_digit())
            && port.parse::<u16>().is_ok()
    };
    port.is_some_and(numbered) && name.is_none_or(known)
}

const CONNECTIONS: usize = 32;
// Room past the longest target for whatever headers a browser sends.
const LONGEST_HEAD: usize = super::LONGEST_TARGET + 56 * 1024;
const READING: Duration = Duration::from_secs(10);
const WRITING: Duration = Duration::from_secs(30);
// More than a browser sends.
const HEADERS: usize = 64;

const ON_EVERY_RESPONSE: [(&str, &str); 5] = [
    ("Content-Type", "text/html; charset=utf-8"),
    ("X-Content-Type-Options", "nosniff"),
    ("Cache-Control", "no-store"),
    ("Referrer-Policy", "no-referrer"),
    (
        "Content-Security-Policy",
        "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; \
         base-uri 'none'; frame-ancestors 'none'",
    ),
];

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        421 => "Misdirected Request",
        500 => "Internal Server Error",
        _ => "",
    }
}

struct Head {
    method: String,
    target: String,
    hosts: Vec<String>,
}

impl Head {
    fn is_head(&self) -> bool {
        self.method == "HEAD"
    }
}

enum Arrived {
    Head(Head),
    Malformed(String),
    // The client closed, stalled past its deadline or failed before its head was whole.
    Nothing,
}

// The head once it is whole, `None` while more of it is to come.
fn parsed(bytes: &[u8]) -> Option<Result<Head, String>> {
    let mut headers = [httparse::EMPTY_HEADER; HEADERS];
    let mut request = httparse::Request::new(&mut headers);
    match request.parse(bytes) {
        Ok(httparse::Status::Partial) => None,
        Ok(httparse::Status::Complete(_)) => Some(Ok(Head {
            method: request.method.unwrap_or_default().to_owned(),
            target: request.path.unwrap_or_default().to_owned(),
            hosts: (request.headers.iter())
                .filter(|header| header.name.eq_ignore_ascii_case("host"))
                .map(|header| String::from_utf8_lossy(header.value).into_owned())
                .collect(),
        })),
        Err(error) => Some(Err(format!("the request is not HTTP/1: {error}"))),
    }
}

fn arrived(stream: &mut TcpStream, deadline: Instant) -> Arrived {
    let mut bytes = Vec::new();
    let mut piece = [0; 4096];
    loop {
        if let Some(head) = parsed(&bytes) {
            return head.map_or_else(Arrived::Malformed, Arrived::Head);
        }
        if bytes.len() > LONGEST_HEAD {
            return Arrived::Malformed("the request is too long".to_owned());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() || stream.set_read_timeout(Some(left)).is_err() {
            return Arrived::Nothing;
        }
        match stream.read(&mut piece) {
            Ok(0) => return Arrived::Nothing,
            Ok(read) => bytes.extend_from_slice(&piece[..read]),
            Err(error) if error.kind() == ErrorKind::Interrupted => {}
            Err(_) => return Arrived::Nothing,
        }
    }
}

fn write_page(stream: &mut TcpStream, page: &Page, only_head: bool, writing: Duration) {
    let mut head = format!("HTTP/1.1 {} {}\r\n", page.status, reason(page.status));
    for (name, value) in ON_EVERY_RESPONSE {
        let _ = write!(head, "{name}: {value}\r\n");
    }
    if page.status == 405 {
        head.push_str("Allow: GET, HEAD\r\n");
    }
    let _ = write!(
        head,
        "Content-Length: {}\r\nConnection: close\r\n\r\n",
        page.html.len()
    );
    let body = if only_head { "" } else { page.html.as_str() };
    // A client that hung up or stopped reading leaves nothing to answer.
    let _ = (stream.set_write_timeout(Some(writing)))
        .and_then(|()| stream.write_all(head.as_bytes()))
        .and_then(|()| stream.write_all(body.as_bytes()))
        .and_then(|()| stream.shutdown(Shutdown::Write));
}

enum Event {
    Asked {
        method: String,
        target: String,
        page: mpsc::Sender<Page>,
    },
    Written,
    Failed(io::Error),
}

#[derive(Clone)]
struct Limits {
    hosts: Arc<[String]>,
    // How long a request's head may take to arrive, and how long one write of a page may block.
    reading: Duration,
    writing: Duration,
}

fn connection(stream: &mut TcpStream, limits: &Limits, events: &mpsc::Sender<Event>) {
    let (page, only_head) = match arrived(stream, Instant::now() + limits.reading) {
        Arrived::Nothing => return,
        Arrived::Malformed(why) => (Problem::BadRequest(why).page(), false),
        Arrived::Head(head) => {
            let only_head = head.is_head();
            if matches!(head.hosts.as_slice(), [host] if names_this_server(host, &limits.hosts)) {
                let (page, made) = mpsc::channel();
                let asked = Event::Asked {
                    method: head.method,
                    target: head.target,
                    page,
                };
                // Both fail only once `run` has returned.
                let Ok(()) = events.send(asked) else { return };
                let Ok(page) = made.recv() else { return };
                (page, only_head)
            } else {
                (Problem::Misdirected.page(), only_head)
            }
        }
    };
    write_page(stream, &page, only_head, limits.writing);
    let _ = events.send(Event::Written);
}

#[derive(Default)]
struct Slots {
    open: Mutex<usize>,
    freed: Condvar,
}

struct Slot(Arc<Slots>);

impl Slots {
    // Blocks while `CONNECTIONS` slots are taken.
    fn take(slots: &Arc<Slots>) -> Slot {
        let mut open = slots.open.lock().unwrap_or_else(PoisonError::into_inner);
        while *open >= CONNECTIONS {
            open = (slots.freed.wait(open)).unwrap_or_else(PoisonError::into_inner);
        }
        *open += 1;
        Slot(Arc::clone(slots))
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut open = (self.0.open.lock()).unwrap_or_else(PoisonError::into_inner);
        *open -= 1;
        self.0.freed.notify_one();
    }
}

fn accept(
    listener: &TcpListener,
    limits: &Limits,
    events: &mpsc::Sender<Event>,
    stop: &AtomicBool,
) {
    let slots = Arc::new(Slots::default());
    loop {
        // A connection past the bound waits in the listening socket's queue.
        let slot = Slots::take(&slots);
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            // A client that gave up before it was accepted.
            Err(error)
                if matches!(
                    error.kind(),
                    ErrorKind::ConnectionAborted
                        | ErrorKind::ConnectionReset
                        | ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(error) => {
                let _ = events.send(Event::Failed(error));
                return;
            }
        };
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let _ = stream.set_nodelay(true);
        let (limits, events) = (limits.clone(), events.clone());
        // A thread that cannot start closes its connection unanswered.
        let _ = thread::Builder::new().spawn(move || {
            let _slot = slot;
            connection(&mut stream, &limits, &events);
        });
    }
}

/// A bound socket; `run` answers its connections.
pub struct Server {
    listener: TcpListener,
    address: SocketAddr,
    limits: Limits,
}

impl Server {
    /// `name` as one of the `hosts` of `bind`, or why it is refused: it is empty, or holds a
    /// `:`, a `/` or whitespace. A request's `Host` is compared without its port.
    pub fn host_name(name: &str) -> Result<String, String> {
        if name.is_empty() {
            return Err("a host name is not empty".to_owned());
        }
        if name.contains([':', '/']) || name.contains(char::is_whitespace) {
            return Err("a host name holds no port, path or space".to_owned());
        }
        Ok(name.to_owned())
    }

    /// `hosts` are the names, beside `localhost` and any IP address, a request's `Host` may
    /// give for this server; one `host_name` refuses is a usage failure.
    pub fn bind(address: &str, hosts: &[String]) -> Result<Server, Failure> {
        let hosts = (hosts.iter())
            .map(|name| Server::host_name(name).map_err(|why| format!("{name}: {why}")))
            .collect::<Result<Vec<String>, String>>()
            .map_err(Failure::Usage)?;
        let unbound = |error| Failure::Store(StoreError::io(address, error));
        let listener = TcpListener::bind(address).map_err(unbound)?;
        let bound = listener.local_addr().map_err(unbound)?;
        Ok(Server {
            listener,
            address: bound,
            limits: Limits {
                hosts: hosts.into(),
                reading: READING,
                writing: WRITING,
            },
        })
    }

    /// Where it listens, with the port the system chose for a `:0`.
    #[must_use]
    pub fn address(&self) -> String {
        self.address.to_string()
    }

    // The address a connection reaches this server at, the loopback in place of an
    // unspecified one.
    fn reachable(&self) -> SocketAddr {
        let mut address = self.address;
        if address.ip().is_unspecified() {
            address.set_ip(match address {
                SocketAddr::V4(_) => Ipv4Addr::LOCALHOST.into(),
                SocketAddr::V6(_) => Ipv6Addr::LOCALHOST.into(),
            });
        }
        address
    }

    /// Answers each connection's one request and closes it: a request whose `Host` does not
    /// name this server is a 421 page, one that is not HTTP/1 a 400, any other gets the page
    /// `answer` returns for its method and target, a 500 page when `answer` panics, with no
    /// body for a `HEAD`. Each connection is read and written on a thread of its own, its head
    /// given `READING` to arrive and each write of its page `WRITING`, and closed unanswered
    /// when its head does not arrive; `answer` runs on the calling thread. While `CONNECTIONS`
    /// are open no other is accepted, and the system holds new ones until one closes. Returns
    /// once `requests` of them are answered and written, and with `None` only when the socket
    /// fails.
    pub fn run(
        &self,
        requests: Option<usize>,
        answer: &dyn Fn(&str, &str) -> Page,
    ) -> io::Result<()> {
        let (events, received) = mpsc::channel();
        let stop = Arc::new(AtomicBool::new(false));
        let accepting = {
            let listener = self.listener.try_clone()?;
            let limits = self.limits.clone();
            let stop = Arc::clone(&stop);
            move || accept(&listener, &limits, &events, &stop)
        };
        thread::Builder::new().spawn(accepting)?;
        let mut written = 0;
        let ended = loop {
            if requests.is_some_and(|limit| written >= limit) {
                break Ok(());
            }
            match received.recv() {
                Ok(Event::Asked {
                    method,
                    target,
                    page,
                }) => {
                    let asked = AssertUnwindSafe(|| answer(&method, &target));
                    let made = catch_unwind(asked).unwrap_or_else(|_| {
                        Problem::Broken("the request could not be answered".to_owned()).page()
                    });
                    // A client that hung up has no use for its page.
                    let _ = page.send(made);
                }
                Ok(Event::Written) => written += 1,
                Ok(Event::Failed(error)) => break Err(error),
                Err(_) => break Err(io::Error::other("the loop accepting connections stopped")),
            }
        };
        stop.store(true, Ordering::SeqCst);
        // A connection wakes the accept loop to find `stop` set.
        let _ = TcpStream::connect_timeout(&self.reachable(), Duration::from_secs(1));
        ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::{World, fact, fact_fields, world, world_with_atlas};
    use crate::web::respond;
    use crate::web::tests::{has, lacks};

    struct Serving {
        address: String,
        ended: mpsc::Receiver<io::Result<()>>,
    }

    impl Serving {
        fn start(
            hosts: &[String],
            run: impl FnOnce(&Server) -> io::Result<()> + Send + 'static,
        ) -> Serving {
            Serving::start_with(hosts, |_| {}, run)
        }

        fn start_with(
            hosts: &[String],
            setup: impl FnOnce(&mut Server),
            run: impl FnOnce(&Server) -> io::Result<()> + Send + 'static,
        ) -> Serving {
            let mut server = Server::bind("127.0.0.1:0", hosts).expect("a free port");
            setup(&mut server);
            let (sent, ended) = mpsc::channel();
            let address = server.address();
            thread::spawn(move || sent.send(run(&server)));
            Serving { address, ended }
        }

        fn done(self) {
            self.done_within(Duration::from_mins(1));
        }

        fn done_within(self, wait: Duration) {
            self.ended
                .recv_timeout(wait)
                .expect("a server that returned in time without panicking")
                .expect("a socket that kept answering");
        }
    }

    fn exchange(address: &str, request: &str) -> (String, String) {
        let mut stream = TcpStream::connect(address).expect("a connection");
        stream
            .set_read_timeout(Some(Duration::from_secs(10)))
            .expect("a timeout");
        stream.write_all(request.as_bytes()).expect("a request");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).expect("an answer");
        let (head, body) = answer.split_once("\r\n\r\n").expect("a head and a body");
        (head.to_lowercase(), body.to_owned())
    }

    fn request(address: &str, line: &str) -> (String, String) {
        let (head, body) = exchange(
            address,
            &format!(
                "{line} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
            ),
        );
        secured(line, &head);
        (head, body)
    }

    fn secured(line: &str, head: &str) {
        for header in [
            "content-type: text/html; charset=utf-8",
            "x-content-type-options: nosniff",
            "cache-control: no-store",
            "referrer-policy: no-referrer",
            "content-security-policy: default-src 'none'; style-src 'unsafe-inline'; \
             form-action 'self'; base-uri 'none'; frame-ancestors 'none'",
        ] {
            assert_eq!(
                head.matches(header).count(),
                1,
                "{line}: {header} in {head}"
            );
        }
        assert_eq!(head.matches("content-type:").count(), 1, "{head}");
    }

    #[test]
    fn the_server_answers_a_get_and_a_head_and_refuses_a_post() {
        let serving = Serving::start(&[], move |server| {
            let (world, lantern) = world();
            fact(&world, &lantern, "relay-pin", "");
            server.run(Some(5), &|method, target| {
                respond(&world.deps().read(), method, target)
            })
        });
        let address = serving.address.clone();
        assert!(address.starts_with("127.0.0.1:"), "{address}");
        assert!(!address.ends_with(":0"), "{address}");

        let (head, body) = request(&address, "GET /facts?ended=1");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        has(&body, "lantern/relay-pin");
        has(&head, &format!("content-length: {}", body.len()));
        lacks(&head, "allow:");

        let (head_only, nothing) = request(&address, "HEAD /facts?ended=1");
        assert!(head_only.starts_with("http/1.1 200 "), "{head_only}");
        has(&head_only, &format!("content-length: {}", body.len()));
        assert_eq!(nothing, "");

        let (refused, page) = request(&address, "POST /facts");
        assert!(refused.starts_with("http/1.1 405 "), "{refused}");
        assert_eq!(refused.matches("allow: get, head").count(), 1, "{refused}");
        has(&page, "<h1>405</h1>");

        let (missing, _) = request(&address, "GET /nowhere");
        assert!(missing.starts_with("http/1.1 404 "), "{missing}");
        lacks(&missing, "allow:");

        let (bare, page) = exchange(&address, "GET /facts HTTP/1.0\nHost: localhost\n\n");
        assert!(bare.starts_with("http/1.1 200 "), "{bare}");
        has(&page, "lantern/relay-pin");

        serving.done();
    }

    #[test]
    fn a_request_whose_answer_panics_is_a_500_and_the_next_is_answered() {
        let serving = Serving::start(&[], move |server| {
            let (world, _) = world();
            server.run(Some(2), &|method, target| {
                assert_ne!(target, "/facts", "the relay pin is secret");
                respond(&world.deps().read(), method, target)
            })
        });
        let address = serving.address.clone();

        let (head, body) = request(&address, "GET /facts");
        assert!(head.starts_with("http/1.1 500 "), "{head}");
        has(&body, "<h1>500</h1>");
        has(&body, "the request could not be answered");
        lacks(&body, "secret");

        let (head, body) = request(&address, "GET /");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        has(&body, ">lantern</a>");

        serving.done();
    }

    #[test]
    fn a_head_of_a_large_page_has_its_length_and_no_body() {
        let serving = Serving::start(&[], move |server| {
            let (world, lantern) = world();
            let summary = "relay ".repeat(8_000);
            world.put(
                "fact",
                &fact_fields(&lantern, "relay-pin", &summary, ""),
                "\n",
            );
            server.run(Some(2), &|method, target| {
                respond(&world.deps().read(), method, target)
            })
        });
        let address = serving.address.clone();

        let (head, body) = request(&address, "GET /facts");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        has(&head, &format!("content-length: {}", body.len()));
        lacks(&head, "transfer-encoding:");
        assert!(body.len() > 40_000, "{}", body.len());
        has(&body, "lantern/relay-pin");

        let (head_only, nothing) = request(&address, "HEAD /facts");
        assert!(head_only.starts_with("http/1.1 200 "), "{head_only}");
        has(&head_only, &format!("content-length: {}", body.len()));
        assert_eq!(nothing, "");

        serving.done();
    }

    #[test]
    fn a_host_names_this_server_when_it_is_an_address_localhost_or_a_name_it_was_given() {
        let none: [String; 0] = [];
        let desk = ["desk.local".to_owned(), "Atlas".to_owned()];
        for host in [
            "127.0.0.1:8080",
            "127.0.0.1",
            "192.168.7.20:80",
            "localhost",
            "localhost:8080",
            "LOCALHOST",
            "LocalHost:65535",
            "[::1]:8080",
            "[::1]",
            "[fe80::7]:8080",
        ] {
            assert!(names_this_server(host, &none), "{host}");
        }
        for host in [
            "desk.local",
            "DESK.Local:8080",
            "atlas",
            "localhost",
            "[::1]:1",
        ] {
            assert!(names_this_server(host, &desk), "{host}");
        }
        for host in [
            "desk.local",
            "evil.example",
            "evil.example:8080",
            "",
            ":8080",
            "127.0.0.1.evil.example",
            "localhost.evil.example",
            "user@localhost",
            "user:pass@localhost:8080",
            "localhost@evil.example",
            "localhost/path",
            "localhost:8080/path",
            "http://localhost",
            "localhost.",
            "127.0.0.1.",
            "localhost:",
            "localhost:http",
            "localhost:99999",
            "localhost:8080:8080",
            " localhost",
            "localhost ",
            "::1",
            "[::1",
            "[::1]8080",
            "[::1]:",
            "[localhost]",
            "[127.0.0.1]",
            "127.1",
            "localhost,evil.example",
        ] {
            assert!(!names_this_server(host, &none), "{host}");
        }
        for host in [
            "desk.local.",
            "desk.local.evil.example",
            "xdesk.local",
            "desk",
        ] {
            assert!(!names_this_server(host, &desk), "{host}");
        }
    }

    #[test]
    fn a_head_is_its_method_its_target_and_every_host_once_it_is_whole() {
        let head = parsed(b"GET /x?y HTTP/1.1\r\nHOST: desk\r\nAccept: */*\r\nhost:b\r\n\r\nrest")
            .expect("a whole head")
            .expect("a head");
        assert_eq!(head.method, "GET");
        assert_eq!(head.target, "/x?y");
        assert_eq!(head.hosts, ["desk", "b"]);
        assert!(!head.is_head());
        let bare = parsed(b"\r\n\nHEAD / HTTP/1.0\n\n").expect("a whole head");
        assert!(bare.expect("a head").is_head());
        for partial in [
            &b""[..],
            b"GET / HTTP/1.1\r\nHost: a\r\n",
            b"GET / HTTP/1.1\r\nHost: a\r\n\r",
        ] {
            assert!(
                parsed(partial).is_none(),
                "{}",
                String::from_utf8_lossy(partial)
            );
        }
        for broken in [
            &b"hello\r\n\r\n"[..],
            b"GET / HTTP/2\r\n\r\n",
            b"GET  / HTTP/1.1\r\n\r\n",
            b"GET / HTTP/1.1\r\nHo st: a\r\n\r\n",
            b"GET / HTTP/1.1\r\nHost a\r\n\r\n",
        ] {
            let why = parsed(broken).expect("a whole head").err();
            let why = why.expect("a head that does not parse");
            assert!(why.starts_with("the request is not HTTP/1: "), "{why}");
        }
    }

    #[test]
    fn a_name_no_host_header_can_give_is_refused_before_the_socket_is_bound() {
        for name in ["desk.local:8080", "http://desk.local", "desk local", ""] {
            let Err(failure) = Server::bind("127.0.0.1:0", &[name.to_owned()]) else {
                panic!("{name:?}: bound");
            };
            assert!(matches!(&failure, Failure::Usage(_)), "{name:?}: {failure}");
            assert!(
                failure
                    .to_string()
                    .starts_with(&format!("{name}: a host name ")),
                "{failure}"
            );
        }
    }

    #[test]
    fn a_request_for_another_host_is_misdirected() {
        let serving = Serving::start(&["desk.local".to_owned()], move |server| {
            let (world, _) = world();
            server.run(Some(6), &|method, target| {
                respond(&world.deps().read(), method, target)
            })
        });
        let address = serving.address.clone();
        let asked = |hosts: &[&str]| {
            let hosts: Vec<String> = (hosts.iter())
                .map(|host| format!("Host: {host}\r\n"))
                .collect();
            let hosts = hosts.concat();
            let (head, body) = exchange(
                &address,
                &format!("GET / HTTP/1.1\r\n{hosts}Connection: close\r\n\r\n"),
            );
            secured("GET /", &head);
            (head, body)
        };

        for host in ["desk.local:8080", "LOCALHOST", address.as_str()] {
            let (head, body) = asked(&[host]);
            assert!(head.starts_with("http/1.1 200 "), "{host}: {head}");
            has(&body, ">lantern</a>");
        }
        for hosts in [&["evil.example"][..], &[], &["desk.local", "evil.example"]] {
            let (head, body) = asked(hosts);
            assert!(head.starts_with("http/1.1 421 "), "{hosts:?}: {head}");
            has(&body, "<h1>421</h1>");
            lacks(&body, "evil.example");
            lacks(&body, ">lantern</a>");
        }

        serving.done();
    }

    const SMALL_BUFFER: usize = 4096;
    const LARGE_PAGE: usize = 250_000;

    // A socket the server accepts takes its buffer sizes from the listening one.
    fn shrunk(server: &mut Server) {
        rustix::net::sockopt::set_socket_send_buffer_size(&server.listener, SMALL_BUFFER)
            .expect("a send buffer");
    }

    fn with_a_large_page() -> World {
        let (world, lantern, atlas) = world_with_atlas();
        fact(&world, &lantern, "relay-pin", "");
        let summary = "relay ".repeat(LARGE_PAGE / 5);
        world.put("fact", &fact_fields(&atlas, "map", &summary, ""), "\n");
        world
    }

    fn asking_and_not_reading(address: &str, target: &str) -> TcpStream {
        let mut stream = TcpStream::connect(address).expect("a connection");
        rustix::net::sockopt::set_socket_recv_buffer_size(&stream, SMALL_BUFFER)
            .expect("a receive buffer");
        let request =
            format!("GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("a request");
        stream
    }

    #[test]
    fn a_client_that_does_not_read_its_page_does_not_keep_the_next_from_its_own() {
        let (asked, answers) = mpsc::channel();
        let serving = Serving::start_with(&[], shrunk, move |server| {
            let world = with_a_large_page();
            server.run(Some(2), &|method, target| {
                let page = respond(&world.deps().read(), method, target);
                asked.send(page.html.len()).expect("a test still listening");
                page
            })
        });
        let address = serving.address.clone();

        let slow = asking_and_not_reading(&address, "/facts?topic=atlas");
        let size = answers
            .recv_timeout(Duration::from_mins(1))
            .expect("the large page");
        assert!(size > LARGE_PAGE, "{size}");
        let (head, body) = request(&address, "GET /facts?topic=lantern");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        has(&body, "lantern/relay-pin");

        drop(slow);
        serving.done();
    }

    #[test]
    #[cfg_attr(
        target_os = "macos",
        ignore = "on macOS a request past the bound is answered at once"
    )]
    fn past_the_bound_on_open_connections_a_request_waits_for_one_to_close() {
        let (asked, answers) = mpsc::channel();
        let serving = Serving::start_with(&[], shrunk, move |server| {
            let world = with_a_large_page();
            let large = respond(&world.deps().read(), "GET", "/facts?topic=atlas").html;
            server.run(Some(CONNECTIONS + 1), &|_, _| {
                asked.send(()).expect("a test still listening");
                Page {
                    status: 200,
                    html: large.clone(),
                }
            })
        });
        let address = serving.address.clone();

        let mut slow: Vec<TcpStream> = (0..CONNECTIONS)
            .map(|_| asking_and_not_reading(&address, "/facts?topic=atlas"))
            .collect();
        for _ in 0..CONNECTIONS {
            answers
                .recv_timeout(Duration::from_mins(1))
                .expect("a page for each client that does not read");
        }
        let (waited, answered) = mpsc::channel();
        let next = address.clone();
        thread::spawn(move || waited.send(request(&next, "GET /")));
        assert!(
            answered.recv_timeout(Duration::from_millis(300)).is_err(),
            "a request answered past the bound"
        );
        assert!(answers.try_recv().is_err());

        drop(slow.pop());
        let (head, _) = answered
            .recv_timeout(Duration::from_mins(1))
            .expect("an answer once a connection closed");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        drop(slow);
        serving.done();
    }

    #[test]
    fn a_client_that_sends_no_whole_head_in_time_is_closed_and_the_others_are_answered() {
        let serving = Serving::start_with(
            &[],
            |server| server.limits.reading = Duration::from_millis(200),
            move |server| {
                let (world, _) = world();
                server.run(Some(1), &|method, target| {
                    respond(&world.deps().read(), method, target)
                })
            },
        );
        let address = serving.address.clone();
        let silent = TcpStream::connect(&address).expect("a connection");
        let mut dripping = TcpStream::connect(&address).expect("a connection");
        dripping
            .write_all(b"GET / HTTP/1.1\r\n")
            .expect("a request line");

        let (head, body) = request(&address, "GET /");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        has(&body, ">lantern</a>");
        for mut stalled in [silent, dripping] {
            stalled
                .set_read_timeout(Some(Duration::from_secs(10)))
                .expect("a timeout");
            let mut answer = Vec::new();
            let read = stalled
                .read_to_end(&mut answer)
                .expect("a connection closed");
            assert_eq!(read, 0, "{}", String::from_utf8_lossy(&answer));
        }

        serving.done();
    }

    #[test]
    fn a_client_that_stops_reading_its_page_is_closed_once_a_write_times_out() {
        let serving = Serving::start_with(
            &[],
            |server| {
                shrunk(server);
                server.limits.writing = Duration::from_millis(200);
            },
            move |server| {
                let world = with_a_large_page();
                server.run(Some(1), &|method, target| {
                    respond(&world.deps().read(), method, target)
                })
            },
        );
        let slow = asking_and_not_reading(&serving.address, "/facts?topic=atlas");
        serving.done_within(Duration::from_secs(20));
        drop(slow);
    }

    #[test]
    fn a_request_that_is_not_http_1_is_a_bad_request() {
        let serving = Serving::start(&[], move |server| {
            let (world, _) = world();
            server.run(Some(4), &|method, target| {
                respond(&world.deps().read(), method, target)
            })
        });
        let address = serving.address.clone();
        for asked in [
            "hello\r\n\r\n",
            "GET / HTTP/2\r\nHost: localhost\r\n\r\n",
            "GET  / HTTP/1.1\r\nHost: localhost\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost\r\nno colon\r\n\r\n",
        ] {
            let (head, body) = exchange(&address, asked);
            assert!(head.starts_with("http/1.1 400 "), "{asked:?}: {head}");
            secured(asked, &head);
            has(&body, "the request is not HTTP/1: ");
        }

        serving.done();
    }

    #[test]
    fn an_address_nothing_can_listen_on_is_a_store_failure_naming_it() {
        let Err(failure) = Server::bind("not an address", &[]) else {
            panic!("an address nothing can listen on");
        };
        assert!(matches!(&failure, Failure::Store(_)), "{failure}");
        assert!(
            failure.to_string().starts_with("not an address: "),
            "{failure}"
        );
    }
}
