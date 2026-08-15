//! Shared programmable HTTP callback server primitive.

use crate::{
    callback_path::{CallbackPath, CallbackPathError},
    server_config::MockServerConfig,
};
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::Request,
    response::Response,
};
use bytes::Bytes;
use dashmap::DashMap;
use http::{HeaderMap, Method, StatusCode, Uri};
use serde::de::DeserializeOwned;
use std::{
    collections::VecDeque,
    io::Error as IoError,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    sync::{Mutex, Notify, oneshot},
    task::{JoinError, JoinHandle},
    time::{Instant, timeout_at},
};
use typed_builder::TypedBuilder;
use url::Url;

/// Exact method-and-path request matcher.
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct RequestMatcher {
    method: Method,
    path: CallbackPath,
}

impl RequestMatcher {
    /// Match one HTTP method and absolute path.
    pub const fn new(method: Method, path: CallbackPath) -> RequestMatcher {
        RequestMatcher { method, path }
    }
}

/// One raw callback request captured by the server.
#[derive(Clone, Debug)]
pub struct CapturedRequest {
    /// Request method.
    pub method: Method,
    /// Complete request URI, including query parameters.
    pub uri: Uri,
    /// Complete request headers.
    pub headers: HeaderMap,
    /// Exact request body bytes.
    pub body: Bytes,
}

impl CapturedRequest {
    /// Decode the captured request bytes as JSON without discarding invalid input.
    pub fn json<T>(&self) -> Result<T, CapturedRequestJsonError>
    where
        T: DeserializeOwned,
    {
        if self.body.is_empty() {
            return Err(CapturedRequestJsonError::EmptyBody);
        }
        serde_json::from_slice(&self.body).map_err(|source| CapturedRequestJsonError::InvalidJson {
            body: self.body.clone(), // clone: the decode error owns the invalid bytes while the captured request remains inspectable.
            source,
        })
    }
}

/// Failure to decode a captured callback request body.
#[derive(Debug, Error)]
pub enum CapturedRequestJsonError {
    /// The callback request contained no body bytes.
    #[error("captured callback request has an empty body")]
    EmptyBody,
    /// The callback request contained invalid JSON.
    #[error("captured callback request contains invalid JSON: {source}")]
    InvalidJson {
        /// Exact invalid request bytes.
        body: Bytes,
        /// JSON decoder failure.
        #[source]
        source: serde_json::Error,
    },
}

/// A barrier that can hold a response after its request has been captured.
#[derive(Clone, Debug, Default)]
pub struct ResponseGate {
    state: Arc<ResponseGateState>,
}

#[derive(Debug, Default)]
struct ResponseGateState {
    released: AtomicBool,
    notify: Notify,
}

impl ResponseGate {
    /// Construct a closed response gate.
    pub fn closed() -> ResponseGate {
        ResponseGate::default()
    }

    /// Release every request currently or subsequently waiting at this gate.
    pub fn release(&self) {
        self.state.released.store(true, Ordering::Release);
        self.state.notify.notify_waiters();
    }

    async fn wait(&self) {
        while !self.state.released.load(Ordering::Acquire) {
            self.state.notify.notified().await;
        }
    }
}

/// One configured callback response.
#[derive(Clone, Debug, TypedBuilder)]
pub struct MockResponse {
    /// HTTP response status.
    status: StatusCode,
    /// Response headers.
    #[builder(default = Default::default())]
    headers: HeaderMap,
    /// Response body bytes.
    #[builder(default = Default::default())]
    body: Bytes,
    /// Optional gate holding delivery after capture.
    #[builder(default = Default::default())]
    gate: Option<ResponseGate>,
}

type ResponseQueue = Arc<Mutex<VecDeque<Arc<MockResponse>>>>;

