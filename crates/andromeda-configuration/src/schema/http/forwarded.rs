//! The `[http.forwarded]` table: whose forwarded fields the broker believes.
//!
//! `Forwarded` (IETF RFC 7239), `X-Forwarded-Proto` and `X-Forwarded-Host`
//! are ordinary request fields: anything that can reach the broker's listener
//! can set them, the client included. Believing them lets the sender choose
//! the scheme and host of every URL the broker mints from the request origin,
//! including the `@context` URL clause 5.13 has the broker store and serve,
//! which outlives the request that created it. So nothing is believed until
//! an operator says whose fields to believe.

use crate::{address_rule::AddressRule, comma_list::array_or_comma_list};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whose forwarded fields are believed, and the proxies a `proxies` trust
/// names.
#[derive(Deserialize, Serialize, JsonSchema, Debug, Clone, Default, PartialEq, Eq)]
#[schemars(deny_unknown_fields)]
#[schemars(
    description = "Whether the `Forwarded`, `X-Forwarded-Proto` and `X-Forwarded-Host` request fields decide the scheme and host of the URLs the broker builds, such as a pagination `Link` and the stored URL of an `@context`."
)]
pub struct ForwardedConfig {
    /// Whose forwarded fields are believed: `none`, `any` or `proxies`.
    #[schemars(description = "Whose forwarded fields the broker believes.")]
    pub trust: ForwardedTrust,
    /// The proxies whose forwarded fields are believed under `proxies`, each
    /// matched against the address of the peer a request arrived from.
    /// Required and non-empty under `proxies`, refused under any other trust.
    #[serde(deserialize_with = "array_or_comma_list")]
    #[schemars(
        description = "The IP addresses and subnets (`\"10.0.0.0/8\"`, `\"192.0.2.7\"`, `\"fd00::/8\"`) of the proxies whose forwarded fields are believed, matched against the address a request arrives from. An IPv4 peer reaching a dual-stack listener is matched as its IPv4 address."
    )]
    pub proxies: Vec<AddressRule>,
}

/// Whose forwarded fields are believed.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whose forwarded fields the broker believes.")]
pub enum ForwardedTrust {
    /// Believe no one: the URLs the broker mints carry the scheme of its own
    /// listener and the authority the request addressed.
    #[default]
    #[schemars(description = "Ignores the fields. The URLs the broker builds carry the scheme its own listener speaks and the host the request addressed.")]
    None,
    /// Believe every peer, because the network lets only proxies the operator
    /// controls reach the broker.
    #[schemars(
        description = "Believes the fields from every peer. Set it only when nothing but a reverse proxy that overwrites the fields can reach the broker, or any client chooses the scheme and host of the URLs the broker builds, including the stored URL of an `@context` it serves to later readers."
    )]
    Any,
    /// Believe only the peers `proxies` lists.
    #[schemars(
        description = "Believes the fields only from a peer `http.forwarded.proxies` lists, and ignores them from every other peer. Each listed proxy must overwrite the fields rather than append to them."
    )]
    Proxies,
}

#[cfg(test)]
mod tests {
    use crate::{
        address_rule::AddressRule,
        schema::http::forwarded::{ForwardedConfig, ForwardedTrust},
    };
    use pretty_assertions::assert_eq;
    use std::error::Error;

    /// Nothing may be believed until an operator says so.
    #[test]
    fn no_forwarded_field_is_believed_by_default() {
        assert_eq!(ForwardedConfig::default().trust, ForwardedTrust::None);
        assert_eq!(ForwardedConfig::default().proxies, Vec::<AddressRule>::new());
    }

    #[test]
    fn the_trust_tokens_are_kebab_case_and_case_insensitive() -> Result<(), Box<dyn Error>> {
        for (trust, token) in [
            (ForwardedTrust::None, "none"),
            (ForwardedTrust::Any, "any"),
            (ForwardedTrust::Proxies, "proxies"),
        ] {
            assert_eq!(trust.to_string(), token);
            assert_eq!(token.to_uppercase().parse::<ForwardedTrust>()?, trust);
            assert_eq!(serde_json::to_string(&trust)?, format!("\"{token}\""));
        }
        Ok(())
    }

    #[test]
    fn proxies_read_from_an_array_and_from_a_comma_separated_string() -> Result<(), Box<dyn Error>> {
        let array: ForwardedConfig = toml::from_str("trust = \"proxies\"\nproxies = [\"10.0.0.0/8\", \"fd00::/8\", \"192.0.2.7\"]")?;
        let commas: ForwardedConfig = toml::from_str("trust = \"proxies\"\nproxies = \"10.0.0.0/8, fd00::/8,192.0.2.7\"")?;
        assert_eq!(array, commas);
        assert_eq!(array.proxies, ["10.0.0.0/8".parse()?, "fd00::/8".parse()?, "192.0.2.7".parse()?]);
        Ok(())
    }

    #[test]
    fn a_proxy_that_is_no_address_or_subnet_is_refused() {
        assert!(toml::from_str::<ForwardedConfig>("trust = \"proxies\"\nproxies = [\"proxy.example\"]").is_err());
    }

    #[test]
    fn the_configuration_round_trips_through_toml() -> Result<(), Box<dyn Error>> {
        let config = ForwardedConfig {
            trust: ForwardedTrust::Proxies,
            proxies: vec!["10.0.0.0/8".parse()?],
        };
        assert_eq!(toml::from_str::<ForwardedConfig>(&toml::to_string(&config)?)?, config);
        Ok(())
    }
}
