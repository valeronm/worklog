//! The HTTP loop: a bound socket whose requests are answered one at a time, each with the
//! page it is handed for its method and target.

use std::io::Cursor;
use std::net::{Ipv4Addr, Ipv6Addr};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};

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

fn header(name: &'static str, value: &'static str) -> tiny_http::Header {
    tiny_http::Header::from_bytes(name, value).expect("a header name and a value in ASCII")
}

const WRITERS: usize = 32;

struct Writing<'a>(&'a AtomicUsize);

impl<'a> Writing<'a> {
    fn begun(count: &'a AtomicUsize) -> Writing<'a> {
        count.fetch_add(1, Ordering::SeqCst);
        Writing(count)
    }
}

impl Drop for Writing<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// A bound socket; `run` answers one request at a time on the calling thread.
pub struct Server {
    inner: tiny_http::Server,
    hosts: Vec<String>,
    on_every_response: Vec<tiny_http::Header>,
    allow: tiny_http::Header,
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
        let inner = tiny_http::Server::http(address)
            .map_err(|error| Failure::Store(StoreError::io(address, error)))?;
        Ok(Server {
            inner,
            hosts,
            on_every_response: vec![
                header("Content-Type", "text/html; charset=utf-8"),
                header("X-Content-Type-Options", "nosniff"),
                header("Cache-Control", "no-store"),
                header("Referrer-Policy", "no-referrer"),
                header(
                    "Content-Security-Policy",
                    "default-src 'none'; style-src 'unsafe-inline'; form-action 'self'; \
                     base-uri 'none'; frame-ancestors 'none'",
                ),
            ],
            allow: header("Allow", "GET, HEAD"),
        })
    }

    /// Where it listens, with the port the system chose for a `:0`.
    #[must_use]
    pub fn address(&self) -> String {
        self.inner.server_addr().to_string()
    }

    fn page(&self, request: &tiny_http::Request, answer: &dyn Fn(&str, &str) -> Page) -> Page {
        let mut hosts = (request.headers().iter()).filter(|header| header.field.equiv("Host"));
        let named = match (hosts.next(), hosts.next()) {
            (Some(host), None) => names_this_server(host.value.as_str(), &self.hosts),
            _ => false,
        };
        if !named {
            return Problem::Misdirected.page();
        }
        let asked = AssertUnwindSafe(|| answer(request.method().as_str(), request.url()));
        catch_unwind(asked).unwrap_or_else(|_| {
            Problem::Broken("the request could not be answered".to_owned()).page()
        })
    }

    fn response(&self, page: Page) -> tiny_http::Response<Cursor<Vec<u8>>> {
        let mut response =
            tiny_http::Response::from_string(page.html).with_status_code(page.status);
        for header in &self.on_every_response {
            response.add_header(header.clone());
        }
        if page.status == 405 {
            response.add_header(self.allow.clone());
        }
        response
    }

