//! Minimal HTTP mock sever for service tests (tiny_http on a random port).

use std::future::Future;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

use parking_lot::Mutex;

/// Runs a test future on the shared runtime, matching how the app drives the
/// services (the shared HTTP client must not hop between runtimes).
pub fn run<F: Future>(future: F) -> F::Output {
    crate::services::runtime::block_on(future)
}

#[derive(Debug, Clone)]
pub struct RecordedRequest {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl RecordedRequest {
    pub fn header(&self, name: &str) -> Option<String> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.clone())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body).unwrap_or(serde_json::Value::Null)
    }
}

#[derive(Debug, Clone)]
pub struct Reply {
    pub status: u16,
    pub body: String,
    pub delay: Duration,
}

impl Reply {
    pub fn json(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.to_string(),
            delay: Duration::ZERO,
        }
    }

    pub fn status(status: u16) -> Self {
        Self::json(status, "")
    }

    pub fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }
}

type Handler = dyn Fn(&RecordedRequest) -> Reply + Send + Sync;

pub struct MockServer {
    server: Arc<tiny_http::Server>,
    port: u16,
    requests: Arc<Mutex<Vec<RecordedRequest>>>,
    thread: Option<JoinHandle<()>>,
}

impl MockServer {
    pub fn start(handler: impl Fn(&RecordedRequest) -> Reply + Send + Sync + 'static) -> Self {
        let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").expect("bind mock server"));
        let port = server
            .server_addr()
            .to_ip()
            .map(|a| a.port())
            .expect("mock server port");
        let requests = Arc::new(Mutex::new(Vec::new()));
        let handler: Arc<Handler> = Arc::new(handler);
        let thread = {
            let server = server.clone();
            let requests = requests.clone();
            std::thread::spawn(move || {
                for mut request in server.incoming_requests() {
                    let mut body = Vec::new();
                    let _ = request.as_reader().read_to_end(&mut body);
                    let recorded = RecordedRequest {
                        method: request.method().as_str().to_uppercase(),
                        path: request.url().to_string(),
                        headers: request
                            .headers()
                            .iter()
                            .map(|h| (h.field.as_str().to_string(), h.value.as_str().to_string()))
                            .collect(),
                        body,
                    };
                    requests.lock().push(recorded.clone());
                    let handler = handler.clone();
                    std::thread::spawn(move || {
                        let reply = handler(&recorded);
                        if !reply.delay.is_zero() {
                            std::thread::sleep(reply.delay);
                        }
                        let header = tiny_http::Header::from_bytes(
                            &b"Content-Type"[..],
                            &b"application/json"[..],
                        )
                        .expect("static header");
                        let response = tiny_http::Response::from_string(reply.body)
                            .with_status_code(reply.status)
                            .with_header(header);
                        let _ = request.respond(response);
                    });
                }
            })
        };
        Self {
            server,
            port,
            requests,
            thread: Some(thread),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{}", self.port, path)
    }

    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.requests.lock().clone()
    }
}

impl Drop for MockServer {
    fn drop(&mut self) {
        self.server.unblock();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
