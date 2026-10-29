//! A context source of one test: a server the test registers with a broker and stubs the answers of.
//!
//! The broker forwards to the source what its registration matches. A broker also forwards a discovery of types or
//! attributes to every registration of the tenant supporting it (clauses 5.7.5 to 5.7.11), so the source receives
//! other tests' discovery requests; every read of what the source received leaves `/ngsi-ld/v1/types` and
//! `/ngsi-ld/v1/attributes` out and counts entity paths only.

use crate::{
    callback_server::{CallbackError, CallbackServer},
    eventually::{Arrivals, Poll, TimedOut, delivery_deadline, eventually},
    source_request::SourceRequest,
};
use http::Method;
use url::Url;
use wiremock::{Mock, MockBuilder, Request, ResponseTemplate};

/// The paths of the discovery operations a broker forwards to every source of the tenant.
const FOREIGN_PATHS: [&str; 2] = ["/ngsi-ld/v1/types", "/ngsi-ld/v1/attributes"];

/// Why the source could not start or be read.
#[derive(Debug, thiserror::Error)]
pub enum SourceError {
    /// The server could not start or does not serve yet.
    #[error(transparent)]
    Server(#[from] CallbackError),
    /// The awaited request did not arrive.
    #[error(transparent)]
    TimedOut(#[from] TimedOut<Arrivals>),
    /// An answer sequence must hold at least one answer.
    #[error("an answer sequence holds no answer")]
    NoAnswer,
    /// A path pattern is not a regular expression.
    #[error("a path pattern is not a regular expression")]
    Pattern(#[source] regex::Error),
}

/// A context source of one test.
#[derive(Debug)]
pub struct ContextSource {
    /// The server the broker forwards to.
    server: CallbackServer,
}

impl ContextSource {
    /// A source that serves from the start, answering 404 to every request no stub matches.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] when the server cannot start.
    pub async fn start() -> Result<ContextSource, SourceError> {
        Ok(ContextSource {
            server: CallbackServer::start().await?,
        })
    }

    /// A source that closes every connection unanswered until [`start_serving`](Self::start_serving), as a source
    /// that is not running yet.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError`] when the server cannot listen.
    pub fn refusing() -> Result<ContextSource, SourceError> {
        Ok(ContextSource {
            server: CallbackServer::refusing()?,
        })
    }

    /// The source answering from now on, with nothing recorded of what it refused.
    pub async fn start_serving(self) -> ContextSource {
        ContextSource {
            server: self.server.start_serving().await,
        }
    }

    /// The registration endpoint of the source, `http://<callback host>:<port>/<prefix>`, as the brokers reach it.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] when `prefix` breaks the URL.
    pub fn endpoint(&self, prefix: &str) -> Result<Url, SourceError> {
        Ok(self.server.url(prefix)?)
    }

    /// The registration endpoint of the source at its root, `http://<callback host>:<port>/`, for a test with one
    /// registration.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] when the callback host breaks the URL.
    pub fn root_endpoint(&self) -> Result<Url, SourceError> {
        self.endpoint("/")
    }

    /// Mounts `mock`.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] before the source serves.
    pub async fn mount(&self, mock: Mock) -> Result<(), SourceError> {
        self.server.server()?.register(mock).await;
        Ok(())
    }

    /// Answers the requests `given` matches with `answers` in turn: each answer once, and the last for every request
    /// after it.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::NoAnswer`] for an empty sequence, and [`SourceError::Server`] before the source serves.
    pub async fn answer_in_turn(&self, given: impl Fn() -> MockBuilder, answers: Vec<ResponseTemplate>) -> Result<(), SourceError> {
        let count = answers.len();
        if count == 0 {
            return Err(SourceError::NoAnswer);
        }
        let server = self.server.server()?;
        for (index, answer) in answers.into_iter().enumerate() {
            let mock = given().respond_with(answer);
            let mock = if index + 1 < count { mock.up_to_n_times(1) } else { mock };
            server.register(mock).await;
        }
        Ok(())
    }

    /// Answers every `verb` request on `request_path` (beneath the source's root) with `answer`.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] before the source serves.
    pub async fn answer(&self, verb: Method, request_path: &str, answer: ResponseTemplate) -> Result<(), SourceError> {
        self.answer_when(SourceRequest::new(verb, request_path), answer).await
    }

    /// Answers every request `request` describes with `answer`.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] before the source serves.
    pub async fn answer_when(&self, request: SourceRequest, answer: ResponseTemplate) -> Result<(), SourceError> {
        self.mount(request.mock(answer)).await
    }

    /// Every `verb` request the source received on `request_path`, in arrival order.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] before the source serves.
    pub async fn received_on(&self, verb: Method, request_path: &str) -> Result<Vec<Request>, SourceError> {
        Ok(self
            .received()
            .await?
            .into_iter()
            .filter(|request| request.method == verb && request.url.path() == request_path)
            .collect())
    }

    /// Every `verb` request on `request_path` once the source holds at least `count` of them, waiting up to the
    /// [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::TimedOut`] when fewer than `count` arrive within the deadline.
    pub async fn wait_for_on(&self, verb: Method, request_path: &str, count: usize) -> Result<Vec<Request>, SourceError> {
        eventually(delivery_deadline(), || {
            let verb = verb.clone();
            async move {
                match self.received_on(verb, request_path).await {
                    Ok(received) if received.len() >= count => Poll::Ready(Ok(received)),
                    Ok(received) => Poll::Pending(Arrivals {
                        held: received.len(),
                        awaited: count,
                    }),
                    Err(error) => Poll::Ready(Err(error)),
                }
            }
        })
        .await?
    }

    /// Every request `request` describes that the source received, in arrival order.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] before the source serves, and [`SourceError::Pattern`] for a path pattern
    /// that is not a regular expression.
    pub async fn received_matching(&self, request: &SourceRequest) -> Result<Vec<Request>, SourceError> {
        let mut matching = Vec::new();
        for received in self.received().await? {
            if request.holds(&received)? {
                matching.push(received);
            }
        }
        Ok(matching)
    }

    /// Every request `request` describes once the source holds at least `count` of them, waiting up to the
    /// [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::TimedOut`] when fewer than `count` arrive within the deadline.
    pub async fn wait_for_matching(&self, request: &SourceRequest, count: usize) -> Result<Vec<Request>, SourceError> {
        eventually(delivery_deadline(), || async move {
            match self.received_matching(request).await {
                Ok(received) if received.len() >= count => Poll::Ready(Ok(received)),
                Ok(received) => Poll::Pending(Arrivals {
                    held: received.len(),
                    awaited: count,
                }),
                Err(error) => Poll::Ready(Err(error)),
            }
        })
        .await?
    }

    /// Every request the source received on an entity path, in arrival order.
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::Server`] before the source serves.
    pub async fn received(&self) -> Result<Vec<Request>, SourceError> {
        Ok(self
            .server
            .received()
            .await?
            .into_iter()
            .filter(|request| !FOREIGN_PATHS.iter().any(|path| request.url.path().starts_with(path)))
            .collect())
    }

    /// The source's requests on entity paths once it holds at least `count`, waiting up to the [`delivery_deadline`].
    ///
    /// # Errors
    ///
    /// Returns [`SourceError::TimedOut`] when fewer than `count` arrive within the deadline.
    pub async fn wait_for(&self, count: usize) -> Result<Vec<Request>, SourceError> {
        eventually(delivery_deadline(), || async move {
            match self.received().await {
                Ok(received) if received.len() >= count => Poll::Ready(Ok(received)),
                Ok(received) => Poll::Pending(Arrivals {
                    held: received.len(),
                    awaited: count,
                }),
                Err(error) => Poll::Ready(Err(error)),
            }
        })
        .await?
    }
}
