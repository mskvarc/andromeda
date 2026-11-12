//! How a client reaches a broker over TLS (clause 6.2): the authorities it trusts, the identity it presents for mutual
//! TLS, the newest TLS version it offers and the HTTP version it speaks. Paths name files beneath the stack's directory,
//! where the stack keeps its certificates.
//!
//! The stack's brokers are named by hosts the certificates carry, which the runner may reach under another name; a
//! client connects each such host to the addresses of the host the stack maps it to, so the TLS name stays the one
//! the certificate holds.

use crate::{
    broker::{BrokerClient, SuiteTransport},
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    exchange_log::ExchangeLog,
    request_version::RequestVersion,
};
use http::{Method, Version};
use ngsi_ld::{
    client::{Client, ReqwestTransport},
    http::EndpointError,
};
use reqwest::{Certificate, Identity, RequestBuilder, redirect::Policy, tls};
use std::{
    fs,
    io,
    iter,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::net::lookup_host;
use url::{Host, Url};

/// The longest a request may take, as for every test client.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// The HTTP versions a client speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HttpVersion {
    /// HTTP/2 and HTTP/1.1 over TCP, offered in ALPN for the broker to choose.
    #[default]
    Negotiated,
    /// HTTP/1.1 only.
    Http11,
    /// HTTP/2 only.
    Http2,
    /// HTTP/3 only, over QUIC.
    Http3,
}

/// The authorities a client verifies the broker's certificate against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
enum Trust {
    /// The platform's authorities.
    #[default]
    Platform,
    /// The platform's authorities and the one in this PEM file beneath the stack's directory.
    PlatformAnd(PathBuf),
    /// No authority, so no certificate verifies.
    Nothing,
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
    /// A host cannot be resolved.
    #[error("the host {0} cannot be resolved")]
    Resolve(Host<String>, #[source] io::Error),
    /// A host resolves to no address.
    #[error("the host {0} resolves to no address")]
    NoAddress(Host<String>),
    /// The broker URL names no host.
    #[error("{0} names no host")]
    NoHost(Url),
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
    /// The authorities trusted.
    trust: Trust,
    /// The certificate and key presented, beneath the stack's directory.
    identity: Option<(PathBuf, PathBuf)>,
    /// The newest TLS version offered; `None` offers every version the client supports.
    newest_tls: Option<tls::Version>,
    /// The HTTP versions spoken.
    version: HttpVersion,
}

impl Connection {
    /// A connection trusting the platform's authorities, presenting no identity, offering TLS 1.2 and 1.3, and HTTP/2
    /// and HTTP/1.1 over TCP.
    #[must_use]
    pub fn new() -> Connection {
        Connection::default()
    }

    /// Also trusts the authority in the PEM file `path` beneath the stack's directory.
    #[must_use]
    pub fn trusting(self, path: &str) -> Connection {
        Connection {
            trust: Trust::PlatformAnd(PathBuf::from(path)),
            ..self
        }
    }

    /// Trusts no authority, not even the platform's, so the handshake fails whatever certificate the broker presents.
    #[must_use]
    pub fn trusting_nothing(self) -> Connection {
        Connection { trust: Trust::Nothing, ..self }
    }

    /// Presents the certificate and key in the PEM files `certificate` and `key` beneath the stack's directory.
    #[must_use]
    pub fn presenting(self, certificate: &str, key: &str) -> Connection {
        Connection {
            identity: Some((PathBuf::from(certificate), PathBuf::from(key))),
            ..self
        }
    }

    /// Offers no TLS version newer than `newest`.
    #[must_use]
    pub fn capped_at(self, newest: tls::Version) -> Connection {
        Connection {
            newest_tls: Some(newest),
            ..self
        }
    }

    /// Speaks `version` only.
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
        let base_url = api_root(config, role)?;
        let transport = self.transport(config).await?;
        Client::builder()
            .base_url(base_url.clone())
            .transport(transport)
            .build()
            .map_err(|error| TlsError::Endpoint(base_url, Box::new(error)))
    }

    /// A plain HTTP client of the broker of `role` connecting this way, for a request the typed client cannot send.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError`] when the stack names no such broker, a file cannot be read, or the client cannot be built.
    pub async fn raw_client(&self, role: BrokerRole) -> Result<RawClient, TlsError> {
        let config = SuiteConfig::get().map_err(|error| TlsError::Config(error.clone()))?;
        Ok(RawClient {
            client: self.http_client(config).await?,
            api_root: api_root(config, role)?,
            version: self.pinned_version(),
        })
    }

    /// The transport of this connection.
    async fn transport(&self, config: &SuiteConfig) -> Result<SuiteTransport, TlsError> {
        let transport = ReqwestTransport::new(self.http_client(config).await?);
        Ok(ExchangeLog::new(match self.pinned_version() {
            Some(version) => RequestVersion::pinned(transport, version),
            None => RequestVersion::as_written(transport),
        }))
    }

    /// The version every request of this connection is sent under, when the version it is written with does not
    /// decide: `reqwest` takes the QUIC path of a client built for HTTP/3 only for a request whose version is HTTP/3.
    const fn pinned_version(&self) -> Option<Version> {
        match self.version {
            HttpVersion::Http3 => Some(Version::HTTP_3),
            HttpVersion::Negotiated | HttpVersion::Http11 | HttpVersion::Http2 => None,
        }
    }

