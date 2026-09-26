//! How one request against a broker resolved.
//!
//! A request resolves either to the HTTP status -- with the body of any non-2xx
//! answer, the NGSI-LD problem details -- or, when no response arrived at all, to
//! the client error classified by kind with its full source chain, so a failed
//! request is never reduced to a bare "status 0".

use reqwest::RequestBuilder;
use std::error::Error as StdError;

/// What kind of client-side failure left a request without an HTTP response.
///
/// Read from [`reqwest::Error`]'s own classification, so a report separates a
/// connection the broker refused or reset from a request the client gave up on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TransportFailureKind {
    /// The client's own timeout elapsed before a response arrived.
    Timeout,
    /// No connection could be established (refused, unreachable, no socket).
    Connect,
    /// The request was sent but the connection failed before a response head
    /// arrived -- a reset or a close mid-request.
    Request,
    /// A response head arrived but its body could not be read in full.
    Body,
    /// The response body could not be decoded.
    Decode,
    /// The redirect policy stopped the request.
    Redirect,
    /// Any other client error, such as a request that could not be built.
    Other,
}

/// A request that got no HTTP response, with why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportFailure {
    /// The client's classification of the failure.
    pub kind: TransportFailureKind,
    /// The error and every error in its source chain, outermost first, joined
    /// with `": "` -- the lower layers name the socket-level cause.
    pub chain: String,
}

impl TransportFailure {
    /// Classify a client error and capture its full source chain.
    #[must_use]
    pub fn from_error(error: &reqwest::Error) -> TransportFailure {
        let kind = if error.is_timeout() {
            TransportFailureKind::Timeout
        } else if error.is_connect() {
            TransportFailureKind::Connect
        } else if error.is_body() {
            TransportFailureKind::Body
        } else if error.is_decode() {
            TransportFailureKind::Decode
        } else if error.is_redirect() {
            TransportFailureKind::Redirect
        } else if error.is_request() {
            TransportFailureKind::Request
        } else {
            TransportFailureKind::Other
        };
        TransportFailure {
            kind,
            chain: source_chain(error),
        }
    }
}

/// How one request resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestOutcome {
    /// The broker answered.
    Responded {
        /// The HTTP status code.
        status: u16,
        /// The body of a non-2xx answer (the NGSI-LD problem details), `None`
        /// for a 2xx.
        detail: Option<String>,
    },
    /// No HTTP response arrived.
    TransportFailed(TransportFailure),
}

impl RequestOutcome {
    /// Send `request`, read its whole body and classify how it resolved.
    ///
    /// The body is always read to the end, so the connection returns to the
    /// client's pool instead of being discarded -- an unread body would churn a
    /// fresh connection per request and could itself exhaust the host's sockets.
    pub async fn of(request: RequestBuilder) -> RequestOutcome {
        let response = match request.send().await {
            Ok(response) => response,
            Err(error) => return RequestOutcome::TransportFailed(TransportFailure::from_error(&error)),
        };
        let status = response.status();
        match response.text().await {
            Ok(body) => RequestOutcome::Responded {
                status: status.as_u16(),
                detail: (!status.is_success()).then_some(body),
            },
            Err(error) => RequestOutcome::TransportFailed(TransportFailure::from_error(&error)),
        }
    }

    /// The HTTP status, or `None` when no response arrived.
    #[must_use]
    pub const fn status(&self) -> Option<u16> {
        match self {
            RequestOutcome::Responded { status, .. } => Some(*status),
            RequestOutcome::TransportFailed(_) => None,
        }
    }

    /// Whether the broker answered with a 2xx status.
    #[must_use]
    pub const fn is_success(&self) -> bool {
        matches!(self, RequestOutcome::Responded { status: 200..=299, .. })
    }
}

/// An error and its source chain, outermost first, joined with `": "`.
fn source_chain(error: &(dyn StdError + 'static)) -> String {
    let mut chain = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        chain.push_str(": ");
        chain.push_str(&cause.to_string());
        source = cause.source();
    }
    chain
}

#[cfg(test)]
mod tests {
    use crate::pooler_probe::request_outcome::{RequestOutcome, TransportFailure, TransportFailureKind};

    /// A response outcome with the given status, carrying a body when non-2xx.
    fn responded(status: u16) -> RequestOutcome {
        RequestOutcome::Responded {
            status,
            detail: (!(200..300).contains(&status)).then(|| format!("{{\"status\":{status}}}")),
        }
    }

    /// A transport failure of the given kind and chain.
    fn failed(kind: TransportFailureKind, chain: &str) -> RequestOutcome {
        RequestOutcome::TransportFailed(TransportFailure { kind, chain: chain.to_owned() })
    }

    #[test]
    fn only_a_2xx_answer_is_a_success() {
        assert!(responded(204).is_success());
        assert!(!responded(503).is_success());
        assert!(!failed(TransportFailureKind::Timeout, "timed out").is_success());
        pretty_assertions::assert_eq!(failed(TransportFailureKind::Timeout, "timed out").status(), None);
        pretty_assertions::assert_eq!(responded(201).status(), Some(201));
    }
}
