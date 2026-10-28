//! A test as a sequence of requests, each with what its answer must meet, sharing the variables earlier answers
//! captured.
//!
//! The sequence stops at the first answer that fails its expectation and reports every requirement that answer did
//! not meet. A retried request is sent again until its answer meets the expectation, within the larger of its own
//! budget and the delivery deadline, since a passing wait ends at its first passing attempt.
//!
//! Each sequence draws its own namespace and runs its own stub server, whose address the brokers reach as
//! `{{mock_internal}}` and the test process as `{{mock}}`.

pub mod answer;
pub mod body;
pub mod expect;
pub mod mqtt_relay;
pub mod parameters;
pub mod payload;
pub mod peer_certificate;
pub mod predicate;
pub mod queried;
pub mod query;
pub mod request;
pub mod stub_server;
pub mod variables;
pub mod written_json;

use crate::{
    broker::{self, BrokerError, SuiteTransport},
    cleanup::{Cleanup, Removal},
    config::{BrokerRole, Budget, ConfigError, SuiteConfig},
    eventually::delivery_deadline,
    exchange_log::ExchangeLog,
    namespace::Namespace,
    sequence::{
        answer::Answer,
        body::{BodyError, BodyKind},
        expect::{Expect, Verdict},
        mqtt_relay::{MqttRelay, RelayError},
        parameters::{ParameterRefusal, check},
        peer_certificate::PeerCertificate,
        request::{Action, Connection, HttpRequest, HttpVersion, Parameters, Request},
        stub_server::{StubServer, StubServerError},
        variables::{TemplateError, Variables},
    },
};
use bytes::Bytes;
use http::{
    HeaderMap,
    HeaderName,
    HeaderValue,
    Method,
    Request as HttpMessage,
    StatusCode,
    Uri,
    header::{ACCEPT, CONTENT_TYPE, LOCATION},
};
use ngsi_ld::client::{ReqwestTransport, Transport};
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use reqwest::{Certificate, Identity, redirect::Policy, tls::TlsInfo};
use std::{fs, net::SocketAddr, time::Duration};
use tokio::{
    net::lookup_host,
    time::{Instant, sleep},
};
use url::Url;

/// The characters a query parameter keeps as they are: the unreserved characters of RFC 3986.
const QUERY_KEPT: &AsciiSet = &NON_ALPHANUMERIC.remove(b'-').remove(b'.').remove(b'_').remove(b'~');

/// The header naming the tenant of a request (clause 6.3.14).
const TENANT: &str = "NGSILD-Tenant";

