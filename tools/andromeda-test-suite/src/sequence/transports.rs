//! The clients a sequence sends its requests with: the default one, built from the stack's host mapping, and one per
//! connection a request names, built the first time it is named.

use crate::{
    broker::SuiteTransport,
    config::SuiteConfig,
    sequence::{
        REQUEST_TIMEOUT,
        Sequence,
        request::{Connection, HttpVersion},
        sequence_error::SequenceError,
    },
};
use ngsi_ld::client::ReqwestTransport;
use reqwest::{Certificate, Identity, redirect::Policy};
use std::{fs, net::SocketAddr, path::PathBuf};
use tokio::net::lookup_host;

impl Sequence {
    /// The transport of `connection`, built the first time a request names it.
    pub(crate) fn transport(&mut self, connection: &Connection) -> Result<&SuiteTransport, SequenceError> {
        if !self.transports.iter().any(|(known, _)| known == connection) {
            let transport = self.connect(connection)?;
            // The connection is kept as the key the later requests naming it find their transport by.
            self.transports.push((connection.clone(), transport));
        }
        let step = self.step;
        self.transports
            .iter()
            .find(|(known, _)| known == connection)
            .map(|(_, transport)| transport)
            .ok_or(SequenceError::NoTransport { step })
    }

    /// A transport for `connection`, printing into the test's exchange log.
    fn connect(&self, connection: &Connection) -> Result<SuiteTransport, SequenceError> {
        let step = self.step;
        let failed = |source| SequenceError::Connection { step, source };
        let read = |template: &str| -> Result<Vec<u8>, SequenceError> {
            let path = PathBuf::from(self.fill(template)?);
            fs::read(&path).map_err(|source| SequenceError::ConnectionFile { step, path, source })
        };
        let mut builder = client_builder(&self.resolved);
        if let Some(authority) = &connection.authority {
            let certificate = Certificate::from_pem(&read(authority)?).map_err(failed)?;
            builder = builder.add_root_certificate(certificate);
        }
        if let Some((certificate, key)) = &connection.identity {
            let mut pem = read(certificate)?;
            pem.extend(read(key)?);
            builder = builder.identity(Identity::from_pem(&pem).map_err(failed)?);
        }
        builder = match connection.version {
            HttpVersion::Negotiated | HttpVersion::Http2 => builder,
            HttpVersion::Http11 => builder.http1_only(),
        };
        let http = builder.build().map_err(failed)?;
        let first = self
            .transports
            .first()
            .map(|(_, transport)| transport)
            .ok_or(SequenceError::NoTransport { step })?;
        Ok(first.beside(ReqwestTransport::new(http)))
    }
}

/// The client every connection starts from: no redirects followed, no proxy, the request timeout, the TLS
/// session's peer certificate kept for the checks that read it, and each host of `resolved` connected to at its
/// addresses.
pub(crate) fn client_builder(resolved: &[(String, Vec<SocketAddr>)]) -> reqwest::ClientBuilder {
    resolved.iter().fold(
        reqwest::Client::builder()
            .redirect(Policy::none())
            .no_proxy()
            .timeout(REQUEST_TIMEOUT)
            .tls_info(true),
        |builder, (name, addresses)| builder.resolve_to_addrs(name, addresses),
    )
}

/// The addresses of each host the stack connects a URL's host to.
pub(crate) async fn resolve(config: &SuiteConfig) -> Result<Vec<(String, Vec<SocketAddr>)>, SequenceError> {
    let mut resolved = Vec::new();
    for (name, target) in config.connect_to() {
        let host = target.host().to_string();
        let addresses = lookup_host((host.as_str(), 0)).await.map_err(|source| SequenceError::Resolve {
            // clone: the error names the host the configuration keeps
            host: target.clone(),
            source,
        })?;
        resolved.push((name.host().to_string(), addresses.collect()));
    }
    Ok(resolved)
}