    /// Answers one request at a time: a request whose `Host` does not name this server is a
    /// 421 page, any other gets the page `answer` returns for its method and target, a 500
    /// page when `answer` panics, with no body for a `HEAD`. Each page is written to its
    /// client on a thread of its own, and while `WRITERS` of them are being written a request
    /// is answered 503 whatever its `Host`, without calling `answer`. No write has a timeout:
    /// a client that stalls holds its writer, and one that stalls on a 503 holds `run`, which
    /// writes that page itself. Returns once `requests` of them are answered and written, and
    /// with `None` only when the socket fails.
    pub fn run(
        &self,
        requests: Option<usize>,
        answer: &dyn Fn(&str, &str) -> Page,
    ) -> std::io::Result<()> {
        let writing = AtomicUsize::new(0);
        std::thread::scope(|writers| {
            let mut answered = 0;
            while requests.is_none_or(|limit| answered < limit) {
                let request = self.inner.recv()?;
                answered += 1;
                if writing.load(Ordering::SeqCst) >= WRITERS {
                    // A client that hung up before the page was written leaves nothing to answer.
                    let _ = request.respond(self.response(Problem::Busy.page()));
                    continue;
                }
                let response = self.response(self.page(&request, answer));
                let slot = Writing::begun(&writing);
                // A thread that cannot start drops the request, which tiny_http answers 500.
                let _ = std::thread::Builder::new().spawn_scoped(writers, move || {
                    let _slot = slot;
                    let _ = catch_unwind(AssertUnwindSafe(|| request.respond(response)));
                });
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    use super::*;
    use crate::app::testing::{World, fact, fact_fields, world, world_with_atlas};
    use crate::domain::id::DocumentId;
    use crate::web::respond;
    use crate::web::tests::{has, lacks};

    struct Serving {
        address: String,
        thread: std::thread::JoinHandle<std::io::Result<()>>,
    }

    impl Serving {
        fn start(
            hosts: &[String],
            run: impl FnOnce(&Server) -> std::io::Result<()> + Send + 'static,
        ) -> Serving {
            let server = Server::bind("127.0.0.1:0", hosts).expect("a free port");
            Serving {
                address: server.address(),
                thread: std::thread::spawn(move || run(&server)),
            }
        }

        fn done(self) {
            self.thread
                .join()
                .expect("a server that did not panic")
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
            server.run(Some(4), &|method, target| {
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
    fn a_head_of_a_page_too_large_for_one_piece_has_its_headers_and_no_body() {
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
        has(&head, "transfer-encoding: chunked");
        lacks(&head, "content-length:");
        assert!(body.len() > 40_000, "{}", body.len());
        has(&body, "lantern/relay-pin");

        let (head_only, nothing) = request(&address, "HEAD /facts");
        assert!(head_only.starts_with("http/1.1 200 "), "{head_only}");
        has(&head_only, "transfer-encoding: chunked");
        lacks(&head_only, "content-length:");
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

    fn with_a_large_page() -> (World, DocumentId) {
        let (world, lantern, atlas) = world_with_atlas();
        fact(&world, &lantern, "relay-pin", "");
        let summary = "relay ".repeat(1_400_000);
        world.put("fact", &fact_fields(&atlas, "map", &summary, ""), "\n");
        (world, lantern)
    }

    fn asking_and_not_reading(address: &str, target: &str) -> TcpStream {
        let mut stream = TcpStream::connect(address).expect("a connection");
        let request =
            format!("GET {target} HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n");
        stream.write_all(request.as_bytes()).expect("a request");
        stream
    }

    #[test]
    fn a_client_that_does_not_read_its_page_does_not_keep_the_next_from_its_own() {
        let (asked, answers) = std::sync::mpsc::channel();
        let serving = Serving::start(&[], move |server| {
            let (world, _) = with_a_large_page();
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
        assert!(size > 8_000_000, "{size}");
        let (head, body) = request(&address, "GET /facts?topic=lantern");
        assert!(head.starts_with("http/1.1 200 "), "{head}");
        has(&body, "lantern/relay-pin");

        drop(slow);
        serving.done();
    }

    #[test]
    fn past_the_bound_on_responses_being_written_a_request_is_answered_503() {
        let (asked, answers) = std::sync::mpsc::channel();
        let serving = Serving::start(&[], move |server| {
            let (world, _) = with_a_large_page();
            let large = respond(&world.deps().read(), "GET", "/facts?topic=atlas").html;
            server.run(Some(WRITERS + 1), &|_, _| {
                asked.send(()).expect("a test still listening");
                Page {
                    status: 200,
                    html: large.clone(),
                }
            })
        });
        let address = serving.address.clone();

        let slow: Vec<TcpStream> = (0..WRITERS)
            .map(|_| asking_and_not_reading(&address, "/facts?topic=atlas"))
            .collect();
        for _ in 0..WRITERS {
            answers
                .recv_timeout(Duration::from_mins(1))
                .expect("a page for each client that does not read");
        }
        let (head, body) = request(&address, "GET /");
        assert!(head.starts_with("http/1.1 503 "), "{head}");
        has(&body, "<h1>503</h1>");
        assert!(body.len() < 20_000, "{}", body.len());
        assert!(answers.try_recv().is_err());

        drop(slow);
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
