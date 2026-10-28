//! A transport that prints every exchange of a test, so that a failure reads without the broker's logs.
//!
//! Each exchange goes to the test's standard output as it completes: the request (method, URI, headers, body) and
//! the response (status, version, headers, body), or the transport failure. `cargo nextest` runs each test in a
//! process of its own and shows its output only when it fails, so a passing test prints nothing a reader sees.

use bytes::Bytes;
use http::{HeaderMap, Method, Request, Response, Uri};
use ngsi_ld::client::Transport;
use std::{
    error::Error,
    fmt::{self, Display, Write},
    str,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

/// The bytes of a body printed before the rest is elided: enough for any NGSI-LD document a test reads, far below
/// the generated bodies of the size-limit tests.
const PRINTED_BODY_BYTES: usize = 16 * 1024;

/// A transport printing every exchange it carries.
#[derive(Debug, Clone)]
pub struct ExchangeLog<T> {
    /// The transport that sends.
    inner: T,
    /// The number of the last exchange, shared by every client derived from the same one.
    sequence: Arc<AtomicUsize>,
}

impl<T: Transport> ExchangeLog<T> {
    /// Prints every exchange `inner` carries.
    #[must_use]
    pub fn new(inner: T) -> ExchangeLog<T> {
        ExchangeLog {
            inner,
            sequence: Arc::new(AtomicUsize::new(0)),
        }
    }

    /// Prints every exchange `inner` carries, numbered after this log's exchanges, for a test that sends through
    /// more than one client.
    #[must_use]
    pub fn beside(&self, inner: T) -> ExchangeLog<T> {
        ExchangeLog {
            inner,
            sequence: Arc::clone(&self.sequence),
        }
    }
}

impl<T: Transport> Transport for ExchangeLog<T> {
    type Error = T::Error;

    async fn send(&self, request: Request<Bytes>) -> Result<Response<Bytes>, T::Error> {
        let number = self.sequence.fetch_add(1, Ordering::Relaxed) + 1;
        let sent = SentRequest::of(&request);
        let result = self.inner.send(request).await;
        println!(
            "{}",
            Exchange {
                number,
                sent: &sent,
                result: &result
            }
        );
        result
    }
}

/// What the log keeps of a request: its body shares the request's buffer.
#[derive(Debug)]
struct SentRequest {
    /// The method.
    method: Method,
    /// The target.
    uri: Uri,
    /// The header fields.
    headers: HeaderMap,
    /// The body.
    body: Bytes,
}

impl SentRequest {
    /// The parts of `request` the log prints.
    fn of(request: &Request<Bytes>) -> SentRequest {
        SentRequest {
            method: request.method().clone(),
            uri: request.uri().clone(),
            // The request is moved into the transport; the log prints its headers after the response arrives.
            headers: request.headers().clone(),
            body: request.body().clone(),
        }
    }
}

/// One exchange, rendered for the test's output.
struct Exchange<'a, E> {
    /// The position of the exchange in the test.
    number: usize,
    /// The request.
    sent: &'a SentRequest,
    /// The response, or why there was none.
    result: &'a Result<Response<Bytes>, E>,
}

impl<E: Error> Display for Exchange<'_, E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(formatter, "--- exchange {} ---", self.number)?;
        writeln!(formatter, "> {} {}", self.sent.method, self.sent.uri)?;
        write_headers(formatter, '>', &self.sent.headers)?;
        write_body(formatter, &self.sent.body)?;
        match self.result {
            Ok(response) => {
                writeln!(formatter, "< {:?} {}", response.version(), response.status())?;
                write_headers(formatter, '<', response.headers())?;
                write_body(formatter, response.body())
            }
            Err(error) => {
                write!(formatter, "< no response: {error}")?;
                let mut source = error.source();
                while let Some(cause) = source {
                    write!(formatter, ": {cause}")?;
                    source = cause.source();
                }
                formatter.write_char('\n')
            }
        }
    }
}

/// Writes each header field on a line of its own, behind `marker`.
fn write_headers(formatter: &mut fmt::Formatter<'_>, marker: char, headers: &HeaderMap) -> fmt::Result {
    headers.iter().try_for_each(|(name, value)| match value.to_str() {
        Ok(text) => writeln!(formatter, "{marker} {name}: {text}"),
        Err(_) => writeln!(formatter, "{marker} {name}: {value:?}"),
    })
}

/// Writes a body as text when it is UTF-8, cut at [`PRINTED_BODY_BYTES`].
fn write_body(formatter: &mut fmt::Formatter<'_>, body: &Bytes) -> fmt::Result {
    if body.is_empty() {
        return Ok(());
    }
    let shown = &body[..body.len().min(PRINTED_BODY_BYTES)];
    match str::from_utf8(shown) {
        Ok(text) => writeln!(formatter, "{text}")?,
        Err(_) => writeln!(formatter, "[{} bytes, not UTF-8]", body.len())?,
    }
    if body.len() > PRINTED_BODY_BYTES {
        writeln!(formatter, "[{} more bytes]", body.len() - PRINTED_BODY_BYTES)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::exchange_log::{Exchange, SentRequest};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::{Request, Response, StatusCode, header::CONTENT_TYPE};
    use std::io;

    #[gtest]
    fn an_exchange_shows_request_and_response_with_their_headers_and_bodies() -> Result<()> {
        let request = Request::post("http://broker:1056/ngsi-ld/v1/entities")
            .header(CONTENT_TYPE, "application/json")
            .body(Bytes::from_static(b"{\"id\":\"urn:a\"}"))?;
        let response = Response::builder()
            .status(StatusCode::CREATED)
            .header("location", "/ngsi-ld/v1/entities/urn:a")
            .body(Bytes::new())?;
        let result: io::Result<Response<Bytes>> = Ok(response);
        let text = Exchange {
            number: 3,
            sent: &SentRequest::of(&request),
            result: &result,
        }
        .to_string();
        verify_that!(
            text,
            eq(
                "--- exchange 3 ---\n> POST http://broker:1056/ngsi-ld/v1/entities\n> content-type: application/json\n{\"id\":\"urn:a\"}\n< HTTP/1.1 201 Created\n< location: /ngsi-ld/v1/entities/urn:a\n"
            )
        )
    }

    #[gtest]
    fn a_transport_failure_shows_its_causes() -> Result<()> {
        let request = Request::get("http://broker:1056/ngsi-ld/v1/types").body(Bytes::new())?;
        let result: io::Result<Response<Bytes>> = Err(io::Error::other("connection refused"));
        let text = Exchange {
            number: 1,
            sent: &SentRequest::of(&request),
            result: &result,
        }
        .to_string();
        verify_that!(text, ends_with("< no response: connection refused\n"))
    }

    #[gtest]
    fn a_long_body_is_cut_and_the_rest_counted() -> Result<()> {
        let request = Request::post("http://broker:1056/ngsi-ld/v1/entities").body(Bytes::from(vec![b'a'; 16 * 1024 + 5]))?;
        let result: io::Result<Response<Bytes>> = Err(io::Error::other("reset"));
        let text = Exchange {
            number: 1,
            sent: &SentRequest::of(&request),
            result: &result,
        }
        .to_string();
        verify_that!(text, contains_substring("[5 more bytes]"))
    }
}
