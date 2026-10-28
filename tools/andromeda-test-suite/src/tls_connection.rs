//! How a client reaches a broker over TLS (clause 6.2): the authority it trusts beside the platform's, the identity
//! it presents for mutual TLS, and the HTTP version it offers. Paths name files beneath the stack's directory, where
//! the stack keeps its certificates.
//!
//! The stack's brokers are named by hosts the certificates carry, which the runner may reach under another name; a
//! client connects each such host to the addresses of the host the stack maps it to, so the TLS name stays the one
//! the certificate holds.

use crate::{
    broker::{BrokerClient, SuiteTransport},
    config::{BrokerRole, ConfigError, SuiteConfig},
    exchange_log::ExchangeLog,
};
use ngsi_ld::{
    client::{Client, ReqwestTransport},
    http::EndpointError,
};
use reqwest::{Certificate, Identity, redirect::Policy};
use std::{
    fs,
    io,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::net::lookup_host;
use url::Url;

/// The longest a request may take, as for every client of the suite.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// The HTTP versions a client offers in ALPN.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpVersion {
    /// HTTP/2 and HTTP/1.1, as the broker chooses.
    #[default]
    Negotiated,
    /// HTTP/1.1 only.
    Http11,
    /// HTTP/2 only.
    Http2,
}

/// Why a TLS client could not be built.
#[derive(Debug, thiserror::Error)]
pub enum TlsError {
    /// The stack does not name the broker or its directory.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// A certificate or key file could not be read.
    #[error("{0} cannot be read")]
    File(PathBuf, #[source] io::Error),
    /// A certificate or key is not PEM the TLS stack reads.
    #[error("{0} is not a certificate or key")]
    Pem(PathBuf, #[source] reqwest::Error),
    /// A host the stack maps cannot be resolved.
    #[error("the host {0} cannot be resolved")]
    Resolve(String, #[source] io::Error),
    /// The HTTP client could not be built.
    #[error("the HTTP client could not be built")]
    Http(#[source] reqwest::Error),
    /// The broker URL cannot carry the API path.
    #[error("{0} cannot carry the API path")]
    ApiPath(Url, #[source] url::ParseError),
    /// The broker URL is not an API root.
    #[error("{0} is not an API root")]
    Endpoint(Url, #[source] Box<EndpointError>),
}

/// How one client connects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Connection {
    /// The authority trusted beside the platform's, beneath the stack's directory.
    authority: Option<PathBuf>,
    /// The certificate and key presented, beneath the stack's directory.
    identity: Option<(PathBuf, PathBuf)>,
    /// The HTTP versions offered.
    version: HttpVersion,
}

impl Connection {
    /// A connection trusting the platform's authorities, presenting no identity and offering both versions.
    #[must_use]
    pub fn new() -> Connection {
        Connection::default()
    }

    /// Also trusts the authority in the PEM file `path` beneath the stack's directory.
    #[must_use]
    pub fn trusting(self, path: &str) -> Connection {
        Connection {
            authority: Some(PathBuf::from(path)),
            ..self
        }
    }

    /// Presents the certificate and key in the PEM files `certificate` and `key` beneath the stack's directory.
    #[must_use]
    pub fn presenting(self, certificate: &str, key: &str) -> Connection {
        Connection {
            identity: Some((PathBuf::from(certificate), PathBuf::from(key))),
            ..self
        }
    }

    /// Offers `version` only.
    #[must_use]
    pub fn offering(self, version: HttpVersion) -> Connection {
        Connection { version, ..self }
    }

    /// A client of the broker of `role` connecting this way, in the default tenant.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError`] when the stack names no such broker, a file cannot be read, or the client cannot be built.
    pub async fn client(&self, role: BrokerRole) -> Result<BrokerClient, TlsError> {
        let config = SuiteConfig::get().map_err(|error| TlsError::Config(error.clone()))?;
        let broker = config.broker(role)?;
        let base_url = broker.join("ngsi-ld/v1/").map_err(|error| TlsError::ApiPath(broker.clone(), error))?;
        let transport = self.transport(config).await?;
        Client::builder()
            .base_url(base_url.clone())
            .transport(transport)
            .build()
            .map_err(|error| TlsError::Endpoint(base_url, Box::new(error)))
    }

    /// The transport of this connection.
    async fn transport(&self, config: &SuiteConfig) -> Result<SuiteTransport, TlsError> {
        let mut builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .tls_info(true);
        for (name, addresses) in resolved(config).await? {
            builder = builder.resolve_to_addrs(&name, &addresses);
        }
        let stack = config.stack_dir()?;
        if let Some(authority) = &self.authority {
            let path = stack.join(authority);
            builder = builder.add_root_certificate(Certificate::from_pem(&read(&path)?).map_err(|error| TlsError::Pem(path, error))?);
        }
        if let Some((certificate, key)) = &self.identity {
            let path = stack.join(certificate);
            let mut pem = read(&path)?;
            pem.extend(read(&stack.join(key))?);
            builder = builder.identity(Identity::from_pem(&pem).map_err(|error| TlsError::Pem(path, error))?);
        }
        builder = match self.version {
            HttpVersion::Negotiated => builder,
            HttpVersion::Http11 => builder.http1_only(),
            HttpVersion::Http2 => builder.http2_prior_knowledge(),
        };
        Ok(ExchangeLog::new(ReqwestTransport::new(builder.build().map_err(TlsError::Http)?)))
    }
}

/// The contents of `path`.
fn read(path: &Path) -> Result<Vec<u8>, TlsError> {
    fs::read(path).map_err(|error| TlsError::File(path.to_owned(), error))
}

/// The addresses each host the stack maps is connected to: those of the host it maps to.
async fn resolved(config: &SuiteConfig) -> Result<Vec<(String, Vec<SocketAddr>)>, TlsError> {
    let mut resolved = Vec::new();
    for (name, target) in config.connect_to() {
        let target = target.host().to_string();
        let addresses = lookup_host((target.as_str(), 0))
            .await
            .map_err(|error| TlsError::Resolve(target.clone(), error))?;
        resolved.push((name.host().to_string(), addresses.collect()));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use crate::tls_connection::{Connection, HttpVersion};
    use googletest::prelude::*;
    use std::path::PathBuf;

    #[gtest]
    fn a_connection_keeps_what_it_trusts_presents_and_offers() -> Result<()> {
        let connection = Connection::new()
            .trusting("tls/ca.crt")
            .presenting("tls/client.crt", "tls/client.key")
            .offering(HttpVersion::Http11);
        expect_that!(connection.authority, some(eq(&PathBuf::from("tls/ca.crt"))));
        expect_that!(
            connection.identity.as_ref().map(|(certificate, _)| certificate),
            some(eq(&PathBuf::from("tls/client.crt")))
        );
        verify_that!(connection.version, eq(HttpVersion::Http11))
    }
}