#[derive(Debug, Default)]
struct CallbackState {
    captures: Mutex<VecDeque<CapturedRequest>>,
    received: Notify,
    responses: DashMap<RequestMatcher, ResponseQueue>,
    counts: DashMap<RequestMatcher, Arc<AtomicUsize>>,
    total_count: AtomicUsize,
}

/// Programmable HTTP callback server with explicit lifecycle.
pub struct CallbackServer {
    endpoint: Url,
    state: Arc<CallbackState>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<Result<(), IoError>>>,
}

impl CallbackServer {
    /// Bind an ephemeral server and return only after its listener is ready.
    pub async fn start(config: MockServerConfig) -> Result<CallbackServer, CallbackServerError> {
        let state = Arc::new(CallbackState::default());
        let handler_state = Arc::clone(&state);
        let router = Router::new().fallback(move |request: Request| {
            let state = Arc::clone(&handler_state);
            async move { handle_request(state, request).await }
        });
        let listener = TcpListener::bind(config.bind_address()).await.map_err(CallbackServerError::Bind)?;
        let port = listener.local_addr().map_err(CallbackServerError::LocalAddress)?.port();
        let endpoint = Url::parse(&format!("http://{}:{port}/", config.visible_host())).map_err(CallbackServerError::Endpoint)?;
        let (shutdown, shutdown_receiver) = oneshot::channel();
        let task = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(async move {
                    let _result = shutdown_receiver.await;
                })
                .await
        });
        Ok(CallbackServer {
            endpoint,
            state,
            shutdown: Some(shutdown),
            task: Some(task),
        })
    }

    /// Resolve a callback path against the broker-visible server endpoint.
    pub fn endpoint(&self, path: &CallbackPath) -> Result<Url, CallbackServerError> {
        self.endpoint.join(path.as_str()).map_err(CallbackServerError::Endpoint)
    }

    /// Add a response to an exact match queue. The final response repeats after earlier responses are consumed.
    pub async fn enqueue(&self, matcher: RequestMatcher, response: MockResponse) {
        let queue = self
            .state
            .responses
            .entry(matcher)
            .or_insert_with(|| Arc::new(Mutex::new(VecDeque::new())))
            .value()
            .clone(); // clone: enqueue owns the queue handle after the concurrent map entry guard is released.
        queue.lock().await.push_back(Arc::new(response));
    }

    /// Wait for and remove the next captured request.
    pub async fn next_request(&self, timeout: Duration) -> Result<CapturedRequest, CallbackServerError> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(request) = self.state.captures.lock().await.pop_front() {
                return Ok(request);
            }
            timeout_at(deadline, self.state.received.notified())
                .await
                .map_err(|_| CallbackServerError::RequestTimeout(timeout))?;
        }
    }

    /// Remove and return every request already captured without waiting.
    pub async fn drain_requests(&self) -> Vec<CapturedRequest> {
        self.state.captures.lock().await.drain(..).collect()
    }

    /// Return the number of requests captured for an exact matcher.
    pub fn request_count(&self, matcher: &RequestMatcher) -> usize {
        self.state.counts.get(matcher).map_or(0, |count| count.load(Ordering::Acquire))
    }

    /// Return the total number of requests captured by this server.
    pub fn total_request_count(&self) -> usize {
        self.state.total_count.load(Ordering::Acquire)
    }

    /// Shut down the listener and report both signaling and server-task failures.
    pub async fn shutdown(mut self) -> Result<(), CallbackServerError> {
        if let Some(shutdown) = self.shutdown.take() {
            shutdown.send(()).map_err(|_| CallbackServerError::ShutdownSignal)?;
        }
        if let Some(task) = self.task.take() {
            task.await.map_err(CallbackServerError::Join)?.map_err(CallbackServerError::Serve)?;
        }
        Ok(())
    }
}

