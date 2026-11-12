//! The identity this broker reports as a Context Source and carries in the
//! `Via` chain of what it forwards.
//!
//! `federation.identity` may leave the alias and the identifier out. The alias
//! then comes from the host clients and peers reach the broker at, the host of
//! `server.public_url`, and without one from the machine's host name, so two
//! brokers left at their defaults do not take each other's registrations for
//! themselves (ETSI GS CIM 009 v1.9.1 clause 4.3.6.4). Wherever it comes from,
//! the alias is an HTTP token (Table 5.2.40-1). The identifier follows the
//! alias in effect. The `contextSourceExtras` are read beside them.

use andromeda_configuration::schema::federation::identity::IdentityConfig;
use andromeda_json_literal::{JsonLiteral, configured_extras::ConfiguredExtrasError};
use andromeda_ngsi_ld_lexicon::{
    context_source_alias::{ContextSourceAlias, ContextSourceAliasError, DerivedIdentifierError},
    identifier::{Identifier, IdentifierParseError},
};
use derive_more::Display;
use thiserror::Error;
use tracing::info;
use url::{Host, Url};

/// Where the alias in effect comes from.
#[derive(Clone, Copy, Debug, Display, Eq, PartialEq)]
pub(crate) enum AliasOrigin {
    /// `federation.identity.source_alias` sets it.
    #[display("federation.identity.source_alias")]
    Configured,
    /// The host of `server.public_url`.
    #[display("the host of server.public_url")]
    PublicUrl,
    /// The machine's host name.
    #[display("the machine's host name")]
    HostName,
}

