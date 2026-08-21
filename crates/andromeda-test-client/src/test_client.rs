//! Runtime-local HTTP client bound to a broker endpoint and optional tenant.

use crate::{broker_endpoint::BrokerEndpoint, response::TestResponse, tenant_name::TenantName, transport_error::TransportError};
use http::{
    HeaderValue,
    Method,
    header::{CONTENT_TYPE, HeaderName, LINK},
};
use reqwest::{Client, RequestBuilder};
use url::Url;

/// The media type whose requests carry their context inline (clause 6.3.5),
/// and therefore never a context `Link`.
const JSON_LD: &str = "application/ld+json";

/// Test-side HTTP client for one Tokio runtime and optional tenant.
#[derive(Clone, Debug)]
pub struct TestClient {
    endpoint: BrokerEndpoint,
    http: Client,
    tenant: Option<TenantName>,
    /// The JSON-LD context `Link` added to every request that names no
    /// context of its own and does not carry one inline.
    default_context: Option<HeaderValue>,
}

impl TestClient {
    /// Construct a fresh runtime-local client without probing the network.
    pub fn new(endpoint: BrokerEndpoint, tenant: Option<TenantName>) -> TestClient {
        TestClient {
            endpoint,
            http: Client::new(),
            tenant,
            default_context: None,
        }
    }

    /// Attach the JSON-LD context `Link` sent with every request that neither
    /// names a context through its own `Link` nor carries one inline in an
    /// `application/ld+json` body (clause 6.3.5).
    pub fn with_default_context(self, link: HeaderValue) -> TestClient {
        TestClient {
            default_context: Some(link),
            ..self
        }
    }

    /// The default JSON-LD context `Link`, if one is configured.
    pub const fn default_context(&self) -> Option<&HeaderValue> {
        self.default_context.as_ref()
    }

    /// A view of this client that sends no JSON-LD context unless a request
    /// names one, for requests whose point is the absence of a context.
    pub fn without_default_context(&self) -> TestClient {
        TestClient {
            endpoint: self.endpoint.clone(), // clone: the view owns its endpoint while this client keeps its own
            http: self.http.clone(),         // clone: reqwest clients share their connection pool by reference count
            tenant: self.tenant.clone(),     // clone: same as the endpoint
            default_context: None,
        }
    }

    /// Borrow the validated broker endpoint.
    pub const fn endpoint(&self) -> &BrokerEndpoint {
        &self.endpoint
    }

    /// Borrow the underlying reqwest client for intentionally low-level cases.
    pub const fn http(&self) -> &Client {
        &self.http
    }

    /// Borrow the tenant carried on every request, if configured.
    pub const fn tenant(&self) -> Option<&TenantName> {
        self.tenant.as_ref()
    }

    pub(crate) fn request(&self, method: Method, url: Url) -> RequestBuilder {
        let request = self.http.request(method, url);
        match self.tenant.as_ref() {
            Some(tenant) => request.header(HeaderName::from_static("ngsild-tenant"), tenant.header_value()),
            None => request,
        }
    }

    pub(crate) async fn send(&self, method: Method, url: Url, request: RequestBuilder) -> Result<TestResponse, TransportError> {
        let mut request = request.build().map_err(|source| TransportError::new(method.clone(), url.clone(), source))?; // clone: a build failure owns method and URL while the success path still needs them.
        if let Some(link) = &self.default_context {
            let carries_context_inline = request
                .headers()
                .get(CONTENT_TYPE)
                .is_some_and(|value| value.as_bytes().starts_with(JSON_LD.as_bytes()));
            if !request.headers().contains_key(LINK) && !carries_context_inline {
                request.headers_mut().insert(LINK, link.clone()); // clone: every request owns its header value
            }
        }
        let response = self
            .http
            .execute(request)
            .await
            .map_err(|source| TransportError::new(method.clone(), url.clone(), source))?; // clone: a send failure owns method and URL while the success path still needs them.
        let status = response.status();
        let headers = response.headers().clone(); // clone: TestResponse owns headers after reqwest's response is consumed for bytes.
        let body = response.bytes().await.map_err(|source| TransportError::new(method, url, source))?;
        Ok(TestResponse::new(status, headers, body))
    }
}

#[cfg(test)]
mod tests {
    use crate::{broker_endpoint::BrokerEndpoint, test_client::TestClient};

    #[test]
    fn construction_does_not_probe_an_unreachable_endpoint() {
        let endpoint = BrokerEndpoint::parse("http://127.0.0.1:1/ngsi-ld/v1/").unwrap_or_else(|error| unreachable!("static URL is valid: {error}"));

        let _client = TestClient::new(endpoint, None);
    }
}
