//! Clients of the stack's brokers, and the private tenant a test brings up when no selector can scope its subject.
//!
//! Every client of a test prints its exchanges ([`ExchangeLog`]) and speaks to the broker over HTTP/1.1 or HTTP/2 as
//! the broker offers, following no redirect, through no proxy and with no content decoding.

use crate::{
    config::{BrokerRole, PeerRole, SuiteConfig},
    config_error::ConfigError,
    eventually::{Poll, TimedOut, delivery_deadline, eventually},
    exchange_log::ExchangeLog,
    namespace::{Namespace, NamespaceError},
    request_version::RequestVersion,
};
use ngsi_ld::{
    client::{Client, ClientError, ReqwestError, ReqwestTransport},
    http::EndpointError,
    model::{BuildError, Tenant, UriError},
    registration::{CSourceRegistration, Problem, RegistrationId, RegistrationInfo},
};
use reqwest::redirect::Policy;
use std::time::Duration;
use url::Url;

/// The longest a request may take: beyond every forward timeout a broker waits out before answering.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(120);

/// The transport every test client shares.
pub type SuiteTransport = ExchangeLog<RequestVersion<ReqwestTransport>>;

/// A client of one broker of the stack.
pub type BrokerClient = Client<SuiteTransport>;

/// Why a client could not be built or a tenant brought up.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    /// The stack does not name the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The HTTP client could not be built.
    #[error("the HTTP client could not be built")]
    Http(#[source] reqwest::Error),
    /// The broker URL is not an API root.
    #[error("{0} is not an API root")]
    Endpoint(Url, #[source] Box<EndpointError>),
    /// The broker URL cannot carry the API path.
    #[error("{0} cannot carry the API path")]
    ApiPath(Url, #[source] url::ParseError),
    /// A namespaced value is invalid.
    #[error(transparent)]
    Namespace(#[from] NamespaceError),
    /// The provisioning registration's identifier is invalid.
    #[error("the provisioning registration cannot be built")]
    RegistrationId(#[source] UriError),
    /// The provisioning registration or its information is invalid.
    #[error("the provisioning registration cannot be built")]
    Registration(#[source] BuildError<Problem>),
    /// The broker stayed saturated for the whole delivery deadline.
    #[error("the broker stayed saturated while bringing up the tenant")]
    Saturated(#[source] TimedOut<ClientError<ReqwestError>>),
    /// The broker refused a provisioning request.
    #[error("the broker refused to bring up the tenant")]
    Provisioning(#[source] Box<ClientError<ReqwestError>>),
}

/// A client of the broker of `role`, in the default tenant, sending no `@context`.
///
/// # Errors
///
/// Returns [`BrokerError`] when the stack runs no such broker or the client cannot be built.
pub fn client(role: BrokerRole) -> Result<BrokerClient, BrokerError> {
    client_at(api_root(role)?)
}

/// A client of the broker of `role` that prints none of its exchanges, for a test generating load whose thousands of
/// exchanges it reports as a tally.
///
/// # Errors
///
/// Returns [`BrokerError`] when the stack runs no such broker or the client cannot be built.
pub fn quiet_client(role: BrokerRole) -> Result<BrokerClient, BrokerError> {
    let transport = ExchangeLog::quiet(RequestVersion::as_written(ReqwestTransport::new(http_client()?)));
    built(api_root(role)?, transport)
}

/// A client of the API root `base_url`, in the default tenant, sending no `@context`.
///
/// # Errors
///
/// Returns [`BrokerError`] when the client cannot be built.
pub fn client_at(base_url: Url) -> Result<BrokerClient, BrokerError> {
    let transport = ExchangeLog::new(RequestVersion::as_written(ReqwestTransport::new(http_client()?)));
    built(base_url, transport)
}

/// The API root of the broker of `role`.
fn api_root(role: BrokerRole) -> Result<Url, BrokerError> {
    let config = SuiteConfig::get().map_err(|error| BrokerError::Config(error.clone()))?;
    let broker = config.broker(role)?;
    broker.join("ngsi-ld/v1/").map_err(|error| BrokerError::ApiPath(broker.clone(), error)) // clone: the error must own the URL it reports
}

/// The HTTP client every test client sends through.
fn http_client() -> Result<reqwest::Client, BrokerError> {
    reqwest::Client::builder()
        .redirect(Policy::none())
        .no_proxy()
        .timeout(REQUEST_TIMEOUT)
        .build()
        .map_err(BrokerError::Http)
}

/// The client of the API root `base_url` sending through `transport`.
fn built(base_url: Url, transport: SuiteTransport) -> Result<BrokerClient, BrokerError> {
    Client::builder()
        .base_url(base_url.clone()) // clone: the builder takes a copy, and the error still reports the API root
        .transport(transport)
        .build()
        .map_err(|error| BrokerError::Endpoint(base_url, Box::new(error)))
}

/// The broker of `role` as another broker of the stack reaches it, the `endpoint` a registration of it names.
///
/// # Errors
///
/// Returns [`BrokerError::Config`] when the stack names no such peer.
pub fn peer(role: PeerRole) -> Result<Url, BrokerError> {
    let config = SuiteConfig::get().map_err(|error| BrokerError::Config(error.clone()))?;
    // The configuration is shared by every test of the process; a registration builder owns its endpoint.
    Ok(config.peer(role)?.clone())
}

/// A client of the front broker.
///
/// # Errors
///
/// Returns [`BrokerError`] when the stack names no front broker or the client cannot be built.
pub fn front() -> Result<BrokerClient, BrokerError> {
    client(BrokerRole::Front)
}

/// The status a broker answers while the tenant it is bringing up is saturated, asking to be asked again.
const SATURATED: u16 = 503;

/// Brings up the tenant named after `namespace` on the broker `client` speaks to, and returns `client` on it, as
/// [`provisioned_tenant`] does.
///
/// # Errors
///
/// Returns [`BrokerError`] as [`provisioned_tenant`] does, or when `namespace` names no tenant.
pub async fn private_tenant(client: &BrokerClient, namespace: &Namespace) -> Result<BrokerClient, BrokerError> {
    provisioned_tenant(client, namespace.tenant()?, namespace).await
}

/// Brings up `tenant` on the broker `client` speaks to, and returns `client` on it. The registration that brings it up
/// is named after `namespace`.
///
/// Add @context is not one of the operations clause 5.5.10 lets create a tenant, so the tenant comes up with Create
/// Context Source Registration, which is, and the registration is deleted again: the tenant then holds only the core
/// `@context`. The registration names no `@context` and a type no test uses.
///
/// A broker bringing up many tenants at once answers the create with 503 ("temporarily saturated; retry with
/// backoff") once its own deadline passes; the create is then sent again until the delivery deadline. The answer that
/// is not a 503 is the one checked.
///
/// # Errors
///
/// Returns [`BrokerError::Provisioning`] when the broker answers the create with anything but 201 or the delete with
/// anything but 204.
pub async fn provisioned_tenant(client: &BrokerClient, tenant: Tenant, namespace: &Namespace) -> Result<BrokerClient, BrokerError> {
    let tenant = client.with_tenant(tenant);
    let config = SuiteConfig::get().map_err(|error| BrokerError::Config(error.clone()))?;
    let id: RegistrationId = namespace
        .urn_text("ContextSourceRegistration", "provision")
        .parse()
        .map_err(BrokerError::RegistrationId)?;
    let information = RegistrationInfo::builder()
        .entities(["ProvisioningSentinel"])
        .build()
        .map_err(BrokerError::Registration)?;
    let registration = CSourceRegistration::builder(config.unreachable_origin().as_str())
        .id(id.clone())
        .information([information])
        .build()
        .map_err(BrokerError::Registration)?;
    let registered = eventually(delivery_deadline(), || async {
        match tenant.registrations().register(&registration).send().await {
            Err(error) if error.problem().and_then(|problem| problem.status) == Some(SATURATED) => Poll::Pending(error),
            other => Poll::Ready(other),
        }
    })
    .await;
    match registered {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => return Err(BrokerError::Provisioning(Box::new(error))),
        Err(saturated) => return Err(BrokerError::Saturated(saturated)),
    }
    tenant
        .registrations()
        .delete(&id)
        .send()
        .await
        .map_err(|error| BrokerError::Provisioning(Box::new(error)))?;
    Ok(tenant)
}