/// The identity is not one the broker can report.
#[derive(Debug, Error)]
pub(crate) enum SourceIdentityError {
    /// No alias is configured and nothing names the broker.
    #[error(
        "federation.identity.source_alias is not set, and neither server.public_url nor the machine's host name gives one; set federation.identity.source_alias"
    )]
    NoAlias,
    /// The alias in effect is not an HTTP token.
    #[error("the alias taken from {origin} is not an HTTP token: {source}; set federation.identity.source_alias")]
    Alias {
        /// Where the alias was taken from.
        origin: AliasOrigin,
        /// Why it is no token.
        source: ContextSourceAliasError,
    },
    /// The configured identifier is not one a stored resource can carry.
    #[error("federation.identity.source_id: {0}")]
    Identifier(#[from] IdentifierParseError),
    /// No identifier is configured, and the alias in effect derives none.
    #[error("federation.identity.source_id is not set, and the alias in effect derives no identifier: {0}; set federation.identity.source_id")]
    DerivedIdentifier(#[from] DerivedIdentifierError),
    /// `federation.identity.extras` holds a value JSON cannot spell, or is no
    /// admissible JSON.
    #[error(transparent)]
    Extras(#[from] ConfiguredExtrasError),
}

/// The alias, the identifier and the extras the broker reports.
#[derive(Clone, Debug)]
pub(crate) struct SourceIdentity {
    /// The `id` reported at `/info/sourceIdentity`.
    pub(crate) id: Identifier,
    /// The default tenant's alias, from which every other tenant's follows.
    pub(crate) alias: ContextSourceAlias,
    /// Where `alias` comes from.
    pub(crate) alias_origin: AliasOrigin,
    /// The `contextSourceExtras` the operator configures, if any.
    pub(crate) extras: Option<JsonLiteral>,
}

impl SourceIdentity {
    /// The identity `identity` configures, its left-out parts taken from
    /// `public_url` and, failing that, from `host_name`, which is read only
    /// when needed.
    ///
    /// # Errors
    ///
    /// Returns [`SourceIdentityError`] when no alias can be found, when the
    /// alias in effect is no HTTP token, when the configured identifier is no
    /// identifier or the alias derives none, or when the extras are no
    /// admissible JSON.
    pub(crate) fn resolve(
        identity: &IdentityConfig,
        public_url: Option<&Url>,
        host_name: impl FnOnce() -> Option<String>,
    ) -> Result<SourceIdentity, SourceIdentityError> {
        let (alias, alias_origin) = match &identity.source_alias {
            Some(configured) => (ContextSourceAlias::try_from(configured), AliasOrigin::Configured),
            None => match public_url.and_then(url_host) {
                Some(host) => (ContextSourceAlias::new(host), AliasOrigin::PublicUrl),
                None => (ContextSourceAlias::new(host_name().ok_or(SourceIdentityError::NoAlias)?), AliasOrigin::HostName),
            },
        };
        let alias = alias.map_err(|source| SourceIdentityError::Alias { origin: alias_origin, source })?;
        let id = match &identity.source_id {
            Some(configured) => Identifier::try_from(configured)?,
            None => alias.derived_identifier()?,
        };
        let extras = identity.extras.as_ref().map(JsonLiteral::try_from).transpose()?;
        Ok(SourceIdentity {
            id,
            alias,
            alias_origin,
            extras,
        })
    }

    /// Log the identity in effect, and where its alias comes from.
    pub(crate) fn report(&self) {
        info!(
            event = "source_identity",
            source_id = %self.id,
            source_alias = %self.alias,
            source_alias_origin = %self.alias_origin,
            "Reporting this broker as a context source"
        );
    }
}

/// The host of `url` as an alias can carry it: a domain or an IPv4 address,
/// without the port. An IPv6 literal is written in brackets with
/// colons, which no token holds, so it names nothing here.
fn url_host(url: &Url) -> Option<String> {
    match url.host()? {
        Host::Domain(domain) => Some(domain.to_owned()),
        Host::Ipv4(address) => Some(address.to_string()),
        Host::Ipv6(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use crate::configuration::source_identity::{AliasOrigin, SourceIdentity, SourceIdentityError};
    use andromeda_configuration::schema::{federation::identity::IdentityConfig, source_alias::SourceAlias, source_identifier::SourceIdentifier};
    use andromeda_ngsi_ld_lexicon::context_source_alias::ContextSourceAliasError;
    use iri_rs::iri;
    use pretty_assertions::assert_eq;
    use url_macro::url;

    fn no_host_name() -> Option<String> {
        None
    }

    /// An identity whose only configured part is `source_alias = alias`.
    fn configured_alias(alias: &'static str) -> IdentityConfig {
        IdentityConfig {
            source_alias: Some(SourceAlias::from_static(alias)),
            ..IdentityConfig::default()
        }
    }

    /// Table 5.2.40-1 makes the alias an RFC 7230 pseudonym, a token, so a
    /// `received-by` host and port, a path, a user part or a quoted string is
    /// refused at startup, naming the key and the first character outside
    /// `tchar`.
    #[test]
    fn a_configured_alias_outside_the_token_characters_is_refused_naming_its_first_offending_character() {
        for (alias, offending) in [("example.com:8080", ':'), ("broker/a", '/'), ("broker@a", '@'), ("\"broker\"", '"')] {
            let refused = resolved(&configured_alias(alias), None, Some("machine"));
            assert!(
                matches!(
                    refused,
                    Err(SourceIdentityError::Alias {
                        origin: AliasOrigin::Configured,
                        source: ContextSourceAliasError::NotAToken { offending: found },
                    }) if found == offending
                ),
                "{alias:?}: {refused:?}"
            );
        }
    }

    #[test]
    fn an_empty_configured_alias_is_refused() {
        let refused = resolved(&configured_alias(""), None, Some("machine"));
        assert!(
            matches!(
                refused,
                Err(SourceIdentityError::Alias {
                    origin: AliasOrigin::Configured,
                    source: ContextSourceAliasError::Empty,
                })
            ),
            "{refused:?}"
        );
    }

    /// The identifier derived from an alias keeps letters, digits and
    /// `-._!$'*+` as written and percent-encodes every other token character.
    #[test]
    fn a_configured_alias_of_token_characters_only_is_used_as_written() {
        for (alias, id) in [
            ("broker-a", "urn:ngsi-ld:ContextSourceIdentity:broker-a"),
            ("broker.example.org", "urn:ngsi-ld:ContextSourceIdentity:broker.example.org"),
            ("a_b~c!#$%&'*+^`|", "urn:ngsi-ld:ContextSourceIdentity:a_b%7Ec!%23$%25%26'*+%5E%60%7C"),
        ] {
            assert_eq!(
                resolved(&configured_alias(alias), None, Some("machine")).ok(),
                Some((alias.to_owned(), id.to_owned(), AliasOrigin::Configured))
            );
        }
    }

    /// The URL standard admits `,` in a domain, and no token holds it.
    #[test]
    fn a_public_url_host_that_is_no_token_is_refused_naming_where_it_came_from() {
        let public_url = url::Url::parse("http://broker,a.example/").expect("a URL");
        let refused = resolved(&IdentityConfig::default(), Some(&public_url), Some("node-7"));
        assert!(
            matches!(
                refused,
                Err(SourceIdentityError::Alias {
                    origin: AliasOrigin::PublicUrl,
                    source: ContextSourceAliasError::NotAToken { offending: ',' },
                })
            ),
            "{refused:?}"
        );
    }

    fn resolved(
        identity: &IdentityConfig,
        public_url: Option<&url::Url>,
        host_name: Option<&str>,
    ) -> Result<(String, String, AliasOrigin), SourceIdentityError> {
        SourceIdentity::resolve(identity, public_url, || host_name.map(str::to_owned))
            .map(|identity| (identity.alias.to_string(), identity.id.as_str().to_owned(), identity.alias_origin))
    }

    #[test]
    fn a_configured_alias_and_identifier_are_used_as_written() {
        let identity = IdentityConfig {
            source_id: Some(SourceIdentifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:hub"))),
            source_alias: Some(SourceAlias::from_static("broker-a")),
            extras: None,
        };
        assert_eq!(
            resolved(&identity, Some(&url!("https://broker.example.org/")), Some("machine")).ok(),
            Some((
                "broker-a".to_owned(),
                "urn:ngsi-ld:ContextSourceIdentity:hub".to_owned(),
                AliasOrigin::Configured
            ))
        );
    }

    #[test]
    fn a_left_out_alias_is_the_public_url_host_without_its_port_and_names_the_identifier() {
        assert_eq!(
            resolved(
                &IdentityConfig::default(),
                Some(&url!("https://broker.example.org:8443/context-broker")),
                Some("machine")
            )
            .ok(),
            Some((
                "broker.example.org".to_owned(),
                "urn:ngsi-ld:ContextSourceIdentity:broker.example.org".to_owned(),
                AliasOrigin::PublicUrl
            ))
        );
    }

    #[test]
    fn an_ipv4_public_url_names_the_alias() {
        assert_eq!(
            resolved(&IdentityConfig::default(), Some(&url!("http://192.0.2.7:1056/")), None).ok(),
            Some((
                "192.0.2.7".to_owned(),
                "urn:ngsi-ld:ContextSourceIdentity:192.0.2.7".to_owned(),
                AliasOrigin::PublicUrl
            ))
        );
    }

    #[test]
    fn without_a_usable_public_url_the_alias_is_the_machine_host_name() {
        for public_url in [None, Some(url!("http://[2001:db8::7]:1056/"))] {
            assert_eq!(
                resolved(&IdentityConfig::default(), public_url.as_ref(), Some("node-7")).ok(),
                Some((
                    "node-7".to_owned(),
                    "urn:ngsi-ld:ContextSourceIdentity:node-7".to_owned(),
                    AliasOrigin::HostName
                ))
            );
        }
    }

    #[test]
    fn a_configured_identifier_is_kept_beside_a_derived_alias() {
        let identity = IdentityConfig {
            source_id: Some(SourceIdentifier::from_static(iri!("urn:ngsi-ld:ContextSourceIdentity:hub"))),
            ..IdentityConfig::default()
        };
        assert_eq!(
            resolved(&identity, None, Some("node-7")).ok(),
            Some(("node-7".to_owned(), "urn:ngsi-ld:ContextSourceIdentity:hub".to_owned(), AliasOrigin::HostName))
        );
    }

    #[test]
    fn nothing_to_take_an_alias_from_is_refused() {
        let refused = SourceIdentity::resolve(&IdentityConfig::default(), None, no_host_name);
        assert!(matches!(refused, Err(SourceIdentityError::NoAlias)), "{refused:?}");
    }

    #[test]
    fn a_host_name_that_is_no_token_is_refused_naming_where_it_came_from() {
        for (host_name, offending) in [("my machine", ' '), ("node@7", '@')] {
            let refused = resolved(&IdentityConfig::default(), None, Some(host_name));
            assert!(
                matches!(
                    refused,
                    Err(SourceIdentityError::Alias {
                        origin: AliasOrigin::HostName,
                        source: ContextSourceAliasError::NotAToken { offending: found },
                    }) if found == offending
                ),
                "{host_name:?}: {refused:?}"
            );
        }
    }
}
