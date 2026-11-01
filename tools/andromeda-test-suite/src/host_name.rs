//! A host the suite configuration names, as the environment spells it.

use serde::Deserialize;
use url::Host;

/// A host name or address, written as a URL host is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(try_from = "String")]
pub struct HostName(Host<String>);

// `url::Host` deserializes from its enum form; the environment holds the host as text.
impl TryFrom<String> for HostName {
    type Error = url::ParseError;

    fn try_from(text: String) -> Result<HostName, url::ParseError> {
        Host::parse(&text).map(HostName)
    }
}

impl HostName {
    /// The host.
    #[must_use]
    pub const fn host(&self) -> &Host<String> {
        &self.0
    }
}
