//! Whether the broker believes the forwarded-protocol fields it is sent.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use strum::{Display, EnumString};

/// Whether a forwarded-protocol field on an inbound request is believed.
///
/// `Forwarded` and `X-Forwarded-Proto` are
/// ordinary request fields: anything that can reach the broker's listener can
/// set them, including the client itself. Believing them unconditionally would
/// let a client choose the scheme of every URL the broker mints from the
/// request origin -- including the `@context` URL clause 5.13 has the broker
/// store and serve, which outlives the request that created it and is handed
/// to every later reader.
///
/// So the default is [`ForwardedHeaderTrust::Untrusted`], and an operator opts
/// in only once a reverse proxy in front of the broker sets these fields and
/// strips whatever the client sent.
#[derive(Clone, Copy, Debug, Default, Deserialize, Display, EnumString, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
#[strum(serialize_all = "kebab-case", ascii_case_insensitive)]
#[schemars(description = "Whether the `Forwarded` and `X-Forwarded-Proto` request fields decide the scheme of the URLs the broker builds from a request.")]
pub enum ForwardedHeaderTrust {
    /// Ignore every forwarded-protocol field. The scheme the broker's own
    /// listener speaks is the scheme of the URLs it mints.
    #[default]
    #[schemars(description = "Ignores both fields. The URLs the broker builds carry the scheme its own listener speaks.")]
    Untrusted,
    /// Believe the forwarded-protocol fields, because a reverse proxy the
    /// operator controls sets them and overwrites whatever the client sent.
    #[schemars(
        description = "Believes both fields. Set it only behind a reverse proxy that overwrites both, or any client chooses the scheme of the URLs the broker builds, including the stored URL of an `@context` it serves to later readers."
    )]
    Trusted,
}

#[cfg(test)]
mod tests {
    use crate::schema::forwarded_header_trust::ForwardedHeaderTrust;

    /// Nothing may be believed until an operator says so, so an unconfigured
    /// broker ignores the fields.
    #[test]
    fn forwarded_fields_are_untrusted_until_an_operator_says_otherwise() {
        pretty_assertions::assert_eq!(ForwardedHeaderTrust::default(), ForwardedHeaderTrust::Untrusted);
    }

    #[test]
    fn the_configuration_tokens_are_kebab_case_and_case_insensitive() {
        pretty_assertions::assert_eq!(ForwardedHeaderTrust::Trusted.to_string(), "trusted");
        pretty_assertions::assert_eq!(ForwardedHeaderTrust::Untrusted.to_string(), "untrusted");
        pretty_assertions::assert_eq!("TRUSTED".parse::<ForwardedHeaderTrust>(), Ok(ForwardedHeaderTrust::Trusted));
    }

    #[test]
    fn the_serde_tokens_match_the_configuration_tokens() -> Result<(), serde_json::Error> {
        pretty_assertions::assert_eq!(serde_json::to_string(&ForwardedHeaderTrust::Trusted)?, "\"trusted\"");
        pretty_assertions::assert_eq!(serde_json::from_str::<ForwardedHeaderTrust>("\"untrusted\"")?, ForwardedHeaderTrust::Untrusted);
        Ok(())
    }
}
