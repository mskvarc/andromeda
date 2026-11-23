//! The `health` command: asking a running broker whether it is ready, over the
//! local socket it serves in every management mode.
//!
//! The socket answers `/readyz` from the same indicators as the management
//! listener, 503 when one is down. It carries no TLS and no credential and is
//! reachable only by the user the broker runs as, so the answer does not
//! depend on `management.mode`, on the listeners' transports or on client
//! authentication.

use crate::outcome::ProcessExitOutcome;
use andromeda_configuration::human_duration::HumanDuration;
use andromeda_http_health::routes::READINESS_PATH;
use andromeda_http_server::local_socket_address::LocalSocketAddress;
use headers::{HeaderMapExt, Host};
use http::uri::Authority;
use hyper::{Request, StatusCode, Uri, client::conn::http1};
use hyper_util::rt::TokioIo;
use std::{io, path::PathBuf, time::Duration};
use thiserror::Error;
use tokio::time::timeout;

/// The longest the probe waits for the broker's answer.
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Why the broker could not be confirmed ready.
#[derive(Debug, Error)]
enum ReadinessProbeError {
    /// No broker accepted the connection: no socket exists, no broker listens
    /// on it, or the calling user may not open it.
    #[error("no broker answers on the local socket {}: {source}", .socket.display())]
    Connect {
        /// The socket probed.
        socket: PathBuf,
        /// The connection error.
        source: io::Error,
    },
    /// The HTTP exchange over the socket failed.
    #[error("the readiness exchange over the local socket failed: {0}")]
    Exchange(#[source] hyper::Error),
    /// No answer arrived before the probe's deadline.
    #[error("the broker did not answer the readiness probe on {} within {}", .socket.display(), HumanDuration::new(PROBE_TIMEOUT))]
    Timeout {
        /// The socket probed.
        socket: PathBuf,
    },
    /// The broker answered with a status other than success.
    #[error("the broker answered the readiness probe on {} with {status}", .socket.display())]
    NotReady {
        /// The socket probed.
        socket: PathBuf,
        /// The status the broker answered with.
        status: StatusCode,
    },
}

/// Probe the broker listening on `address`, report the result and return the
/// process outcome.
pub(crate) async fn probe_reported(address: &LocalSocketAddress) -> ProcessExitOutcome {
    match probe(address).await {
        Ok(()) => {
            println!("Broker ready: {}", address.path().display());
            ProcessExitOutcome::Success
        }
        Err(error) => {
            eprintln!("{error}");
            ProcessExitOutcome::NotReady
        }
    }
}

/// Ask the broker on `address` for its readiness, and succeed when it answers
/// with success.
///
/// # Errors
///
/// Returns a [`ReadinessProbeError`] when no broker accepts the connection,
/// when the exchange fails or outlasts [`PROBE_TIMEOUT`], or when the answer is
/// not a success.
async fn probe(address: &LocalSocketAddress) -> Result<(), ReadinessProbeError> {
    let socket = || address.path().to_path_buf();
    let status = timeout(PROBE_TIMEOUT, readiness_status(address))
        .await
        .map_err(|_elapsed| ReadinessProbeError::Timeout { socket: socket() })??;
    if status.is_success() {
        Ok(())
    } else {
        Err(ReadinessProbeError::NotReady { socket: socket(), status })
    }
}

/// The status the broker on `address` answers `GET /readyz` with, over one
/// HTTP/1.1 connection.
///
/// # Errors
///
/// Returns [`ReadinessProbeError::Connect`] when no broker accepts the
/// connection and [`ReadinessProbeError::Exchange`] when the exchange fails.
async fn readiness_status(address: &LocalSocketAddress) -> Result<StatusCode, ReadinessProbeError> {
    let stream = address.connect().await.map_err(|source| ReadinessProbeError::Connect {
        socket: address.path().to_path_buf(),
        source,
    })?;
    let (mut sender, connection) = http1::handshake(TokioIo::new(stream)).await.map_err(ReadinessProbeError::Exchange)?;
    let mut request = Request::new(String::new());
    *request.uri_mut() = Uri::from_static(READINESS_PATH);
    // HTTP/1.1 requires a Host field (RFC 9112 section 3.2); the socket leads
    // to one broker, which reads none.
    request.headers_mut().typed_insert(Host::from(Authority::from_static("localhost")));
    // The answer is reduced to its status inside the exchange, so its body and
    // the sender are dropped there and the connection can finish.
    let exchange = async move { sender.send_request(request).await.map(|response| response.status()) };
    let (status, closed) = tokio::join!(exchange, connection);
    let status = status.map_err(ReadinessProbeError::Exchange)?;
    closed.map_err(ReadinessProbeError::Exchange)?;
    Ok(status)
}

#[cfg(test)]
mod tests {
    use crate::readiness_probe::{ReadinessProbeError, probe};
    use andromeda_configuration::{header_section_limit::HeaderSectionLimit, schema::server::ServerConfig};
    use andromeda_health::{
        aggregate::Health,
        component::{ComponentDetail, ComponentName},
        indicator::HealthIndicator,
        status::HealthStatus,
    };
    use andromeda_http_health::routes::HealthRoutes;
    use andromeda_http_server::{execution::serve_local_socket, local_socket_address::LocalSocketAddress, local_socket_listener::LocalSocketListener};
    use async_trait::async_trait;
    use hyper::StatusCode;
    use std::{error::Error, future::pending};
    use tempfile::TempDir;

    /// An indicator that always reports the status it was built with.
    struct FixedIndicator {
        /// The status it always reports.
        status: HealthStatus,
    }

    #[async_trait]
    impl HealthIndicator for FixedIndicator {
        fn name(&self) -> ComponentName {
            ComponentName::PostgreSql
        }

        async fn check(&self) -> ComponentDetail {
            ComponentDetail {
                status: self.status,
                details: None,
            }
        }
    }

    /// Serve the probes over an indicator reporting `status` on a local socket
    /// in `directory`, and return its address.
    fn serve_probes(directory: &TempDir, status: HealthStatus) -> Result<LocalSocketAddress, Box<dyn Error>> {
        let address = LocalSocketAddress::new(directory.path(), 1056);
        let listener = LocalSocketListener::bind(&address)?;
        let router = HealthRoutes::new(Health::builder().with_indicator(FixedIndicator { status }).build()).into_router();
        tokio::spawn(async move { serve_local_socket(&ServerConfig::default(), HeaderSectionLimit::MAXIMUM, listener, router, pending()).await });
        Ok(address)
    }

    #[tokio::test]
    async fn a_broker_whose_indicators_are_up_is_ready() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        probe(&serve_probes(&directory, HealthStatus::Up)?).await?;
        Ok(())
    }

    #[tokio::test]
    async fn a_broker_with_a_down_indicator_is_not_ready_with_503() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let outcome = probe(&serve_probes(&directory, HealthStatus::Down)?).await;
        assert!(matches!(
            outcome,
            Err(ReadinessProbeError::NotReady {
                status: StatusCode::SERVICE_UNAVAILABLE,
                ..
            })
        ));
        Ok(())
    }

    #[tokio::test]
    async fn a_socket_no_broker_serves_fails_to_connect() -> Result<(), Box<dyn Error>> {
        let directory = TempDir::new()?;
        let outcome = probe(&LocalSocketAddress::new(directory.path(), 1056)).await;
        assert!(matches!(outcome, Err(ReadinessProbeError::Connect { .. })));
        Ok(())
    }
}
