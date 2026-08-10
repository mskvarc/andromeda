//! Validated endpoint of the broker under test.

use crate::transport_error::EndpointError;
use std::env;
use url::Url;

/// Environment variable containing the broker's NGSI-LD v1 base URL.
pub const BROKER_ENDPOINT_ENV: &str = "ANDROMEDA_TEST_BROKER_URL";

/// Return the default endpoint used when [`BROKER_ENDPOINT_ENV`] is absent.
pub const fn default_broker_endpoint() -> &'static str {
    "http://127.0.0.1:1056/ngsi-ld/v1/"
}

/// Validated NGSI-LD v1 endpoint of the broker under test.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrokerEndpoint {
    url: Url,
}

impl BrokerEndpoint {
    /// Read and validate the broker endpoint from the process environment.
    pub fn from_environment() -> Result<BrokerEndpoint, EndpointError> {
        let value = env::var(BROKER_ENDPOINT_ENV).unwrap_or_else(|_| default_broker_endpoint().to_owned());
        BrokerEndpoint::parse(value)
    }

    /// Validate a broker endpoint and normalize it to a trailing slash.
    pub fn parse(value: impl AsRef<str>) -> Result<BrokerEndpoint, EndpointError> {
        let mut url = Url::parse(value.as_ref()).map_err(EndpointError::InvalidUrl)?;
        if !url.path().ends_with('/') {
            let mut path = url.path().to_owned();
            path.push('/');
            url.set_path(&path);
        }
        Ok(BrokerEndpoint { url })
    }

    /// Borrow the normalized endpoint URL.
    pub const fn url(&self) -> &Url {
        &self.url
    }

    /// Resolve a relative endpoint path against the NGSI-LD v1 endpoint.
    pub fn join(&self, path: &str) -> Result<Url, EndpointError> {
        self.url.join(path).map_err(EndpointError::InvalidUrl)
    }

    /// Resolve a path rooted at the broker origin rather than under NGSI-LD v1.
    pub fn origin_join(&self, path: &str) -> Result<Url, EndpointError> {
        self.url.join(path).map_err(EndpointError::InvalidUrl)
    }
}

#[cfg(test)]
mod tests {
    use crate::broker_endpoint::BrokerEndpoint;

    #[test]
    fn endpoint_normalizes_trailing_slash_and_joins_relative_paths() {
        let endpoint = BrokerEndpoint::parse("http://127.0.0.1:1056/ngsi-ld/v1").unwrap_or_else(|error| unreachable!("static URL is valid: {error}"));
        let joined = endpoint.join("entities").unwrap_or_else(|error| unreachable!("static path is valid: {error}"));

        assert_eq!(joined.as_str(), "http://127.0.0.1:1056/ngsi-ld/v1/entities");
    }
}
