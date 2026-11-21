//! A request written to the broker's socket byte for byte, for a test whose subject is a message the HTTP client
//! cannot form: a `POST` that states neither a body nor a `Content-Length`, which the client's HTTP stack completes
//! with `Content-Length: 0` (clause 6.3.2 has the broker answer 411 Length Required).
//!
//! The request goes over HTTP/1.1 with `Connection: close`, and the response is read to the end of the stream.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
};
use bytes::Bytes;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
    Response,
    StatusCode,
    header::{InvalidHeaderName, InvalidHeaderValue, TRANSFER_ENCODING},
};
use httparse::{EMPTY_HEADER, Status};
use std::{
    io::{self, Read, Write},
    net::TcpStream,
    time::Duration,
};
use tokio::task::{self, JoinError};
use url::Url;

/// The longest the broker may take to answer.
const ANSWER_TIMEOUT: Duration = Duration::from_secs(120);

/// The most header fields an answer's head is read with; a head carrying more is refused as unreadable.
const MAX_FIELDS: usize = 64;

/// Why a raw request could not be sent or its answer read.
#[derive(Debug, thiserror::Error)]
pub enum RawHttpError {
    /// The test configuration does not name the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The broker URL is not a plain HTTP origin the socket can be opened to.
    #[error("{0} is not an HTTP origin with a host")]
    Origin(Url),
    /// The broker URL cannot carry the API path.
    #[error("{0} cannot carry the API path")]
    ApiPath(Url, #[source] url::ParseError),
    /// The socket failed.
    #[error("the exchange with the broker failed")]
    Io(#[from] io::Error),
    /// The blocking exchange did not finish.
    #[error("the exchange with the broker did not finish")]
    Join(#[from] JoinError),
    /// The answer ends before the blank line that ends its head.
    #[error("the answer has no end of its head")]
    NoHeadEnd,
    /// The head is not the head of an HTTP/1.x response.
    #[error("the head of the answer does not read")]
    Head(#[source] httparse::Error),
    /// The three-digit status code lies below 100, where no status is defined.
    #[error("the status line of the answer names no status code")]
    StatusLine,
    /// A field name of the head is not a header name.
    #[error("a field name of the answer is not a header name")]
    FieldName(#[source] InvalidHeaderName),
    /// A field value of the head is not a header value.
    #[error("a field value of the answer is not a header value")]
    FieldValue(#[source] InvalidHeaderValue),
    /// The body ends before the line stating a chunk's size.
    #[error("a chunk of the answer has no size line")]
    NoChunkSizeLine,
    /// A chunk size line is not a hexadecimal size with optional extensions.
    #[error("a chunk size of the answer is not hexadecimal")]
    ChunkSize,
    /// The body ends before a chunk of the stated size.
    #[error("the answer ends inside a chunk of {size} octets")]
    ShortChunk {
        /// The octets the chunk states.
        size: u64,
    },
    /// A chunk is not followed by its line end.
    #[error("a chunk of the answer is not followed by its line end")]
    NoChunkEnd,
}

/// Sends `method` to `target` (the path below the API root, with its query) of the broker of `role`, with the header
/// fields `headers` and `body` as written: no field is added but `Host` and `Connection`.
///
/// # Errors
///
/// Returns [`RawHttpError`] when the broker is not reachable over plain HTTP, the socket fails or the answer is not
/// an HTTP response.
pub async fn send_bytes(role: BrokerRole, method: &Method, target: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<Response<Bytes>, RawHttpError> {
    let RawTarget { host, port, api_path } = RawTarget::of(role)?;
    let mut request = format!("{method} {api_path}{target} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\n").into_bytes();
    for (name, value) in headers {
        request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    request.extend_from_slice(b"\r\n");
    request.extend_from_slice(body);
    println!("raw request:\n{}", String::from_utf8_lossy(&request));
    let answer = task::spawn_blocking(move || exchange(&host, port, &request)).await??;
    println!("raw answer:\n{}", String::from_utf8_lossy(&answer));
    parse(&answer)
}

/// Where a raw exchange with a broker goes.
#[derive(Debug)]
pub(crate) struct RawTarget {
    /// The host the broker listens on.
    pub(crate) host: String,
    /// The port the broker listens on.
    pub(crate) port: u16,
    /// The path of the broker's API root, with its trailing `/`.
    pub(crate) api_path: String,
}

impl RawTarget {
    /// The plain HTTP socket and API root of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`RawHttpError`] when the configuration does not name the broker, or names it by no plain HTTP origin.
    pub(crate) fn of(role: BrokerRole) -> Result<RawTarget, RawHttpError> {
        let config = SuiteConfig::get().map_err(Clone::clone)?;
        let origin = config.broker(role)?;
        let root = origin.join("ngsi-ld/v1/").map_err(|error| RawHttpError::ApiPath(origin.clone(), error))?;
        match (root.scheme(), root.host_str(), root.port_or_known_default()) {
            ("http", Some(host), Some(port)) => Ok(RawTarget {
                host: host.to_owned(),
                port,
                api_path: root.path().to_owned(),
            }),
            _ => Err(RawHttpError::Origin(root)),
        }
    }
}

/// Writes `request` to `host:port` and reads the stream to its end.
fn exchange(host: &str, port: u16, request: &[u8]) -> Result<Vec<u8>, io::Error> {
    let mut stream = TcpStream::connect((host, port))?;
    stream.set_read_timeout(Some(ANSWER_TIMEOUT))?;
    stream.write_all(request)?;
    let mut answer = Vec::new();
    stream.read_to_end(&mut answer)?;
    Ok(answer)
}

/// The head of an answer, read from the front of the octets received so far.
#[derive(Debug)]
pub(crate) struct AnswerHead {
    /// The status and header fields, with no body.
    pub(crate) response: Response<()>,
    /// The octets the head spans, the blank line that ends it included.
    pub(crate) octets: usize,
}

impl AnswerHead {
    /// The head at the front of `received`; `None` while the blank line that ends it has not arrived.
    ///
    /// # Errors
    ///
    /// Returns [`RawHttpError`] when `received` does not open with the head of an HTTP/1.x response.
    pub(crate) fn read(received: &[u8]) -> Result<Option<AnswerHead>, RawHttpError> {
        let mut fields = [EMPTY_HEADER; MAX_FIELDS];
        let mut head = httparse::Response::new(&mut fields);
        let octets = match head.parse(received).map_err(RawHttpError::Head)? {
            Status::Complete(octets) => octets,
            Status::Partial => return Ok(None),
        };
        let status = head.code.and_then(|code| StatusCode::from_u16(code).ok()).ok_or(RawHttpError::StatusLine)?;
        let headers = head
            .headers
            .iter()
            .map(|field| {
                let name = HeaderName::from_bytes(field.name.as_bytes()).map_err(RawHttpError::FieldName)?;
                let value = HeaderValue::from_bytes(field.value).map_err(RawHttpError::FieldValue)?;
                Ok((name, value))
            })
            .collect::<Result<HeaderMap, RawHttpError>>()?;
        let mut response = Response::new(());
        *response.status_mut() = status;
        *response.headers_mut() = headers;
        Ok(Some(AnswerHead { response, octets }))
    }
}

/// The response in `answer`: its status line, header fields and body, with a chunked body decoded.
///
/// # Errors
///
/// Returns [`RawHttpError`] when `answer` is not an HTTP response.
pub fn parse(answer: &[u8]) -> Result<Response<Bytes>, RawHttpError> {
    let AnswerHead { response, octets } = AnswerHead::read(answer)?.ok_or(RawHttpError::NoHeadEnd)?;
    let rest = &answer[octets..];
    let chunked = response
        .headers()
        .get(TRANSFER_ENCODING)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.to_ascii_lowercase().contains("chunked"));
    let body = if chunked { dechunk(rest)? } else { rest.to_vec() };
    Ok(response.map(|()| Bytes::from(body)))
}

/// The payload of a chunked body.
fn dechunk(mut rest: &[u8]) -> Result<Vec<u8>, RawHttpError> {
    let mut payload = Vec::new();
    loop {
        let (size_line, size) = match httparse::parse_chunk_size(rest).map_err(|_invalid| RawHttpError::ChunkSize)? {
            Status::Complete(sized) => sized,
            Status::Partial => return Err(RawHttpError::NoChunkSizeLine),
        };
        rest = &rest[size_line..];
        if size == 0 {
            return Ok(payload);
        }
        let chunk = usize::try_from(size)
            .ok()
            .and_then(|length| rest.get(..length))
            .ok_or(RawHttpError::ShortChunk { size })?;
        payload.extend_from_slice(chunk);
        rest = rest[chunk.len()..].strip_prefix(b"\r\n").ok_or(RawHttpError::NoChunkEnd)?;
    }
}

#[cfg(test)]
mod tests {
    use crate::raw_http::{AnswerHead, RawHttpError, parse};
    use googletest::prelude::*;
    use http::{StatusCode, header::CONTENT_LENGTH};

    #[gtest]
    fn a_response_without_a_body_is_read() -> Result<()> {
        let response = parse(b"HTTP/1.1 411 Length Required\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        expect_that!(response.status(), eq(StatusCode::LENGTH_REQUIRED));
        verify_that!(response.body().len(), eq(0))
    }

    #[gtest]
    fn a_chunked_body_is_decoded() -> Result<()> {
        let response = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2\r\nde\r\n0\r\n\r\n")?;
        verify_that!(response.body().as_ref(), eq(b"abcde".as_slice()))
    }

    #[gtest]
    fn a_body_without_chunks_is_kept_whole() -> Result<()> {
        let response = parse(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}")?;
        verify_that!(response.body().as_ref(), eq(b"{}".as_slice()))
    }

    #[gtest]
    fn a_message_that_is_no_response_is_refused() -> Result<()> {
        verify_that!(parse(b"garbage"), err(predicate(|error: &RawHttpError| matches!(error, RawHttpError::Head(_)))))
    }

    #[gtest]
    fn a_head_without_its_blank_line_is_not_read_yet() -> Result<()> {
        verify_that!(AnswerHead::read(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n")?, none())
    }

    #[gtest]
    fn a_finished_head_spans_every_octet_before_the_body() -> Result<()> {
        let answer = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\n{}";
        let head = AnswerHead::read(answer)?.or_fail()?;
        expect_that!(head.octets, eq(answer.len() - b"{}".len()));
        verify_that!(head.response.headers().get(CONTENT_LENGTH), some(eq("2")))
    }

    #[gtest]
    fn a_chunk_size_with_an_extension_is_read() -> Result<()> {
        let response = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3;name=value\r\nabc\r\n0\r\n\r\n")?;
        verify_that!(response.body().as_ref(), eq(b"abc".as_slice()))
    }

    #[gtest]
    fn a_chunk_not_followed_by_its_line_end_is_refused() -> Result<()> {
        let answer = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabcX\r\n0\r\n\r\n");
        verify_that!(answer, err(predicate(|error: &RawHttpError| matches!(error, RawHttpError::NoChunkEnd))))
    }

    #[gtest]
    fn a_chunk_longer_than_the_body_is_refused() -> Result<()> {
        let answer = parse(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n9\r\nabc");
        verify_that!(
            answer,
            err(predicate(|error: &RawHttpError| matches!(error, RawHttpError::ShortChunk { size: 9 })))
        )
    }
}
