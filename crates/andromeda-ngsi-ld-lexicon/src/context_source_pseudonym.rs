//! The pseudonym naming one intermediary in a `Via` chain (RFC 9110 clause 7.6.3).

use crate::tenant::TenantName;
use derive_more::{Display, Into};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use thiserror::Error;

/// Separator between a context source's own alias and the tenant it names.
///
/// RFC 9110 clause 5.6.2 lists `~` among the `tchar` set, so a composed alias
/// remains a single `received-by` token that a `Via` element can carry, and
/// [`TenantName`]'s grammar (`[A-Za-z_][A-Za-z0-9_-]*`) never contains it, so
/// the tenant half of a composed alias is unambiguous.
const TENANT_SEPARATOR: char = '~';

/// The pseudonym of one hop in a `Via` header chain (RFC 9110 clause 7.6.3,
/// ETSI GS CIM 009 clause 6.3.18).
///
/// This is the `received-by` token a broker writes to name itself in the `Via`
/// chain and the value forwarding loop detection compares against
/// ([`Via::contains_pseudonym`](crate::via::Via::contains_pseudonym)). It is
/// deliberately permissive: an incoming `Via` from another broker may carry any
/// opaque token -- a bare label (`broker-a`), a host (`example.com:8080`), or a
/// URI (`urn:ngsi-ld:ContextSourceIdentity:broker1`) -- so the only structure
/// required is that a `Via` element can carry the value at all. RFC 9110 makes a
/// `received-by` a single whitespace-delimited token, so the value must be
/// non-empty and free of whitespace and control characters; being URI-shaped is
/// allowed but never required.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Display, Into, Serialize, Deserialize)]
#[display("{_0}")]
#[into(String)]
#[serde(try_from = "String", into = "String")]
pub struct ContextSourcePseudonym(String);

/// Why a string cannot be a [`ContextSourcePseudonym`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ContextSourcePseudonymError {
    /// The token was empty.
    #[error("via pseudonym must not be empty")]
    Empty,
    /// The token carried whitespace or a control character, so it could not be a
    /// single `received-by` token.
    #[error("via pseudonym contains the whitespace or control character {}", .offending.escape_unicode())]
    NotAToken {
        /// The first character that cannot appear in a `received-by` token.
        offending: char,
    },
}

impl ContextSourcePseudonym {
    /// Validate an opaque token as a `Via` pseudonym.
    ///
    /// # Errors
    ///
    /// Returns [`ContextSourcePseudonymError::Empty`] for an empty value and
    /// [`ContextSourcePseudonymError::NotAToken`] when the value carries
    /// whitespace or a control character.
    pub fn new(token: impl Into<String>) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        let token = token.into();
        if token.is_empty() {
            return Err(ContextSourcePseudonymError::Empty);
        }
        if let Some(offending) = token.chars().find(|character| character.is_whitespace() || character.is_control()) {
            return Err(ContextSourcePseudonymError::NotAToken { offending });
        }
        Ok(ContextSourcePseudonym(token))
    }

    /// Build a pseudonym from a caller-authored static token, without routing
    /// it back through validation.
    ///
    /// The token must already be a non-empty `received-by` token free of
    /// whitespace and control characters; the caller passing a fixed literal it
    /// wrote is the guarantor of that invariant. This exists for built-in
    /// configuration defaults, where the literal is compile-time fixed and
    /// covered by a test, so no fallible construction belongs on the path.
    #[must_use]
    pub fn from_static(token: &'static str) -> ContextSourcePseudonym {
        ContextSourcePseudonym(token.to_owned())
    }

    /// The pseudonym as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The pseudonym naming `tenant` within the context source this pseudonym
    /// names.
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
    /// The composition is total, so no fallible constructor is needed: this
    /// pseudonym carries no whitespace and no control character by its own
    /// invariant, [`TenantName`] is a non-empty `[A-Za-z_][A-Za-z0-9_-]*`
    /// label, and the separator is neither, so the result always satisfies the
    /// pseudonym invariant.
    #[must_use]
    pub fn scoped_to_tenant(&self, tenant: &TenantName) -> ContextSourcePseudonym {
        ContextSourcePseudonym(format!("{}{TENANT_SEPARATOR}{}", self.0, tenant.as_str()))
    }
}

impl TryFrom<String> for ContextSourcePseudonym {
    type Error = ContextSourcePseudonymError;

    fn try_from(value: String) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        ContextSourcePseudonym::new(value)
    }
}

impl TryFrom<&str> for ContextSourcePseudonym {
    type Error = ContextSourcePseudonymError;

    fn try_from(value: &str) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        ContextSourcePseudonym::new(value)
    }
}

impl FromStr for ContextSourcePseudonym {
    type Err = ContextSourcePseudonymError;

    fn from_str(value: &str) -> Result<ContextSourcePseudonym, ContextSourcePseudonymError> {
        ContextSourcePseudonym::new(value)
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        context_source_pseudonym::{ContextSourcePseudonym, ContextSourcePseudonymError},
        tenant::TenantName,
    };

