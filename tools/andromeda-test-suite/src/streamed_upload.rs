//! An upload whose content goes out as a stream of chunks, for a test whose subject is content the typed client always
//! sends whole and declared: content that declares no length, or content that stops arriving short of the length it
//! declares.
//!
//! Without a declared length the content goes chunked over HTTP/1.1 and as DATA frames with no `content-length` over
//! HTTP/2. The answer is read whole, and the exchange is printed for a failing test to show.

use crate::tls_connection::{RawClient, TlsError};
use bytes::Bytes;
use futures::stream::{self, StreamExt};
use http::{HeaderName, HeaderValue, Method, Response, header::CONTENT_LENGTH};
use http_body::Frame;
use http_body_util::StreamBody;
use std::{convert::Infallible, iter, num::TryFromIntError};

/// The octets of each chunk an upload sends.
const CHUNK: usize = 64 * 1024;

/// How the content of a streamed upload goes out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamedContent {
    /// At least this many octets of spaces, in whole chunks, declaring no length.
    Undeclared(u64),
    /// One chunk of spaces under a `Content-Length` of this many octets, the rest never sent.
    Stalled(u64),
}

/// Why a streamed upload could not be sent or its answer read.
#[derive(Debug, thiserror::Error)]
pub enum StreamedUploadError {
    /// The client could not be addressed or built.
    #[error(transparent)]
    Tls(#[from] TlsError),
    /// The length does not fit the platform's sizes.
    #[error("the upload length does not fit in memory")]
    Length(#[from] TryFromIntError),
    /// The upload or its answer failed.
    #[error("the exchange with the broker failed")]
    Http(#[from] reqwest::Error),
    /// The parts of the answer do not form a response.
    #[error("the answer does not form a response")]
    Response(#[from] http::Error),
}

/// Sends `content` with `method` to `path` below the API root of `client`'s broker, with the header fields `headers`
/// besides any the framing needs, and reads the answer whole.
///
/// # Errors
///
/// Returns [`StreamedUploadError`] when the request cannot be addressed, the exchange fails, or the answer is not a
/// response.
pub async fn send_streamed(
    client: &RawClient,
    method: Method,
    path: &str,
    headers: &[(HeaderName, HeaderValue)],
    content: StreamedContent,
) -> Result<Response<Bytes>, StreamedUploadError> {
    let request = headers
        .iter()
        .fold(client.request(method, path)?, |request, (name, value)| request.header(name, value));
    let request = match content {
        StreamedContent::Undeclared(octets) => {
            let chunks = usize::try_from(octets)?.div_ceil(CHUNK);
            request.body(reqwest::Body::wrap(StreamBody::new(stream::iter(
                iter::repeat_with(chunk).take(chunks).map(Ok::<_, Infallible>),
            ))))
        }
        StreamedContent::Stalled(declared) => request.header(CONTENT_LENGTH, declared).body(reqwest::Body::wrap(StreamBody::new(
            stream::iter(iter::once_with(chunk).map(Ok::<_, Infallible>)).chain(stream::pending()),
        ))),
    };
    println!("streamed upload sends {content:?} with the header fields {headers:?}");
    let answer = request.send().await?;
    let response = answer.headers().iter().fold(
        Response::builder().status(answer.status()).version(answer.version()),
        |response, (name, value)| response.header(name, value),
    );
    let response = response.body(answer.bytes().await?)?;
    println!(
        "streamed upload receives {:?} {} with the header fields {:?} and {} octets",
        response.version(),
        response.status(),
        response.headers(),
        response.body().len()
    );
    Ok(response)
}

/// One chunk of spaces, as a data frame.
fn chunk() -> Frame<Bytes> {
    Frame::data(Bytes::from(vec![b' '; CHUNK]))
}
