//! The per-test namespace: a token no other test, and no other run, uses, and the identifiers, names and patterns
//! built on it.
//!
//! Every test runs in the shared default tenant beside every other test. Each resource a test creates, each entity
//! type it selects on and each endpoint it hands a broker carries the token, so a selector the test sends can only
//! ever see the test's own data.

use iri_rs::{InvalidIri, IriBuf};
use ngsi_ld::{
    model::{AttributeName, DatasetId, EntityId, EntityTypeName, NameError, Tenant, TenantError, Uri, UriError},
    query::{IdPattern, RegularExpressionError},
    registration::RegistrationId,
    subscription::SubscriptionId,
};
use std::{
    fmt::{self, Display},
    iter,
};
use uuid::Uuid;

/// The characters a token is made of after its leading letter.
const TOKEN_CHARACTERS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// The characters a token holds after its leading letter: 36^12 tokens, so two of the suite's processes never draw
/// the same one.
const TOKEN_TAIL_LENGTH: usize = 12;

/// The bytes of a version 4 UUID that hold its version and variant bits.
const RESERVED_UUID_BYTES: [usize; 2] = [6, 8];

/// Why a value built on the namespace is not valid.
///
/// The token is lowercase letters and digits starting with a letter, valid in every position the suite places it;
/// these errors name a base word or suffix the test supplied that is not.
#[derive(Debug, thiserror::Error)]
pub enum NamespaceError {
    /// An identifier is not a URI.
    #[error("an identifier of the namespace {0} is not a URI")]
    Uri(Namespace, #[source] UriError),
    /// An IRI is not an absolute IRI.
    #[error("a vocabulary IRI is not an IRI")]
    Iri(#[source] InvalidIri<String>),
    /// A name is not an NGSI-LD name.
    #[error("a name of the namespace {0} is not an NGSI-LD name")]
    Name(Namespace, #[source] NameError),
    /// A pattern is not a regular expression.
    #[error("a pattern of the namespace {0} is not a regular expression")]
    Pattern(Namespace, #[source] RegularExpressionError),
    /// A tenant name is not valid.
    #[error("the tenant of the namespace {0} is not a tenant name")]
    Tenant(Namespace, #[source] TenantError),
}

/// The token of one test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Namespace {
    /// Lowercase letters and digits, starting with a letter.
    token: String,
}

impl Namespace {
    /// A fresh namespace, drawn from the operating system's random source.
    ///
    /// The test processes of one run start within microseconds of each other, so a generator seeded from the clock
    /// and the thread could hand two of them the same token; a version 4 UUID carries 122 bits from the operating
    /// system. Its two bytes holding the version and the variant are skipped.
    #[must_use]
    pub fn fresh() -> Namespace {
        let bytes = Uuid::new_v4().into_bytes();
        let mut random = bytes
            .iter()
            .enumerate()
            .filter(|(index, _)| !RESERVED_UUID_BYTES.contains(index))
            .map(|(_, byte)| *byte);
        let leading = random.next().map_or('a', |byte| char::from(b'a' + byte % 26));
        let tail = random
            .take(TOKEN_TAIL_LENGTH)
            .map(|byte| char::from(TOKEN_CHARACTERS[usize::from(byte) % TOKEN_CHARACTERS.len()]));
        Namespace {
            token: iter::once(leading).chain(tail).collect(),
        }
    }

    /// The token.
    #[must_use]
    pub fn token(&self) -> &str {
        &self.token
    }

    /// The identifier `urn:ngsi-ld:<kind>:<token>-<suffix>`: an Entity, Subscription, Registration, Snapshot or
    /// `EntityMap` id of the test, a deliberately unknown one included (`unknown` as the suffix).
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Uri`] when `kind` or `suffix` breaks the URN.
    pub fn urn(&self, kind: &str, suffix: impl Display) -> Result<EntityId, NamespaceError> {
        let text = format!("urn:ngsi-ld:{kind}:{}-{suffix}", self.token);
        text.parse().map_err(|error| NamespaceError::Uri(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The URI `urn:ngsi-ld:<kind>:<token>-<suffix>` of the test where an operation takes a plain URI: the dataset
    /// a Delete Attribute names (clause 6.7.3.2).
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Uri`] when `kind` or `suffix` breaks the URI.
    pub fn uri(&self, kind: &str, suffix: impl Display) -> Result<Uri, NamespaceError> {
        let text = format!("urn:ngsi-ld:{kind}:{}-{suffix}", self.token);
        text.parse().map_err(|error| NamespaceError::Uri(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The dataset id `urn:ngsi-ld:<kind>:<token>-<suffix>` of an attribute instance of the test (clause 4.5.5).
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Uri`] when `kind` or `suffix` breaks the URI.
    pub fn dataset_id(&self, kind: &str, suffix: impl Display) -> Result<DatasetId, NamespaceError> {
        let text = format!("urn:ngsi-ld:{kind}:{}-{suffix}", self.token);
        text.parse().map_err(|error| NamespaceError::Uri(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The subscription id `urn:ngsi-ld:Subscription:<token>-<suffix>`.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Uri`] when `suffix` breaks the URN.
    pub fn subscription_id(&self, suffix: impl Display) -> Result<SubscriptionId, NamespaceError> {
        let text = self.urn_text("Subscription", suffix);
        SubscriptionId::try_from(text.as_str()).map_err(|error| NamespaceError::Uri(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The registration id `urn:ngsi-ld:ContextSourceRegistration:<token>-<suffix>`.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Uri`] when `suffix` breaks the URN.
    pub fn registration_id(&self, suffix: impl Display) -> Result<RegistrationId, NamespaceError> {
        let text = self.urn_text("ContextSourceRegistration", suffix);
        RegistrationId::try_from(text.as_str()).map_err(|error| NamespaceError::Uri(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The text `urn:ngsi-ld:<kind>:<token>-<suffix>`, for a resource kind whose identifier type the client takes as
    /// text.
    #[must_use]
    pub fn urn_text(&self, kind: &str, suffix: impl Display) -> String {
        format!("urn:ngsi-ld:{kind}:{}-{suffix}", self.token)
    }

    /// The pattern `urn:ngsi-ld:<kind>:<token>-.*`, which only the test's own identifiers of `kind` meet.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Pattern`] when `kind` breaks the pattern.
    pub fn id_pattern(&self, kind: &str) -> Result<IdPattern, NamespaceError> {
        self.id_pattern_ending(kind, ".*")
    }

    /// The pattern `urn:ngsi-ld:<kind>:<token>-<tail>`, which only identifiers of the test meet, as `tail` narrows
    /// them.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Pattern`] when `kind` or `tail` breaks the pattern.
    pub fn id_pattern_ending(&self, kind: &str, tail: &str) -> Result<IdPattern, NamespaceError> {
        let text = format!("urn:ngsi-ld:{kind}:{}-{tail}", self.token);
        text.parse().map_err(|error| NamespaceError::Pattern(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The entity type `<base>_<token>`, which expands under the default vocabulary to an IRI no other test uses.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Name`] when `base` is not the start of a name.
    pub fn entity_type(&self, base: &str) -> Result<EntityTypeName, NamespaceError> {
        let text = self.suffixed(base);
        text.parse().map_err(|error| NamespaceError::Name(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The attribute name `<base>_<token>`, for a selector that selects on the attribute alone.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Name`] when `base` is not the start of a name.
    pub fn attribute(&self, base: &str) -> Result<AttributeName, NamespaceError> {
        let text = self.suffixed(base);
        text.parse().map_err(|error| NamespaceError::Name(self.clone(), error)) // clone: the error names the namespace the test keeps
    }

    /// The text `<base>_<token>`.
    #[must_use]
    pub fn suffixed(&self, base: &str) -> String {
        format!("{base}_{}", self.token)
    }

    /// The IRI `https://test.andromeda/<token>/<term>` a test vocabulary maps `term` to.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Iri`] when `term` breaks the IRI.
    pub fn vocabulary_iri(&self, term: &str) -> Result<IriBuf, NamespaceError> {
        let text = format!("https://test.andromeda/{}/{term}", self.token);
        IriBuf::new(text).map_err(NamespaceError::Iri)
    }

    /// The text `https://test.andromeda/<token>/<term>`: the expanded form of `term` in a test vocabulary, as an
    /// answer compacted with no `@context` names it.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Iri`] when `term` breaks the IRI.
    pub fn expanded_term(&self, term: &str) -> Result<String, NamespaceError> {
        Ok(self.vocabulary_iri(term)?.into_inner())
    }

    /// The tenant named after the token, for a test that needs a tenant of its own.
    ///
    /// # Errors
    ///
    /// Returns [`NamespaceError::Tenant`] when the token is not a tenant name, which it always is.
    pub fn tenant(&self) -> Result<Tenant, NamespaceError> {
        self.token.parse().map_err(|error| NamespaceError::Tenant(self.clone(), error)) // clone: the error names the namespace the test keeps
    }
}

impl Display for Namespace {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.token)
    }
}

#[cfg(test)]
mod tests {
    use crate::namespace::Namespace;
    use googletest::prelude::*;

    #[gtest]
    fn a_token_is_lowercase_letters_and_digits_starting_with_a_letter() {
        let namespace = Namespace::fresh();
        let mut characters = namespace.token().chars();
        expect_that!(characters.next(), some(predicate(|first: char| first.is_ascii_lowercase())));
        expect_that!(characters.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()), eq(true));
        expect_that!(namespace.token().len(), eq(13));
    }

    #[gtest]
    fn two_fresh_namespaces_differ() {
        expect_that!(Namespace::fresh(), not(eq(&Namespace::fresh())));
    }

    #[gtest]
    fn identifiers_names_and_patterns_carry_the_token() -> Result<()> {
        let namespace = Namespace::fresh();
        let token = namespace.token();
        expect_that!(namespace.urn("Building", 1)?.as_str(), eq(&format!("urn:ngsi-ld:Building:{token}-1")));
        expect_that!(namespace.id_pattern("Vehicle")?.as_ref(), eq(&format!("urn:ngsi-ld:Vehicle:{token}-.*")));
        expect_that!(
            namespace.id_pattern_ending("Place", "[34]")?.as_ref(),
            eq(&format!("urn:ngsi-ld:Place:{token}-[34]"))
        );
        expect_that!(namespace.entity_type("Building")?.as_ref(), eq(&format!("Building_{token}")));
        expect_that!(namespace.attribute("speed")?.as_ref(), eq(&format!("speed_{token}")));
        expect_that!(
            namespace.vocabulary_iri("speed")?.as_str(),
            eq(&format!("https://test.andromeda/{token}/speed"))
        );
        expect_that!(namespace.tenant()?.as_ref(), eq(token));
        Ok(())
    }

    #[gtest]
    fn a_suffix_that_breaks_the_urn_is_refused() {
        expect_that!(Namespace::fresh().urn("Building", "a b"), err(anything()));
    }
}