    /// A validated tenant label, for composing tenant-scoped aliases.
    fn tenant(raw: &str) -> TenantName {
        raw.parse::<TenantName>().expect("a valid tenant name")
    }

    #[test]
    fn a_bare_label_is_accepted_and_round_trips() {
        let pseudonym = ContextSourcePseudonym::new("broker-a").expect("valid pseudonym");
        pretty_assertions::assert_eq!(pseudonym.as_str(), "broker-a");
        pretty_assertions::assert_eq!(pseudonym.to_string(), "broker-a");
    }

    #[test]
    fn a_uri_shaped_token_is_accepted_but_not_required() {
        let urn = ContextSourcePseudonym::new("urn:ngsi-ld:ContextSourceIdentity:broker1").expect("valid pseudonym");
        pretty_assertions::assert_eq!(urn.as_str(), "urn:ngsi-ld:ContextSourceIdentity:broker1");
        let host = ContextSourcePseudonym::new("example.com:8080").expect("valid pseudonym");
        pretty_assertions::assert_eq!(host.as_str(), "example.com:8080");
    }

    #[test]
    fn an_empty_token_is_rejected() {
        pretty_assertions::assert_eq!(ContextSourcePseudonym::new(""), Err(ContextSourcePseudonymError::Empty));
    }

    #[test]
    fn a_token_with_whitespace_or_a_control_character_is_rejected() {
        for (token, offending) in [("broker a", ' '), ("broker\ta", '\t'), ("broker\u{0007}a", '\u{0007}')] {
            pretty_assertions::assert_eq!(
                ContextSourcePseudonym::new(token),
                Err(ContextSourcePseudonymError::NotAToken { offending }),
                "{token:?} must name its first non-token character"
            );
        }
    }

    #[test]
    fn parsing_and_conversion_agree_with_the_validating_constructor() {
        let parsed: ContextSourcePseudonym = "broker-b".parse().expect("valid pseudonym");
        pretty_assertions::assert_eq!(parsed, ContextSourcePseudonym::try_from("broker-b").expect("valid pseudonym"));
        assert!("".parse::<ContextSourcePseudonym>().is_err());
    }

    /// Table 5.2.40-1: the alias identifies a specific Tenant within a
    /// registered Context Source, so the tenant is part of the value.
    #[test]
    fn a_tenant_scoped_alias_names_both_the_context_source_and_the_tenant() {
        let broker = ContextSourcePseudonym::new("broker-a").expect("valid pseudonym");
        pretty_assertions::assert_eq!(broker.scoped_to_tenant(&tenant("alpha")).as_str(), "broker-a~alpha");
    }

    /// Clause 4.3.6.4 considers each Tenant of a registered Context Source
    /// separately, which only holds if two tenants never share one alias.
    #[test]
    fn two_tenants_of_one_context_source_never_share_an_alias() {
        let broker = ContextSourcePseudonym::new("broker-a").expect("valid pseudonym");
        assert_ne!(broker.scoped_to_tenant(&tenant("alpha")), broker.scoped_to_tenant(&tenant("beta")));
        assert_ne!(broker.scoped_to_tenant(&tenant("alpha")), broker);
    }

    /// Two brokers keep distinct aliases for the same tenant name, so a hop
    /// through one is never mistaken for a hop through the other.
    #[test]
    fn two_context_sources_keep_distinct_aliases_for_the_same_tenant() {
        let first = ContextSourcePseudonym::new("broker-a").expect("valid pseudonym");
        let second = ContextSourcePseudonym::new("broker-b").expect("valid pseudonym");
        assert_ne!(first.scoped_to_tenant(&tenant("alpha")), second.scoped_to_tenant(&tenant("alpha")));
    }

    /// A composed alias is itself a pseudonym, so it can be composed again and
    /// can travel in a `Via` element like any other.
    #[test]
    fn a_tenant_scoped_alias_is_itself_a_valid_pseudonym() {
        let composed = ContextSourcePseudonym::new("broker-a")
            .expect("valid pseudonym")
            .scoped_to_tenant(&tenant("alpha"));
        pretty_assertions::assert_eq!(ContextSourcePseudonym::new(composed.as_str()).expect("valid pseudonym"), composed);
    }

    /// Every tenant label the grammar admits composes to a valid pseudonym --
    /// leading underscore, hyphens, digits and the maximum length included.
    #[test]
    fn every_admissible_tenant_label_composes_to_a_valid_pseudonym() {
        let broker = ContextSourcePseudonym::new("urn:ngsi-ld:ContextSourceIdentity:broker1").expect("valid pseudonym");
        for label in ["_internal", "A_B", "tenant-a", "t9", &"a".repeat(255)] {
            let composed = broker.scoped_to_tenant(&tenant(label));
            assert!(
                ContextSourcePseudonym::new(composed.as_str()).is_ok(),
                "composed alias must stay a pseudonym: {composed}"
            );
            assert!(composed.as_str().ends_with(label), "the tenant must be recoverable from {composed}");
        }
    }
}
