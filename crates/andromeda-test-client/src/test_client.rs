//! Runtime-local HTTP client bound to a broker endpoint and optional tenant.

use crate::{broker_endpoint::BrokerEndpoint, response::TestResponse, tenant_name::TenantName, transport_error::TransportError};
use http::{Method, header::HeaderName};
use reqwest::{Client, RequestBuilder};
use url::Url;

/// Test-side HTTP client for one Tokio runtime and optional tenant.
#[derive(Clone, Debug)]
pub struct TestClient {
    endpoint: BrokerEndpoint,
    http: Client,
    tenant: Option<TenantName>,
}

impl TestClient {
    /// Construct a fresh runtime-local client without probing the network.
    pub fn new(endpoint: BrokerEndpoint, tenant: Option<TenantName>) -> TestClient {
        TestClient {
            endpoint,
            http: Client::new(),
            tenant,
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
        let response = request
            .send()
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
