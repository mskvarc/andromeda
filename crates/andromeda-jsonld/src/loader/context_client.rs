//! The HTTP client a remote `@context` is downloaded with.
//!
//! A `@context` URL is named by a client, so a download is a request the
//! client aims, and every address it reaches is held to the configured address
//! policy: the URL itself by the pre-send guard, each redirect hop by the
//! outbound client's redirect policy and connect-time resolver, and a JSON-LD
//! alternate by the guard again, since it is requested as a URL of its own.

use crate::loader::error::ContextUnavailable;
use andromeda_http_headers::json_ld_accept::JsonLdAccept;
use andromeda_outbound_http::{
    address_policy::AddressPolicy,
    client::{DEFAULT_POOL_IDLE_TIMEOUT, OutboundHttpPolicy},
    endpoint_guard::PrivateIpGuard,
    endpoint_refusal::EndpointRefusal,
};
use headers::HeaderMapExt;
use iri_rs::IriBuf;
use reqwest::{Client, Response, header::HeaderMap};
use std::{sync::Arc, time::Duration};

/// A client that downloads `@context` documents only from addresses the
/// policy admits.
///
/// Cloning shares the connection pool and the guard's cached outcomes.
#[derive(Debug, Clone)]
pub(crate) struct ContextClient {
    /// The client a download goes out on, following redirects it vets.
    client: Client,
    /// The guard that vets each URL a download starts from.
    guard: PrivateIpGuard,
}

impl ContextClient {
    /// A client whose downloads take at most `timeout`, follow at most
    /// `max_redirects` redirects and reach only addresses `addresses` admits.
    ///
    /// # Errors
    ///
    /// Returns the [`reqwest::Error`] the client builder reports.
    pub(crate) fn new(addresses: AddressPolicy, timeout: Duration, max_redirects: u32) -> Result<ContextClient, reqwest::Error> {
        let policy = OutboundHttpPolicy {
            timeout,
            pool_idle_timeout: DEFAULT_POOL_IDLE_TIMEOUT,
            addresses,
        };
        // A limit wider than the address space saturates: no chain that long
        // can be followed, so `usize::MAX` follows exactly the same chains.
        let client = policy
            .redirecting_client_builder(usize::try_from(max_redirects).unwrap_or(usize::MAX))
            .build()?;
        Ok(ContextClient { client, guard: policy.guard() })
    }

    /// Request `target` as JSON-LD.
    ///
    /// # Errors
    ///
    /// Returns [`ContextUnavailable::Refused`] when the policy refuses `target`
    /// or a redirect it leads to, and [`ContextUnavailable::Request`] for any
    /// other failure to obtain a response.
    pub(crate) async fn get(&self, target: &IriBuf) -> Result<Response, ContextUnavailable> {
        let mut request_headers = HeaderMap::new();
        request_headers.typed_insert(JsonLdAccept);
        let request = self.client.get(target.as_str()).headers(request_headers).build().map_err(request_failure)?;
        self.guard.check(request.url()).await.map_err(ContextUnavailable::Refused)?;
        self.client.execute(request).await.map_err(request_failure)
    }
}

/// Why a request produced no response: the refusal the resolver or the
/// redirect policy raised inside `reqwest`, or the transport failure itself.
fn request_failure(error: reqwest::Error) -> ContextUnavailable {
    match EndpointRefusal::find_in(&error) {
        Some(refusal) => ContextUnavailable::Refused(refusal.clone()), // clone: the refusal is borrowed from the error it is found in
        None => ContextUnavailable::Request(Arc::new(error)),
    }
}
