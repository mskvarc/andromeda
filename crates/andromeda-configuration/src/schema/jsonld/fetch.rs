//! The `[jsonld.fetch]` table: whether and how a remote `@context` is
//! downloaded.

use crate::{
    address_rule::AddressRule,
    human_byte_size::HumanByteSize,
    human_duration::HumanDuration,
    schema::{feature_mode::FeatureMode, private_addresses::PrivateAddresses},
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Whether and how a remote `@context` is downloaded.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
#[schemars(description = "Whether and how a remote `@context` is downloaded.")]
pub struct JsonLdFetchConfig {
    /// Whether a context that is neither embedded, preloaded nor cached may
    /// be downloaded: `enabled` or `disabled`.
    #[schemars(
        description = "Whether a context that is neither embedded, preloaded, stored nor cached may be downloaded. With `\"disabled\"` nothing is downloaded: only the core `@context`, preloaded documents, stored `@context`s and what is already cached are used."
    )]
    pub mode: FeatureMode,
    /// How long one download may take, in human-readable form (`"30s"`); at
    /// least one second.
    #[schemars(description = "How long one download may take.")]
    pub timeout: HumanDuration,
    /// How many redirects one download may follow.
    #[schemars(
        description = "How many redirects one download may follow. Every redirect target is held to `jsonld.fetch.private_addresses`, `jsonld.fetch.allowed_addresses` and `jsonld.fetch.denied_addresses` as the first URL is."
    )]
    pub max_redirects: u32,
    /// The size beyond which a downloaded context is refused, in
    /// human-readable form (`"2 MiB"`).
    #[schemars(description = "The largest `@context` document the broker downloads. A larger one is refused with `504` `LdContextNotAvailable`.")]
    pub max_size: HumanByteSize,
    /// Whether a `@context` URL, or a redirect or JSON-LD alternate it leads
    /// to, resolving into private or reserved address space is downloaded.
    /// Checked against the address actually resolved.
    #[schemars(
        description = "Whether a `@context` URL resolving into private or reserved address space is downloaded, and so every redirect and JSON-LD alternate the download follows. Checked against the address actually resolved, not the host name. `jsonld.fetch.allowed_addresses` exempts addresses from `\"refused\"`, and `jsonld.fetch.denied_addresses` refuses addresses whatever this is set to. A refused download is answered `504` `LdContextNotAvailable`. Set `\"refused\"` when clients you do not trust can name a `@context`."
    )]
    pub private_addresses: PrivateAddresses,
    /// Addresses and subnets a `@context` download may resolve into even when
    /// `private_addresses` is `refused`.
    #[schemars(
        description = "IP addresses and subnets (`\"10.2.0.0/24\"`, `\"10.3.0.17\"`, `\"fd00:1234::/48\"`) that a `@context` download may resolve into even though `jsonld.fetch.private_addresses` is `\"refused\"`, such as a server in the broker's own network that hosts the contexts its clients use. Empty by default. An entry in `jsonld.fetch.denied_addresses` takes precedence."
    )]
    pub allowed_addresses: Vec<AddressRule>,
    /// Addresses and subnets a `@context` is never downloaded from.
    #[schemars(
        description = "IP addresses and subnets (`\"203.0.113.0/24\"`, `\"198.51.100.9\"`) that a `@context` download must not resolve into, public ones included and whatever `jsonld.fetch.private_addresses` is set to. Empty by default. Takes precedence over `jsonld.fetch.allowed_addresses`."
    )]
    pub denied_addresses: Vec<AddressRule>,
}

impl Default for JsonLdFetchConfig {
    fn default() -> JsonLdFetchConfig {
        JsonLdFetchConfig {
            mode: FeatureMode::Enabled,
            timeout: HumanDuration::from_secs(30),
            max_redirects: 5,
            max_size: HumanByteSize::mib(2),
            private_addresses: PrivateAddresses::Allowed,
            allowed_addresses: Vec::new(),
            denied_addresses: Vec::new(),
        }
    }
}

impl JsonLdFetchConfig {
    /// Whether a remote context may be downloaded.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self.mode, FeatureMode::Enabled)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        address_rule::AddressRule,
        human_byte_size::HumanByteSize,
        schema::{jsonld::fetch::JsonLdFetchConfig, private_addresses::PrivateAddresses},
    };
    use pretty_assertions::assert_eq;
    use std::error::Error;

    #[test]
    fn the_default_downloads_up_to_two_mebibytes_within_thirty_seconds() {
        let config = JsonLdFetchConfig::default();
        assert!(config.is_enabled());
        assert_eq!(config.max_size, HumanByteSize::mib(2));
    }

    #[test]
    fn the_default_downloads_from_any_address() {
        let config = JsonLdFetchConfig::default();
        assert_eq!(config.private_addresses, PrivateAddresses::Allowed);
        assert_eq!(config.allowed_addresses, Vec::<AddressRule>::new());
        assert_eq!(config.denied_addresses, Vec::<AddressRule>::new());
    }

    #[test]
    fn the_address_policy_reads_from_the_table() -> Result<(), Box<dyn Error>> {
        let config: JsonLdFetchConfig = toml::from_str(
            "mode = \"enabled\"\ntimeout = \"30s\"\nmax_redirects = 5\nmax_size = \"2 MiB\"\nprivate_addresses = \"refused\"\nallowed_addresses = [\"10.2.0.0/24\"]\ndenied_addresses = [\"10.2.0.9\"]\n",
        )?;
        assert_eq!(config.private_addresses, PrivateAddresses::Refused);
        assert_eq!(config.allowed_addresses, vec!["10.2.0.0/24".parse::<AddressRule>()?]);
        assert_eq!(config.denied_addresses, vec!["10.2.0.9".parse::<AddressRule>()?]);
        Ok(())
    }
}
