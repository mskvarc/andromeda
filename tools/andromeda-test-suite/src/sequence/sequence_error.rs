//! Why a sequence stopped.

use crate::{
    broker::BrokerError,
    config::HostName,
    config_error::ConfigError,
    sequence::{
        body::BodyError,
        expect::Verdict,
        mqtt_relay::RelayError,
        parameters::ParameterRefusal,
        peer_certificate::CertificateError,
        stub_server::StubServerError,
        variables::TemplateError,
    },
};
use http::uri::InvalidUri;
use ngsi_ld::client::ReqwestError;
use std::{io, path::PathBuf};

/// Why a sequence stopped.
#[derive(Debug, thiserror::Error)]
pub enum SequenceError {
    /// The stack's configuration lacks a value the sequence needs.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The HTTP client cannot be built.
    #[error("the HTTP client cannot be built")]
    Http(#[source] reqwest::Error),
    /// A certificate or key file a request's client names cannot be read.
    #[error("request {step}: the client cannot be built: {} cannot be read", path.display())]
    ConnectionFile {
        /// The request, counted from 1.
        step: usize,
        /// The file.
        path: PathBuf,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The client a request names cannot be built from its authority or identity.
    #[error("request {step}: the client cannot be built")]
    Connection {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: reqwest::Error,
    },
    /// The sequence holds no transport to build a request's client beside.
    #[error("request {step}: the sequence has no transport")]
    NoTransport {
        /// The request, counted from 1.
        step: usize,
    },
    /// The stub server failed.
    #[error(transparent)]
    Stubs(#[from] StubServerError),
    /// A template cannot be filled.
    #[error("request {step}: {source}")]
    Template {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: TemplateError,
    },
    /// A body cannot be sent.
    #[error("request {step}: {source}")]
    Body {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: BodyError,
    },
    /// The query parameters are refused by their typed model.
    #[error("request {step}: {source}")]
    Parameters {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: ParameterRefusal,
    },
    /// The filled target is not a URI.
    #[error("request {step}: the target is not a URI: {source}")]
    Target {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: InvalidUri,
    },
    /// The request is not an HTTP request.
    #[error("request {step}: the request cannot be built: {source}")]
    Malformed {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: http::Error,
    },
    /// The request could not be sent or its answer read.
    #[error("request {step}: the exchange failed: {source}")]
    Transport {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: ReqwestError,
    },
    /// The certificate the server presented cannot be read.
    #[error("request {step}: {source}")]
    Certificate {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        #[source]
        source: CertificateError,
    },
    /// A host the stack connects a URL's host to cannot be resolved.
    #[error("the host {} cannot be resolved", host.host())]
    Resolve {
        /// The host.
        host: HostName,
        /// Why.
        #[source]
        source: io::Error,
    },
    /// The private tenant could not be brought up.
    #[error(transparent)]
    Tenant(#[from] BrokerError),
    /// The MQTT relay could not start.
    #[error(transparent)]
    Relay(#[from] RelayError),
    /// The answer did not meet its expectation.
    #[error("request {step}: the answer does not meet its expectation:\n  {}", verdict.failures.join("\n  "))]
    Unmet {
        /// The request, counted from 1.
        step: usize,
        /// The verdict naming every unmet requirement.
        verdict: Verdict,
    },
}
