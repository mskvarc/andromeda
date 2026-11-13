//! Asking a broker's readiness probe, GET `readyz` at the root of its management listener, outside the API root, and
//! reading the `HealthCheck` it answers with and how long it took to answer.

use crate::{
    broker::{self, BrokerClient, BrokerError},
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    health_check::HealthDocument,
    wire::{WireError, request_to},
};
use http::{Method, StatusCode, Uri, uri::InvalidUri};
use ngsi_ld::{
    client::{ClientError, ReqwestError},
    http::ResponseError,
};
use std::time::{Duration, Instant};
use url::{ParseError, Url};

/// Why the readiness probe could not be asked or its answer read.
#[derive(Debug, thiserror::Error)]
pub enum ReadinessProbeError {
    /// The stack names no such listener.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// No client of the listener could be built.
    #[error(transparent)]
    Broker(#[from] BrokerError),
    /// The probe's path does not resolve against the listener.
    #[error("the readiness probe has no address on the management listener")]
    Address(#[from] ParseError),
    /// The request could not be started.
    #[error(transparent)]
    Wire(#[from] WireError),
    /// The probe's address is not a request target.
    #[error("the readiness probe's address is not a request target")]
    Target(#[from] InvalidUri),
    /// The request could not be sent, or no answer arrived.
    #[error("the readiness probe could not be asked")]
    Client(#[source] Box<ClientError<ReqwestError>>),
    /// The answer is not a `HealthCheck`.
    #[error("the readiness probe answered no HealthCheck")]
    Document(#[from] serde_json::Error),
}

/// What the readiness probe answered.
#[derive(Debug, Clone)]
pub struct Readiness {
    /// The status: 200 while no component is down, 503 otherwise.
    pub status: StatusCode,
    /// The `HealthCheck` document.
    pub document: HealthDocument,
    /// From sending the request to reading the whole answer.
    pub took: Duration,
}

/// The readiness probe of one broker's management listener.
#[derive(Debug)]
pub struct ReadinessProbe {
    /// A client of the management listener.
    management: BrokerClient,
    /// The probe's address on it.
    readyz: Url,
}

impl ReadinessProbe {
    /// The readiness probe of the management listener of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`ReadinessProbeError`] when the stack names no such listener or no client of it can be built.
    pub fn of(role: BrokerRole) -> Result<ReadinessProbe, ReadinessProbeError> {
        let config = SuiteConfig::get().map_err(ConfigError::clone)?;
        Ok(ReadinessProbe {
            management: broker::client(role)?,
            readyz: config.broker(role)?.join("readyz")?,
        })
    }

    /// Asks the probe once. A 503 is a reading like a 200: the client reports it as a problem, whose body it keeps
    /// as received, and that body is the `HealthCheck`.
    ///
    /// # Errors
    ///
    /// Returns [`ReadinessProbeError`] when no answer arrives, the answer is an error other than a problem, or it is
    /// not a `HealthCheck`.
    pub async fn ask(&self) -> Result<Readiness, ReadinessProbeError> {
        let request = request_to(&self.management, Method::GET, "")?.uri(self.readyz.as_str().parse::<Uri>()?);
        let sent = Instant::now();
        let answer = self.management.send_unchecked(request).await;
        let took = sent.elapsed();
        match answer {
            Ok(answer) => Ok(Readiness {
                status: answer.status(),
                document: serde_json::from_slice(answer.body())?,
                took,
            }),
            Err(ClientError::Response(ResponseError::Problem { status, body, .. })) => Ok(Readiness {
                status,
                document: serde_json::from_str(&body.to_string())?,
                took,
            }),
            Err(error) => Err(ReadinessProbeError::Client(Box::new(error))),
        }
    }
}
