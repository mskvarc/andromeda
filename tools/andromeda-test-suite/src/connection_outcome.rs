//! What became of a connection a broker may refuse (clause 6.2), read from the transport's failure into a value a test
//! compares, so no test reads a message text.
//!
//! A broker refuses a connection in the TLS or QUIC handshake (an untrusted certificate, a protocol version under its
//! floor, an application protocol it does not serve), or ends it before answering: under TLS 1.3 a client finishes its
//! side of the handshake before the broker verifies the client's certificate, so a refused certificate reaches the
//! client as a TLS alert or a reset after it has sent its request, whichever arrives first. A plaintext request to a TLS
//! listener gets no HTTP answer either. All of these are [`ConnectionOutcome::Refused`], told apart from a broker that
//! cannot be reached at all or does not answer in time.

use ngsi_ld::client::{ClientError, ReqwestError};
use quinn::ConnectionError;
use std::{error::Error, io, iter};

/// What became of a connection, and of the request sent over it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ConnectionOutcome {
    /// The handshake completed and, for a request, the broker answered it over HTTP, whatever the status.
    Accepted,
    /// The broker refused the handshake, or ended the connection without an HTTP answer.
    Refused,
    /// Nothing accepted a connection at the broker's address.
    Unreachable,
    /// Nothing answered within the client's deadline.
    TimedOut,
    /// The client failed on its own side: it refused to send the request, or closed its own connection.
    ClientFailed,
}

impl ConnectionOutcome {
    /// What became of a request whose result is `result`.
    #[must_use]
    pub fn of_request<T>(result: &Result<T, ClientError<ReqwestError>>) -> ConnectionOutcome {
        match result {
            Ok(_) | Err(ClientError::Response(_)) => ConnectionOutcome::Accepted,
            Err(ClientError::Transport(error)) => ConnectionOutcome::of_transport_failure(error),
            // The error is non-exhaustive; what it holds beside an answer and a transport failure is a request the
            // client refused to send.
            Err(_unsent) => ConnectionOutcome::ClientFailed,
        }
    }

    /// What became of a QUIC handshake whose result is `result`.
    #[must_use]
    pub const fn of_quic_handshake<T>(result: &Result<T, ConnectionError>) -> ConnectionOutcome {
        match result {
            Ok(_) => ConnectionOutcome::Accepted,
            Err(error) => ConnectionOutcome::of_quic_failure(error),
        }
    }

    /// The outcome a QUIC connection failure stands for.
    const fn of_quic_failure(error: &ConnectionError) -> ConnectionOutcome {
        match error {
            ConnectionError::VersionMismatch
            | ConnectionError::TransportError(_)
            | ConnectionError::ConnectionClosed(_)
            | ConnectionError::ApplicationClosed(_)
            | ConnectionError::Reset => ConnectionOutcome::Refused,
            ConnectionError::TimedOut => ConnectionOutcome::TimedOut,
            ConnectionError::LocallyClosed | ConnectionError::CidsExhausted => ConnectionOutcome::ClientFailed,
        }
    }

    /// The outcome the transport failure `error` stands for, read from the failures it was caused by.
    fn of_transport_failure(error: &ReqwestError) -> ConnectionOutcome {
        let causes = iter::successors(error.source(), |&cause| cause.source());
        let mut request = None;
        for cause in causes {
            if let Some(quic) = cause.downcast_ref::<ConnectionError>() {
                return ConnectionOutcome::of_quic_failure(quic);
            }
            if cause.is::<rustls::Error>() || cause.downcast_ref::<io::Error>().is_some_and(carries_tls_failure) {
                return ConnectionOutcome::Refused;
            }
            if let Some(failure) = cause.downcast_ref::<reqwest::Error>() {
                request.get_or_insert(failure);
            }
        }
        match request {
            Some(failure) if failure.is_timeout() => ConnectionOutcome::TimedOut,
            Some(failure) if failure.is_connect() => ConnectionOutcome::Unreachable,
            Some(failure) if failure.is_request() || failure.is_body() || failure.is_decode() => ConnectionOutcome::Refused,
            Some(_) | None => ConnectionOutcome::ClientFailed,
        }
    }
}

