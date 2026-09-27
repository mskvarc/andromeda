//! The scheme of the `apiRoot` the broker mints its own URLs under.

use strum::{Display, EnumString, IntoStaticStr};

/// The scheme of an `apiRoot`.
///
/// ETSI GS CIM 009 v1.9.1 clause 6.2 admits exactly two: "the `apiRoot`
/// includes the scheme (`http` or `https`)". Anything else names something
/// this broker cannot mint an `apiRoot` from, so it is a parse failure rather
/// than a third variant.
#[derive(Clone, Copy, Debug, Display, EnumString, Eq, Hash, IntoStaticStr, PartialEq)]
#[strum(serialize_all = "lowercase", ascii_case_insensitive)]
pub enum ApiRootScheme {
    /// Plaintext HTTP.
    Http,
    /// HTTP over TLS, as clause 6.2 requires the deployment to offer.
    Https,
}

impl ApiRootScheme {
    /// The URL scheme this spells.
    #[must_use]
    pub fn as_scheme(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::api_root_scheme::ApiRootScheme;

    #[test]
    fn the_two_schemes_clause_6_2_admits_round_trip_through_their_tokens() {
        pretty_assertions::assert_eq!(ApiRootScheme::Http.as_scheme(), "http");
        pretty_assertions::assert_eq!(ApiRootScheme::Https.as_scheme(), "https");
        pretty_assertions::assert_eq!(ApiRootScheme::Https.to_string(), "https");
        pretty_assertions::assert_eq!("HTTPS".parse::<ApiRootScheme>(), Ok(ApiRootScheme::Https));
    }
}
