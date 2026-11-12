//! The `contextSourceAlias` a Context Source answers to (ETSI GS CIM 009
//! v1.9.1 Table 5.2.40-1), and the hop it writes into a `Via` chain.

use crate::{
    context_source_pseudonym::ContextSourcePseudonym,
    http_token::is_tchar,
    identifier::Identifier,
    identifier_length::IdentifierTooLong,
    tenant::TenantName,
    uri::NGSI_LD_URN_NID,
};
use andromeda_configuration::schema::source_alias::SourceAlias;
use derive_more::Display;
use serde::Serialize;
use thiserror::Error;
use urn_rs::{UrnBuilder, percent::encode_nss};

/// The kind the identifier an alias derives names, the first segment of its
/// namespace-specific string.
const IDENTITY_KIND: &str = "ContextSourceIdentity";

/// Separator between a context source's own alias and the tenant it names.
///
/// RFC 9110 section 5.6.2 lists `~` among the `tchar` set, so a composed alias
/// is still a token, and [`TenantName`]'s grammar (`[A-Za-z_][A-Za-z0-9_-]*`)
/// never contains it, so the tenant half of a composed alias is unambiguous.
const TENANT_SEPARATOR: char = '~';

/// The alias this broker answers to as a Context Source: the
/// `contextSourceAlias` of Table 5.2.40-1 and the pseudonym it appends to the
/// `Via` chain of every request it forwards (clause 6.3.18).
///
/// Table 5.2.40-1 restricts the alias to a "Pseudonym field as defined in IETF
/// RFC 7230". RFC 7230 section 5.7.1 makes a `pseudonym` a `token`, and section
/// 3.2.6 makes a token one or more `tchar`: letters, digits and
/// ``!#$%&'*+-.^_`|~``. Another broker's `Via` element may name its hop by
/// `uri-host [ ":" port ]` instead, which [`ContextSourcePseudonym`] reads; the
/// hop this broker writes is always the token.
#[derive(Debug, Clone, PartialEq, Eq, Display, Serialize)]
#[display("{_0}")]
#[serde(transparent)]
pub struct ContextSourceAlias(String);

/// Why a string cannot be a [`ContextSourceAlias`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextSourceAliasError {
    /// The alias is empty.
    #[error("the alias is empty")]
    Empty,
    /// The alias holds a character outside `tchar`.
    #[error("the alias holds '{}', and an HTTP token holds only letters, digits and !#$%&'*+-.^_`|~", .offending.escape_default())]
    NotAToken {
        /// The first character that is no `tchar`.
        offending: char,
    },
}