/// The longest a request may take: beyond every forward timeout a broker waits out before answering.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// Why a sequence stopped.
#[derive(Debug, thiserror::Error)]
pub enum SequenceError {
    /// The stack's configuration lacks a value the sequence needs.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The HTTP client cannot be built.
    #[error("the HTTP client cannot be built")]
    Http(#[source] reqwest::Error),
    /// The client a request names cannot be built: its authority or identity cannot be read.
    #[error("request {step}: the client cannot be built: {reason}")]
    Connection {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        reason: String,
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
    /// The request is not an HTTP request.
    #[error("request {step}: {reason}")]
    Malformed {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        reason: String,
    },
    /// The request could not be sent or its answer read.
    #[error("request {step}: the exchange failed: {reason}")]
    Transport {
        /// The request, counted from 1.
        step: usize,
        /// Why.
        reason: String,
    },
    /// A host the stack connects a URL's host to cannot be resolved.
    #[error("the host {host} cannot be resolved: {reason}")]
    Resolve {
        /// The host.
        host: String,
        /// Why.
        reason: String,
    },
    /// The private tenant could not be brought up.
    #[error(transparent)]
    Tenant(#[from] BrokerError),
    /// The MQTT relay could not start.
    #[error(transparent)]
    Relay(#[from] RelayError),
    /// The answer did not meet its expectation.
    #[error("request {step}: the answer does not meet its expectation:\n  {}", failures.join("\n  "))]
    Unmet {
        /// The request, counted from 1.
        step: usize,
        /// Every unmet requirement.
        failures: Vec<String>,
    },
}

/// The requests of one test.
#[derive(Debug)]
pub struct Sequence {
    /// The values templates are filled from.
    variables: Variables,
    /// The transport of each connection a request named, the default one first; all print into one exchange log.
    transports: Vec<(Connection, SuiteTransport)>,
    /// Each host name a URL may name and the addresses a client connects to in its place.
    resolved: Vec<(String, Vec<SocketAddr>)>,
    /// The test's stub server.
    stubs: StubServer,
    /// The test's namespace, which `{{ns}}` names.
    namespace: Namespace,
    /// The number of requests sent so far.
    step: usize,
    /// The relay of the test's MQTT messages into the stub server, once started.
    relay: Option<MqttRelay>,
    /// The tenant every broker request names when it names none itself.
    tenant: Option<String>,
    /// The registrations and subscriptions the test created, by tenant, deleted when the sequence ends: a test that
    /// leaves one behind would otherwise answer, or be notified for, the requests of tests that run after it on a
    /// kept stack.
    cleanups: Vec<(Option<String>, Cleanup)>,
}

impl Sequence {
    /// A sequence with a fresh namespace and stub server, and the variables every test reads: `broker`, `ns`,
    /// `mock`, `mock_internal`, `unreachable_origin`, `core_context`, the MQTT hosts and port and the budgets.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError`] when the configuration lacks a value or a server cannot start.
    pub async fn start() -> Result<Sequence, SequenceError> {
        let config = SuiteConfig::get().map_err(|error| SequenceError::Config(error.clone()))?;
        let stubs = StubServer::start().await?;
        let mut variables = Variables::default();
        let namespace = Namespace::fresh();
        variables.set_text("ns", namespace.token());
        variables.set_text("broker", config.broker(BrokerRole::Front)?.as_str().trim_end_matches('/'));
        variables.set_text("mock_internal", stubs.base()?);
        variables.set_text("mock", stubs.local()?);
        variables.set_text("unreachable_origin", config.unreachable_origin().as_str().trim_end_matches('/'));
        variables.set_text("core_context", config.core_context().as_str());
        if let Ok(mqtt) = config.mqtt() {
            variables.set_text("mqtt_host", mqtt.host().to_string());
            variables.set_text("mqtt_auth_host", mqtt.auth_host().to_string());
            variables.set_text("mqtt_non_default_port", mqtt.non_default_port().to_string());
        }
        for (role, url) in config.brokers() {
            let name = if role == BrokerRole::Front {
                "broker".to_owned()
            } else {
                format!("broker_{}", <&str>::from(role).to_lowercase())
            };
            variables.set_text(&name, url.as_str().trim_end_matches('/'));
        }
        for (role, url) in config.peers() {
            variables.set_text(&format!("peer_{}", <&str>::from(role).to_lowercase()), url.as_str().trim_end_matches('/'));
        }
        if let Some(port) = config.broker(BrokerRole::Tls).ok().and_then(Url::port_or_known_default) {
            variables.set_text("broker_tls_port", port.to_string());
        }
        if let Ok(api_root) = config.api_root_prefixed() {
            variables.set_text("api_root_prefixed", api_root.as_str().trim_end_matches('/'));
        }
        if let Ok(directory) = config.stack_dir() {
            variables.set_text("stack_dir", directory.display().to_string());
        }
        for (name, budget) in [
            ("request_value_budget", Budget::RequestValue),
            ("entity_value_budget", Budget::EntityValue),
            ("position_budget", Budget::Position),
            ("large_position_budget", Budget::LargePosition),
            ("max_request_body", Budget::MaxRequestBody),
        ] {
            if let Ok(value) = config.budget(budget) {
                variables.set_text(name, value.to_string());
            }
        }
        let resolved = resolve(config).await?;
        let http = client_builder(&resolved).build().map_err(SequenceError::Http)?;
        Ok(Sequence {
            variables,
            transports: vec![(Connection::default(), ExchangeLog::new(ReqwestTransport::new(http)))],
            resolved,
            stubs,
            namespace,
            step: 0,
            relay: None,
            tenant: None,
            cleanups: Vec::new(),
        })
    }

    /// A sequence like [`start`](Self::start) for a test whose requests name the tenant `{{ns}}` themselves: the
    /// tenant is brought up first, waiting out a broker that is saturated by bringing up many tenants at once, so the
    /// test's first request finds it.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError`] as [`start`](Self::start) does, or when the tenant cannot be brought up.
    pub async fn start_with_provisioned_tenant() -> Result<Sequence, SequenceError> {
        let sequence = Sequence::start().await?;
        broker::private_tenant(&broker::front()?, &sequence.namespace).await?;
        Ok(sequence)
    }

    /// A sequence like [`start`](Self::start) whose broker requests name the tenant `{{ns}}` unless they name one
    /// themselves: for a test whose requests read everything the tenant holds, which the shared default tenant fills
    /// with every other test's data. The tenant is brought up before the first request, so a first request that
    /// creates no tenant (Add @context, a query) finds it.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError`] as [`start`](Self::start) does.
    pub async fn start_in_private_tenant() -> Result<Sequence, SequenceError> {
        let mut sequence = Sequence::start().await?;
        broker::private_tenant(&broker::front()?, &sequence.namespace).await?;
        sequence.tenant = Some(sequence.fill("{{ns}}")?);
        Ok(sequence)
    }

    /// Relays every MQTT message published below the test's namespace, on every MQTT broker of the stack, into the
    /// stub server's journal at `/mqtt/<host>/<topic>`.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError`] when the stack runs no MQTT broker or a broker does not acknowledge the subscription.
    pub async fn relay_mqtt(&mut self) -> Result<(), SequenceError> {
        let config = SuiteConfig::get().map_err(|error| SequenceError::Config(error.clone()))?;
        let mqtt = config.mqtt()?;
        let hosts = [mqtt.host().to_string(), mqtt.auth_host().to_string()];
        let namespace = self.fill("{{ns}}")?;
        let local = self.stubs.local()?;
        self.relay = Some(MqttRelay::start(&hosts, mqtt.observer_port(), &namespace, &local).await?);
        Ok(())
    }

    /// The variables, for a test that sets or reads one itself.
    pub const fn variables(&mut self) -> &mut Variables {
        &mut self.variables
    }

    /// Sends `request` and checks its answer against `expect`, as often as the request's repeat and retry say.
    ///
    /// # Errors
    ///
    /// Returns [`SequenceError`] when the request cannot be sent or its answer does not meet `expect`.
    pub async fn send(&mut self, request: Request, expect: Expect) -> Result<(), SequenceError> {
        self.step += 1;
        for _ in 0..request.repeat.max(1) {
            self.attempts(&request, &expect).await?;
        }
        Ok(())
    }

    /// Sends `request` until its answer meets `expect` or its retries are spent.
    async fn attempts(&mut self, request: &Request, expect: &Expect) -> Result<(), SequenceError> {
        let Some(retry) = request.retry else {
            let answer = self.answer(&request.action).await?;
            return self.judge(&answer, expect);
        };
        let budget = retry.interval.saturating_mul(retry.count).max(delivery_deadline());
        let deadline = Instant::now() + budget;
        loop {
            let outcome = match self.answer(&request.action).await {
                Ok(answer) => self.judge(&answer, expect),
                Err(error) => Err(error),
            };
            match outcome {
                Ok(()) => return Ok(()),
                Err(error) if Instant::now() >= deadline => return Err(error),
                Err(_) => sleep(retry.interval).await,
            }
        }
    }

    /// Whether `answer` meets `expect`.
    fn judge(&mut self, answer: &Answer, expect: &Expect) -> Result<(), SequenceError> {
        let Verdict { failures, .. } = expect.verdict(answer, &mut self.variables);
        if failures.is_empty() {
            Ok(())
        } else {
            Err(SequenceError::Unmet { step: self.step, failures })
        }
    }

    /// `template` filled.
    fn fill(&self, template: &str) -> Result<String, SequenceError> {
        self.variables
            .fill(template)
            .map_err(|source| SequenceError::Template { step: self.step, source })
    }

    /// The answer to `action`.
    async fn answer(&mut self, action: &Action) -> Result<Answer, SequenceError> {
        let admin = |path: &str| format!("{}/__admin/{path}", self.stubs.local().unwrap_or_default());
        Ok(match action {
            Action::Http(request) => self.exchange(request).await?,
            Action::SetStub(mapping) => {
                let (status, body) = self.stubs.set(&self.fill(mapping)?)?;
                Answer::json(status, body, admin("mappings"))
            }
            Action::RemoveStub(id) => {
                let status = self.stubs.remove(&self.fill(id)?);
                Answer::json(status, "{}".to_owned(), admin("mappings"))
            }
            Action::RemoveStubs(pattern) => {
                let status = self.stubs.remove_by_metadata(&self.fill(pattern)?)?;
                Answer::json(status, "{}".to_owned(), admin("mappings/remove-by-metadata"))
            }
            Action::FindRequests(pattern) => Answer::json(200, self.stubs.find(&self.fill(pattern)?).await?, admin("requests/find")),
            Action::CountRequests(pattern) => Answer::json(200, self.stubs.count(&self.fill(pattern)?).await?, admin("requests/count")),
        })
    }

    /// Records for deletion the registration or subscription a create answered with `headers` made, when `url` is
    /// one of the broker's registration or subscription collections.
    fn remember(&mut self, url: &str, tenant: Option<String>, headers: &HeaderMap) {
        let Ok(broker) = self.fill("{{broker}}") else { return };
        let Some(collection) = url.strip_prefix(&broker).map(|path| path.split('?').next().unwrap_or_default()) else {
            return;
        };
        let Some(id) = headers
            .get(LOCATION)
            .and_then(|location| location.to_str().ok())
            .and_then(|location| location.rsplit('/').next())
            .map(|id| percent_decode_str(id).decode_utf8_lossy().into_owned())
        else {
            return;
        };
        let removal = match collection {
            "/ngsi-ld/v1/csourceRegistrations" => id.parse().ok().map(Removal::Registration),
            "/ngsi-ld/v1/subscriptions" => id.parse().ok().map(Removal::Subscription),
            "/ngsi-ld/v1/csourceSubscriptions" => id.parse().ok().map(Removal::RegistrationSubscription),
            _ => None,
        };
        let Some(removal) = removal else { return };
        if let Some((_, cleanup)) = self.cleanups.iter_mut().find(|(owner, _)| *owner == tenant) {
            cleanup.remove(removal);
            return;
        }
        let Ok(client) = broker::front() else { return };
        let client = match tenant.as_deref().map(str::parse) {
            Some(Ok(named)) => client.with_tenant(named),
            Some(Err(_)) => return,
            None => client,
        };
        self.cleanups.push((tenant, Cleanup::of(&client).removing(removal)));
    }

    /// Sends the HTTP `request` and reads its answer.
    async fn exchange(&mut self, request: &HttpRequest) -> Result<Answer, SequenceError> {
        let step = self.step;
        let mut url = self.fill(&request.url)?;
        let query = request
            .query
            .iter()
            .map(|(name, value)| {
                Ok(format!(
                    "{}={}",
                    utf8_percent_encode(&self.fill(name)?, QUERY_KEPT),
                    utf8_percent_encode(&self.fill(value)?, QUERY_KEPT)
                ))
            })
            .collect::<Result<Vec<_>, SequenceError>>()?
            .join("&");
        if !query.is_empty() {
            url.push(if url.contains('?') { '&' } else { '?' });
            url.push_str(&query);
        }
        let uri = url.parse::<Uri>().map_err(|error| SequenceError::Malformed {
            step,
            reason: format!("{url} is not a URI: {error}"),
        })?;
        if request.parameters == Parameters::Typed
            && let Some(sent) = uri.query()
        {
            check(&request.method, uri.path(), sent).map_err(|source| SequenceError::Parameters { step, source })?;
        }
        let body = request.body.bytes(&self.variables).map_err(|source| SequenceError::Body { step, source })?;
        let mut message = HttpMessage::builder().method(request.method.clone()).uri(uri);
        let named = |name: &HeaderName| request.headers.iter().any(|(sent, _)| sent.eq_ignore_ascii_case(name.as_str()));
        if !named(&ACCEPT) {
            message = message.header(ACCEPT, HeaderValue::from_static("*/*"));
        }
        if matches!(body, Some((_, BodyKind::Json))) && !named(&CONTENT_TYPE) {
            message = message.header(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        }
        for (name, value) in &request.headers {
            message = message.header(name.as_str(), self.fill(value)?);
        }
        if let Some(tenant) = &self.tenant
            && !request.headers.iter().any(|(name, _)| name.eq_ignore_ascii_case(TENANT))
            && url.starts_with(&self.fill("{{broker}}")?)
        {
            message = message.header(TENANT, tenant.as_str());
        }
        let message = message
            .body(body.map_or_else(Bytes::new, |(text, _)| Bytes::from(text)))
            .map_err(|error| SequenceError::Malformed {
                step,
                reason: error.to_string(),
            })?;
        let tenant = message.headers().get(TENANT).and_then(|value| value.to_str().ok()).map(str::to_owned);
        let started = Instant::now();
        let transport = self.transport(&request.connection)?;
        let response = transport.send(message).await.map_err(|error| SequenceError::Transport {
            step,
            reason: error.to_string(),
        })?;
        let certificate = response
            .extensions()
            .get::<TlsInfo>()
            .and_then(TlsInfo::peer_certificate)
            .map(PeerCertificate::from_der)
            .transpose()
            .map_err(|error| SequenceError::Transport {
                step,
                reason: error.to_string(),
            })?;
        if request.method == Method::POST && response.status() == StatusCode::CREATED {
            self.remember(&url, tenant, response.headers());
        }
        Ok(Answer {
            status: response.status().as_u16(),
            version: response.version(),
            headers: response.headers().clone(),
            url,
            duration: started.elapsed(),
            body: response.into_body(),
            certificate,
        })
    }

    /// The transport of `connection`, built the first time a request names it.
    fn transport(&mut self, connection: &Connection) -> Result<&SuiteTransport, SequenceError> {
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
            .ok_or_else(|| SequenceError::Connection {
                step,
                reason: "the transport is gone".to_owned(),
            })
    }

    /// A transport for `connection`, printing into the test's exchange log.
    fn connect(&self, connection: &Connection) -> Result<SuiteTransport, SequenceError> {
        let step = self.step;
        let failed = |reason: String| SequenceError::Connection { step, reason };
        let read = |template: &str| -> Result<Vec<u8>, SequenceError> {
            let path = self.fill(template)?;
            fs::read(&path).map_err(|error| failed(format!("{path}: {error}")))
        };
        let mut builder = client_builder(&self.resolved);
        if let Some(authority) = &connection.authority {
            let certificate = Certificate::from_pem(&read(authority)?).map_err(|error| failed(error.to_string()))?;
            builder = builder.add_root_certificate(certificate);
        }
        if let Some((certificate, key)) = &connection.identity {
            let mut pem = read(certificate)?;
            pem.extend(read(key)?);
            builder = builder.identity(Identity::from_pem(&pem).map_err(|error| failed(error.to_string()))?);
        }
        builder = match connection.version {
            HttpVersion::Negotiated | HttpVersion::Http2 => builder,
            HttpVersion::Http11 => builder.http1_only(),
        };
        let http = builder.build().map_err(|error| failed(error.to_string()))?;
        let first = self
            .transports
            .first()
            .map(|(_, transport)| transport)
            .ok_or_else(|| failed("the sequence has no transport".to_owned()))?;
        Ok(first.beside(ReqwestTransport::new(http)))
    }
}

/// The client every connection starts from: no redirects followed, no proxy, the request timeout, the TLS
/// session's peer certificate kept for the checks that read it, and each host of `resolved` connected to at its
/// addresses.
fn client_builder(resolved: &[(String, Vec<SocketAddr>)]) -> reqwest::ClientBuilder {
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
async fn resolve(config: &SuiteConfig) -> Result<Vec<(String, Vec<SocketAddr>)>, SequenceError> {
    let mut resolved = Vec::new();
    for (name, target) in config.connect_to() {
        let target = target.host().to_string();
        let addresses = lookup_host((target.as_str(), 0)).await.map_err(|error| SequenceError::Resolve {
            host: target.clone(),
            reason: error.to_string(),
        })?;
        resolved.push((name.host().to_string(), addresses.collect()));
    }
    Ok(resolved)
}