    /// The HTTP client of this connection.
    async fn http_client(&self, config: &SuiteConfig) -> Result<reqwest::Client, TlsError> {
        let mut builder = reqwest::Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .tls_info(true);
        for (name, addresses) in resolved(config).await? {
            builder = builder.resolve_to_addrs(&name, &addresses);
        }
        let stack = config.stack_dir()?;
        builder = match &self.trust {
            Trust::Platform => builder,
            Trust::PlatformAnd(authority) => {
                let path = stack.join(authority);
                builder.add_root_certificate(Certificate::from_pem(&read(&path)?).map_err(|error| TlsError::Pem(path, error))?)
            }
            Trust::Nothing => builder.tls_certs_only(iter::empty()),
        };
        if let Some((certificate, key)) = &self.identity {
            let path = stack.join(certificate);
            let mut pem = read(&path)?;
            pem.extend(read(&stack.join(key))?);
            builder = builder.identity(Identity::from_pem(&pem).map_err(|error| TlsError::Pem(path, error))?);
        }
        if let Some(newest) = self.newest_tls {
            builder = builder.max_tls_version(newest);
        }
        let builder = match self.version {
            HttpVersion::Negotiated => builder,
            HttpVersion::Http11 => builder.http1_only(),
            HttpVersion::Http2 => builder.http2_prior_knowledge(),
            HttpVersion::Http3 => builder.http3_prior_knowledge(),
        };
        builder.build().map_err(TlsError::Http)
    }
}

/// A plain HTTP client of one broker over one connection, for a request the typed client cannot send.
#[derive(Debug, Clone)]
pub struct RawClient {
    /// The client, set up as its connection says.
    client: reqwest::Client,
    /// The broker's API root, with its trailing `/`.
    api_root: Url,
    /// The version every request is sent under; `None` leaves the client to choose.
    version: Option<Version>,
}

impl RawClient {
    /// A request with `method` to `path` below the API root, with its query.
    ///
    /// # Errors
    ///
    /// Returns [`TlsError::ApiPath`] when the API root cannot carry `path`.
    pub fn request(&self, method: Method, path: &str) -> Result<RequestBuilder, TlsError> {
        // clone: the error must own the URL it reports
        let url = self.api_root.join(path).map_err(|error| TlsError::ApiPath(self.api_root.clone(), error))?;
        let request = self.client.request(method, url);
        Ok(match self.version {
            Some(version) => request.version(version),
            None => request,
        })
    }
}

/// The API root of the broker of `role`, with its trailing `/`.
///
/// # Errors
///
/// Returns [`TlsError`] when the stack names no such broker, or its URL cannot carry the API path.
fn api_root(config: &SuiteConfig, role: BrokerRole) -> Result<Url, TlsError> {
    let broker = config.broker(role)?;
    // clone: the error must own the URL it reports
    broker.join("ngsi-ld/v1/").map_err(|error| TlsError::ApiPath(broker.clone(), error))
}

/// The contents of `path`.
///
/// # Errors
///
/// Returns [`TlsError::File`] when the file cannot be read.
pub(crate) fn read(path: &Path) -> Result<Vec<u8>, TlsError> {
    fs::read(path).map_err(|error| TlsError::File(path.to_owned(), error))
}

/// The address a client of `url` connects to: the host the stack maps the URL's host to, or the URL's host itself, at
/// the URL's port.
///
/// # Errors
///
/// Returns [`TlsError`] when the URL names no host or port, or the host resolves to no address.
pub(crate) async fn socket_address(config: &SuiteConfig, url: &Url) -> Result<SocketAddr, TlsError> {
    // clone: the error must own the URL it reports
    let host = url.host().ok_or_else(|| TlsError::NoHost(url.clone()))?.to_owned();
    let port = url.port_or_known_default().ok_or_else(|| TlsError::NoHost(url.clone()))?;
    let target = config
        .connect_to()
        .find(|(name, _)| *name.host() == host)
        .map_or(host, |(_, target)| target.host().clone());
    let text = target.to_string();
    let mut addresses = lookup_host((text.as_str(), port))
        .await
        // clone: the error owns a copy, as the host is also reported when no address resolves
        .map_err(|error| TlsError::Resolve(target.clone(), error))?;
    addresses.next().ok_or(TlsError::NoAddress(target))
}

/// The addresses each host the stack maps is connected to: those of the host it maps to.
async fn resolved(config: &SuiteConfig) -> Result<Vec<(String, Vec<SocketAddr>)>, TlsError> {
    let mut resolved = Vec::new();
    for (name, target) in config.connect_to() {
        let host = target.host().to_string();
        let addresses = lookup_host((host.as_str(), 0))
            .await
            // clone: the error must own the host it reports
            .map_err(|error| TlsError::Resolve(target.host().clone(), error))?;
        resolved.push((name.host().to_string(), addresses.collect()));
    }
    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use crate::tls_connection::{Connection, HttpVersion, Trust};
    use googletest::prelude::*;
    use reqwest::tls;
    use std::path::PathBuf;

    #[gtest]
    fn a_connection_keeps_what_it_trusts_presents_and_offers() -> Result<()> {
        let connection = Connection::new()
            .trusting("tls/ca.crt")
            .presenting("tls/client.crt", "tls/client.key")
            .capped_at(tls::Version::TLS_1_2)
            .offering(HttpVersion::Http11);
        expect_that!(connection.trust, eq(&Trust::PlatformAnd(PathBuf::from("tls/ca.crt"))));
        expect_that!(
            connection.identity.as_ref().map(|(certificate, _)| certificate),
            some(eq(&PathBuf::from("tls/client.crt")))
        );
        expect_that!(connection.newest_tls, some(eq(tls::Version::TLS_1_2)));
        verify_that!(connection.version, eq(HttpVersion::Http11))
    }

    #[gtest]
    fn trusting_nothing_replaces_an_added_authority() {
        let connection = Connection::new().trusting("tls/ca.crt").trusting_nothing();
        expect_that!(connection.trust, eq(&Trust::Nothing));
    }
}
