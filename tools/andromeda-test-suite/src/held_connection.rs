//! A plaintext HTTP/1.1 connection to a broker, written byte for byte and held open, for a test whose subject is how
//! long the broker keeps a connection that carries no request: a head sent in parts or never finished, or a
//! keep-alive connection left idle after its response.

use crate::{
    config::BrokerRole,
    raw_http::{RawHttpError, RawTarget, find, parse},
};
use bytes::Bytes;
use http::{Response, header::CONTENT_LENGTH};
use std::{io, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::{Instant, timeout},
};

/// The longest a test waits for the broker to answer or to close the connection.
const CLOSE_TIMEOUT: Duration = Duration::from_secs(60);

/// Why a held connection could not be used.
#[derive(Debug, thiserror::Error)]
pub enum HeldConnectionError {
    /// The broker could not be addressed, or its answer is not an HTTP response.
    #[error(transparent)]
    Raw(#[from] RawHttpError),
    /// The socket failed.
    #[error("the connection to the broker failed")]
    Io(#[from] io::Error),
    /// The broker neither answered nor closed the connection in time.
    #[error("the broker kept the connection open for over {0:?}")]
    StillOpen(Duration),
    /// The broker closed the connection before its response ended.
    #[error("the broker closed the connection inside its response")]
    ClosedEarly,
    /// The response states no length to read its body by.
    #[error("the response states no content length")]
    NoContentLength,
}

/// A plaintext connection to a broker, held open.
#[derive(Debug)]
pub struct HeldConnection {
    /// The connection itself.
    stream: TcpStream,
    /// Where the connection goes.
    target: RawTarget,
}

impl HeldConnection {
    /// Open a connection to the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`HeldConnectionError`] when the broker is not reachable over plain HTTP.
    pub async fn open(role: BrokerRole) -> Result<HeldConnection, HeldConnectionError> {
        let target = RawTarget::of(role)?;
        let stream = TcpStream::connect((target.host.as_str(), target.port)).await?;
        Ok(HeldConnection { stream, target })
    }

    /// The request line and `Host` field of a `GET` of `path` below the API root, without the empty line that ends a
    /// head.
    #[must_use]
    pub fn get_head_lines(&self, path: &str) -> String {
        let RawTarget { host, port, api_path } = &self.target;
        format!("GET {api_path}{path} HTTP/1.1\r\nHost: {host}:{port}\r\n")
    }

    /// Write `octets` to the connection.
    ///
    /// # Errors
    ///
    /// Returns [`HeldConnectionError::Io`] when the socket fails.
    pub async fn send(&mut self, octets: &str) -> Result<(), HeldConnectionError> {
        println!("held connection sends:\n{octets}");
        Ok(self.stream.write_all(octets.as_bytes()).await?)
    }

    /// Read one response with a `Content-Length` from the connection, leaving it open.
    ///
    /// # Errors
    ///
    /// Returns [`HeldConnectionError`] when the broker answers nothing in time, closes the connection inside the
    /// response, or sends no response with a length.
    pub async fn response(&mut self) -> Result<Response<Bytes>, HeldConnectionError> {
        let mut received = Vec::new();
        loop {
            if let Some(head_end) = find(&received, b"\r\n\r\n") {
                let head = parse(received.get(..head_end.saturating_add(4)).unwrap_or_default())?;
                let length = head
                    .headers()
                    .get(CONTENT_LENGTH)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| value.parse::<usize>().ok())
                    .ok_or(HeldConnectionError::NoContentLength)?;
                if received.len() >= head_end.saturating_add(4).saturating_add(length) {
                    println!("held connection receives:\n{}", String::from_utf8_lossy(&received));
                    return Ok(parse(&received)?);
                }
            }
            let mut chunk = [0; 4096];
            let read = timeout(CLOSE_TIMEOUT, self.stream.read(&mut chunk))
                .await
                .map_err(|_elapsed| HeldConnectionError::StillOpen(CLOSE_TIMEOUT))??;
            if read == 0 {
                return Err(HeldConnectionError::ClosedEarly);
            }
            received.extend_from_slice(chunk.get(..read).unwrap_or_default());
        }
    }

    /// Everything the connection receives until the broker closes it, and when that was.
    ///
    /// # Errors
    ///
    /// Returns [`HeldConnectionError`] when the socket fails or the broker keeps the connection open for longer than
    /// a test waits.
    pub async fn until_closed(&mut self) -> Result<(Vec<u8>, Instant), HeldConnectionError> {
        let mut received = Vec::new();
        timeout(CLOSE_TIMEOUT, self.stream.read_to_end(&mut received))
            .await
            .map_err(|_elapsed| HeldConnectionError::StillOpen(CLOSE_TIMEOUT))??;
        println!("held connection receives before the close:\n{}", String::from_utf8_lossy(&received));
        Ok((received, Instant::now()))
    }
}