/// Why an alias derives no identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DerivedIdentifierError {
    /// The alias spells no URN namespace-specific string.
    #[error("the alias spells no URN: {0}")]
    Urn(#[from] urn_rs::Error),
    /// The URN is longer than an identifier may be.
    #[error(transparent)]
    TooLong(#[from] IdentifierTooLong),
}

impl ContextSourceAlias {
    /// Validate `alias` as an HTTP token.
    ///
    /// # Errors
    ///
    /// Returns [`ContextSourceAliasError::Empty`] for an empty value and
    /// [`ContextSourceAliasError::NotAToken`] naming the first character that
    /// is no `tchar`.
    pub fn new(alias: impl Into<String>) -> Result<ContextSourceAlias, ContextSourceAliasError> {
        let alias = alias.into();
        if alias.is_empty() {
            return Err(ContextSourceAliasError::Empty);
        }
        if let Some(offending) = alias.chars().find(|character| !is_tchar(*character)) {
            return Err(ContextSourceAliasError::NotAToken { offending });
        }
        Ok(ContextSourceAlias(alias))
    }

    /// The alias as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The identifier of the context source this alias names when none is
    /// configured: `urn:ngsi-ld:ContextSourceIdentity:` and the alias
    /// percent-encoded as a namespace-specific string, which keeps letters,
    /// digits and `-._!$'*+` as written.
    ///
    /// # Errors
    ///
    /// Returns [`DerivedIdentifierError::TooLong`] when the URN is longer than
    /// an identifier may be. [`DerivedIdentifierError::Urn`] is unreachable: a
    /// token is never empty and encodes into a valid namespace-specific string.
    pub fn derived_identifier(&self) -> Result<Identifier, DerivedIdentifierError> {
        let nss = format!("{IDENTITY_KIND}:{}", encode_nss(&self.0)?);
        Ok(Identifier::try_from(UrnBuilder::new(NGSI_LD_URN_NID, &nss).build()?)?)
    }

    /// The alias naming `tenant` within the context source this alias names.
    ///
    /// Table 5.2.40-1 requires that "in the multi-tenancy use case (see clause
    /// 4.14), this id shall be identifying a specific Tenant within a
    /// registered Context Source", and clause 4.3.6.4 requires that "each
    /// Tenant found within each registered Context Source shall be considered
    /// separately" when a distributed operation excludes the sources it has
    /// already traversed. One alias shared by every tenant satisfies neither:
    /// a request that has passed through one tenant would look like a loop at
    /// every other tenant of the same broker.
    ///
    /// The composition is total: this alias, the separator and a
    /// [`TenantName`] (`[A-Za-z_][A-Za-z0-9_-]*`) are all made of `tchar`, so
    /// the result is a token too.
    #[must_use]
    pub fn scoped_to_tenant(&self, tenant: &TenantName) -> ContextSourceAlias {
        ContextSourceAlias(format!("{}{TENANT_SEPARATOR}{}", self.0, tenant.as_str()))
    }

    /// Whether `pseudonym` names the context source this alias names: it is
    /// this alias, or this alias scoped to one of its tenants as
    /// [`ContextSourceAlias::scoped_to_tenant`] composes it.
    #[must_use]
    pub fn is_named_by(&self, pseudonym: &ContextSourcePseudonym) -> bool {
        let pseudonym = pseudonym.as_str();
        pseudonym == self.0
            || pseudonym
                .strip_prefix(self.0.as_str())
                .and_then(|rest| rest.strip_prefix(TENANT_SEPARATOR))
                .is_some_and(|tenant| tenant.parse::<TenantName>().is_ok())
    }
}

// The configured alias carries no grammar of its own; this conversion applies
// the alias's, so it is the one place the configured alias is held to it.
impl TryFrom<&SourceAlias> for ContextSourceAlias {
    type Error = ContextSourceAliasError;

    fn try_from(configured: &SourceAlias) -> Result<ContextSourceAlias, ContextSourceAliasError> {
        ContextSourceAlias::new(configured.as_str())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_source_alias::{ContextSourceAlias, ContextSourceAliasError},
        context_source_pseudonym::ContextSourcePseudonym,
        tenant::{TENANT_NAME_BYTE_LIMIT, TenantName},
    };
    use andromeda_configuration::schema::source_alias::SourceAlias;
    use pretty_assertions::assert_eq;

    fn alias(raw: &str) -> ContextSourceAlias {
        ContextSourceAlias::new(raw).expect("a valid alias")
    }

    fn pseudonym(raw: &str) -> ContextSourcePseudonym {
        ContextSourcePseudonym::new(raw).expect("a valid pseudonym")
    }

    /// A validated tenant label, for composing tenant-scoped aliases.
    fn tenant(raw: &str) -> TenantName {
        raw.parse::<TenantName>().expect("a valid tenant name")
    }

    #[test]
    fn a_token_is_an_alias_as_written() {
        for raw in ["broker-a", "broker.example.org", "a_b~c!#$%&'*+^`|", "192.0.2.7"] {
            assert_eq!(alias(raw).as_str(), raw);
            assert_eq!(alias(raw).to_string(), raw);
        }
    }

    #[test]
    fn an_empty_alias_is_refused() {
        assert_eq!(ContextSourceAlias::new(""), Err(ContextSourceAliasError::Empty));
    }

    /// RFC 7230 section 5.7.1: a `received-by` may be `uri-host [ ":" port ]`,
    /// but a pseudonym is a token, so a host and port, a URI and a quoted
    /// string are no alias.
    #[test]
    fn an_alias_outside_the_token_characters_is_refused_naming_its_first_offending_character() {
        for (raw, offending) in [
            ("example.com:8080", ':'),
            ("broker/a", '/'),
            ("broker@a", '@'),
            ("\"broker\"", '"'),
            ("broker a", ' '),
            ("broker\u{7}a", '\u{7}'),
            ("urn:ngsi-ld:ContextSourceIdentity:broker1", ':'),
            ("brökér", 'ö'),
        ] {
            assert_eq!(
                ContextSourceAlias::new(raw),
                Err(ContextSourceAliasError::NotAToken { offending }),
                "{raw:?} must name its first character outside tchar"
            );
        }
    }

    #[test]
    fn a_configured_alias_converts_only_when_it_is_a_token() {
        assert_eq!(ContextSourceAlias::try_from(&SourceAlias::from_static("broker-a")), Ok(alias("broker-a")));
        assert_eq!(ContextSourceAlias::try_from(&SourceAlias::from_static("")), Err(ContextSourceAliasError::Empty));
        assert_eq!(
            ContextSourceAlias::try_from(&SourceAlias::from_static("example.com:8080")),
            Err(ContextSourceAliasError::NotAToken { offending: ':' })
        );
    }

    #[test]
    fn the_derived_identifier_names_the_alias_under_the_context_source_identity_kind() {
        for raw in ["broker-a", "broker.example.org", "192.0.2.7"] {
            assert_eq!(
                alias(raw).derived_identifier().map(|identifier| identifier.as_str().to_owned()),
                Ok(format!("urn:ngsi-ld:ContextSourceIdentity:{raw}"))
            );
        }
    }

    /// The namespace-specific string keeps letters, digits and `-._!$'*+` as
    /// written and percent-encodes every other token character.
    #[test]
    fn the_derived_identifier_percent_encodes_the_other_token_characters() {
        assert_eq!(
            alias("a_b~c!#$%&'*+^`|").derived_identifier().map(|identifier| identifier.as_str().to_owned()),
            Ok("urn:ngsi-ld:ContextSourceIdentity:a_b%7Ec!%23$%25%26'*+%5E%60%7C".to_owned())
        );
    }

    #[test]
    fn the_alias_serialises_as_its_string() {
        assert_eq!(serde_json::to_string(&alias("broker-a")).expect("serialises"), "\"broker-a\"");
    }

    /// Table 5.2.40-1: the alias identifies a specific Tenant within a
    /// registered Context Source, so the tenant is part of the value.
    #[test]
    fn a_tenant_scoped_alias_names_both_the_context_source_and_the_tenant() {
        assert_eq!(alias("broker-a").scoped_to_tenant(&tenant("alpha")).as_str(), "broker-a~alpha");
    }

    /// Clause 4.3.6.4 considers each Tenant of a registered Context Source
    /// separately, which only holds if two tenants never share one alias.
    #[test]
    fn two_tenants_of_one_context_source_never_share_an_alias() {
        let broker = alias("broker-a");
        assert_ne!(broker.scoped_to_tenant(&tenant("alpha")), broker.scoped_to_tenant(&tenant("beta")));
        assert_ne!(broker.scoped_to_tenant(&tenant("alpha")), broker);
    }

    /// Two brokers keep distinct aliases for the same tenant name, so a hop
    /// through one is never mistaken for a hop through the other.
    #[test]
    fn two_context_sources_keep_distinct_aliases_for_the_same_tenant() {
        assert_ne!(
            alias("broker-a").scoped_to_tenant(&tenant("alpha")),
            alias("broker-b").scoped_to_tenant(&tenant("alpha"))
        );
    }

    /// Every tenant label the grammar admits composes to a token (leading
    /// underscore, hyphens, digits and the maximum length included).
    #[test]
    fn every_admissible_tenant_label_composes_to_a_token() {
        let broker = alias("broker.example.org");
        for label in ["_internal", "A_B", "tenant-a", "t9", &"a".repeat(TENANT_NAME_BYTE_LIMIT)] {
            let composed = broker.scoped_to_tenant(&tenant(label));
            assert_eq!(ContextSourceAlias::new(composed.as_str()), Ok(composed.clone()));
            assert!(composed.as_str().ends_with(label), "the tenant must be recoverable from {composed}");
        }
    }

    #[test]
    fn an_alias_is_named_by_itself_and_by_every_tenant_of_it() {
        let broker = alias("broker-a");
        assert!(broker.is_named_by(&pseudonym("broker-a")));
        assert!(broker.is_named_by(&ContextSourcePseudonym::from(&broker.scoped_to_tenant(&tenant("alpha")))));
    }

    /// A `received-by` that only shares a prefix with this alias, or names a
    /// host and port, names another context source.
    #[test]
    fn a_pseudonym_that_only_shares_a_prefix_or_names_a_host_does_not_name_the_alias() {
        let broker = alias("broker-a");
        for other in ["broker-ab", "broker-a~", "broker-a~9alpha", "broker", "broker-b~alpha", "broker-a:8080"] {
            assert!(!broker.is_named_by(&pseudonym(other)), "{other} names another source");
        }
        assert!(!broker.scoped_to_tenant(&tenant("alpha")).is_named_by(&pseudonym("broker-a")));
    }
}