impl Drop for CallbackServer {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

async fn handle_request(state: Arc<CallbackState>, request: Request) -> Response<Body> {
    let method = request.method().clone(); // clone: capture and matching both need the method after the request body is consumed.
    let uri = request.uri().clone(); // clone: capture and path matching both need the URI after the request body is consumed.
    let headers = request.headers().clone(); // clone: the captured request owns headers after the Axum request is consumed.
    let body = match to_bytes(request.into_body(), usize::MAX).await {
        Ok(body) => body,
        Err(error) => return Response::new(Body::from(error.to_string())),
    };
    state.captures.lock().await.push_back(CapturedRequest {
        method: method.clone(), // clone: the capture owns a method while matching retains the original below.
        uri: uri.clone(),       // clone: the capture owns a URI while path matching retains the original below.
        headers,
        body,
    });
    state.total_count.fetch_add(1, Ordering::Release);
    state.received.notify_waiters();
    let path = match CallbackPath::parse(uri.path().to_owned()) {
        Ok(path) => path,
        Err(_error) => return status_response(StatusCode::BAD_REQUEST),
    };
    let matcher = RequestMatcher::new(method, path);
    state
        .counts
        .entry(matcher.clone()) // clone: the request count and response queue use the same immutable match key.
        .or_insert_with(|| Arc::new(AtomicUsize::new(0)))
        .fetch_add(1, Ordering::Release);
    let Some(queue) = state.responses.get(&matcher).map(|entry| Arc::clone(entry.value())) else {
        return status_response(StatusCode::NOT_FOUND);
    };
    let response = {
        let mut responses = queue.lock().await;
        if responses.len() > 1 {
            responses.pop_front()
        } else {
            responses.front().map(Arc::clone)
        }
    };
    let Some(response) = response else {
        return status_response(StatusCode::NOT_FOUND);
    };
    if let Some(gate) = response.gate.as_ref() {
        gate.wait().await;
    }
    let mut outgoing = Response::new(Body::from(response.body.clone())); // clone: the final queued response remains available for deterministic repetition.
    *outgoing.status_mut() = response.status;
    *outgoing.headers_mut() = response.headers.clone(); // clone: the final queued response retains its headers for repetition.
    outgoing
}

fn status_response(status: StatusCode) -> Response<Body> {
    let mut response = Response::new(Body::empty());
    *response.status_mut() = status;
    response
}

/// Callback server startup, observation, or shutdown failure.
#[derive(Debug, Error)]
pub enum CallbackServerError {
    /// Listener binding failed.
    #[error("failed to bind callback listener: {0}")]
    Bind(#[source] IoError),
    /// The bound listener address could not be read.
    #[error("failed to read callback listener address: {0}")]
    LocalAddress(#[source] IoError),
    /// The broker-visible endpoint URL could not be constructed.
    #[error("failed to construct callback endpoint URL: {0}")]
    Endpoint(#[source] url::ParseError),
    /// No request was captured within the timeout.
    #[error("no callback request was captured within {0:?}")]
    RequestTimeout(Duration),
    /// The server stopped before receiving its shutdown signal.
    #[error("callback server stopped before shutdown was signaled")]
    ShutdownSignal,
    /// The spawned server task failed to join.
    #[error("callback server task failed: {0}")]
    Join(#[source] JoinError),
    /// Axum returned a server error.
    #[error("callback server failed: {0}")]
    Serve(#[source] IoError),
    /// A concrete server supplied an invalid callback path.
    #[error(transparent)]
    Path(#[from] CallbackPathError),
}

#[cfg(test)]
mod tests {
    use crate::{
        callback_path::CallbackPath,
        callback_server::{CallbackServer, MockResponse, RequestMatcher, ResponseGate},
        server_config::MockServerConfig,
    };
    use bytes::Bytes;
    use http::{Method, StatusCode};
    use std::{net::SocketAddr, time::Duration};
    use url::Host;

    fn local_config() -> MockServerConfig {
        let host = Host::parse("127.0.0.1").unwrap_or_else(|error| unreachable!("static host is valid: {error}"));
        MockServerConfig::new(SocketAddr::from(([127, 0, 0, 1], 0)), host)
    }

    #[tokio::test]
    async fn response_queue_captures_requests_and_repeats_its_final_response() {
        let server = CallbackServer::start(local_config())
            .await
            .unwrap_or_else(|error| unreachable!("local callback server starts: {error}"));
        let path = CallbackPath::parse("/queue").unwrap_or_else(|error| unreachable!("static path is valid: {error}"));
        let matcher = RequestMatcher::new(Method::POST, path.clone()); // clone: the matcher and endpoint retain the same immutable path.
        server
            .enqueue(
                matcher.clone(), // clone: both queued responses share one exact match key.
                MockResponse::builder().status(StatusCode::CREATED).body(Bytes::from_static(b"first")).build(),
            )
            .await;
        server
            .enqueue(
                matcher.clone(), // clone: request counting below retains the same match key.
                MockResponse::builder().status(StatusCode::ACCEPTED).body(Bytes::from_static(b"final")).build(),
            )
            .await;
        let endpoint = server
            .endpoint(&path)
            .unwrap_or_else(|error| unreachable!("callback endpoint resolves: {error}"));
        let client = reqwest::Client::new();

        for (expected_status, expected_body) in [(StatusCode::CREATED, "first"), (StatusCode::ACCEPTED, "final"), (StatusCode::ACCEPTED, "final")] {
            let response = client
                .post(endpoint.clone()) // clone: each request owns the same immutable endpoint URL.
                .body("payload")
                .send()
                .await
                .unwrap_or_else(|error| unreachable!("local callback request succeeds: {error}"));
            assert_eq!(response.status(), expected_status);
            let body = response.text().await.unwrap_or_else(|error| unreachable!("response body is readable: {error}"));
            assert_eq!(body, expected_body);
        }

        for _index in 0..3 {
            let captured = server
                .next_request(Duration::from_secs(1))
                .await
                .unwrap_or_else(|error| unreachable!("request was captured: {error}"));
            assert_eq!(captured.method, Method::POST);
            assert_eq!(captured.uri.path(), "/queue");
            assert_eq!(captured.body, Bytes::from_static(b"payload"));
        }
        assert_eq!(server.request_count(&matcher), 3);
        server
            .shutdown()
            .await
            .unwrap_or_else(|error| unreachable!("callback server shuts down: {error}"));
    }

    #[tokio::test]
    async fn response_gate_releases_only_after_request_capture() {
        let server = CallbackServer::start(local_config())
            .await
            .unwrap_or_else(|error| unreachable!("local callback server starts: {error}"));
        let path = CallbackPath::parse("/gated").unwrap_or_else(|error| unreachable!("static path is valid: {error}"));
        let gate = ResponseGate::closed();
        server
            .enqueue(
                RequestMatcher::new(Method::GET, path.clone()), // clone: endpoint construction below retains the immutable path.
                MockResponse::builder()
                    .status(StatusCode::NO_CONTENT)
                    .gate(Some(gate.clone())) // clone: the test owns the release handle while the response owns its waiter.
                    .build(),
            )
            .await;
        let endpoint = server
            .endpoint(&path)
            .unwrap_or_else(|error| unreachable!("callback endpoint resolves: {error}"));
        let request = tokio::spawn(async move { reqwest::get(endpoint).await });

        let captured = server
            .next_request(Duration::from_secs(1))
            .await
            .unwrap_or_else(|error| unreachable!("gated request was captured: {error}"));
        assert_eq!(captured.uri.path(), "/gated");
        assert!(!request.is_finished());
        gate.release();
        let response = request
            .await
            .unwrap_or_else(|error| unreachable!("request task joins: {error}"))
            .unwrap_or_else(|error| unreachable!("local request succeeds: {error}"));
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        server
            .shutdown()
            .await
            .unwrap_or_else(|error| unreachable!("callback server shuts down: {error}"));
    }
}