/// Whether the I/O error `error` carries the TLS library's own error, directly or through further I/O errors that
/// carry one another. An I/O error's source is the source of the error it carries, never that error, so the carried
/// errors are not among the causes the source chain lists.
fn carries_tls_failure(error: &io::Error) -> bool {
    iter::successors(error.get_ref(), |carried| carried.downcast_ref::<io::Error>().and_then(io::Error::get_ref))
        .any(|carried| carried.downcast_ref::<rustls::Error>().is_some())
}

#[cfg(test)]
mod tests {
    use crate::connection_outcome::{ConnectionOutcome, carries_tls_failure};
    use googletest::prelude::*;
    use ngsi_ld::client::{ClientError, ReqwestError};
    use quinn::ConnectionError;
    use std::{
        io::{self, ErrorKind},
        time::Duration,
    };
    use tokio::{io::AsyncWriteExt, net::TcpListener};

    /// The TLS alert a TLS listener sends to a client whose first bytes are not a TLS record: level fatal,
    /// description `decode_error` (IETF RFC 8446 section 6).
    const DECODE_ERROR_ALERT: [u8; 7] = [0x15, 0x03, 0x03, 0x00, 0x02, 0x02, 0x32];

    /// The outcome of a plaintext GET of `url` by a client giving up after `timeout`.
    async fn outcome_of_get(url: &str, timeout: Duration) -> Result<ConnectionOutcome> {
        let client = reqwest::Client::builder().no_proxy().timeout(timeout).build()?;
        let result = client
            .get(url)
            .send()
            .await
            .map(drop)
            .map_err(|error| ClientError::<ReqwestError>::Transport(ReqwestError::from(error)));
        Ok(ConnectionOutcome::of_request(&result))
    }

    #[gtest]
    #[tokio::test]
    async fn a_request_to_a_port_nothing_listens_on_is_unreachable() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        drop(listener);
        let outcome = outcome_of_get(&format!("http://{address}/"), Duration::from_secs(10)).await?;
        verify_that!(outcome, eq(ConnectionOutcome::Unreachable))
    }

    #[gtest]
    #[tokio::test]
    async fn a_request_answered_with_a_tls_alert_is_refused() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await?;
            stream.write_all(&DECODE_ERROR_ALERT).await?;
            stream.shutdown().await
        });
        let outcome = outcome_of_get(&format!("http://{address}/"), Duration::from_secs(10)).await?;
        server.await??;
        verify_that!(outcome, eq(ConnectionOutcome::Refused))
    }

    #[gtest]
    #[tokio::test]
    async fn a_request_nothing_answers_in_time_timed_out() -> Result<()> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let outcome = outcome_of_get(&format!("http://{address}/"), Duration::from_millis(200)).await?;
        drop(listener);
        verify_that!(outcome, eq(ConnectionOutcome::TimedOut))
    }

    #[gtest]
    fn a_completed_quic_handshake_is_accepted() {
        let completed: Result<(), ConnectionError> = Ok(());
        expect_that!(ConnectionOutcome::of_quic_handshake(&completed), eq(ConnectionOutcome::Accepted));
    }

    #[gtest]
    fn a_quic_connection_reset_by_the_broker_is_refused() {
        let reset: Result<(), ConnectionError> = Err(ConnectionError::Reset);
        expect_that!(ConnectionOutcome::of_quic_handshake(&reset), eq(ConnectionOutcome::Refused));
    }

    #[gtest]
    fn a_quic_handshake_without_an_answer_timed_out() {
        let silent: Result<(), ConnectionError> = Err(ConnectionError::TimedOut);
        expect_that!(ConnectionOutcome::of_quic_handshake(&silent), eq(ConnectionOutcome::TimedOut));
    }

    #[gtest]
    fn a_quic_connection_the_client_closed_is_a_client_failure() {
        let closed: Result<(), ConnectionError> = Err(ConnectionError::LocallyClosed);
        expect_that!(ConnectionOutcome::of_quic_handshake(&closed), eq(ConnectionOutcome::ClientFailed));
    }

    #[gtest]
    fn a_tls_error_carried_by_two_io_errors_is_a_tls_failure() {
        let carried = io::Error::new(ErrorKind::InvalidData, rustls::Error::General("refused".to_owned()));
        let outer = io::Error::new(ErrorKind::InvalidData, carried);
        expect_that!(carries_tls_failure(&outer), eq(true));
    }

    #[gtest]
    fn a_refused_tcp_connection_is_no_tls_failure() {
        expect_that!(carries_tls_failure(&io::Error::from(ErrorKind::ConnectionRefused)), eq(false));
    }
}
