//! Per-run identity and everything derived from the isolation prefix.
//!
//! A prefix-isolated test owns one [`Scope`]. Every identifier it puts on the
//! wire is composed below the scope's non-empty prefix, so the shared-lock
//! cleanup reclaims exactly the rows the test created and nothing a concurrent
//! test owns. Types and Entity ids are derived here rather than in each suite so
//! that isolation holds by construction instead of by every fixture remembering
//! to scope itself.

use crate::identifiers::{IdentifierError, ResourceKind, ResourceLabel, TestIdentifiers, TestSuiteName};
use iri_rs::IriBuf;

/// Identity of one isolated test run: the cleanup prefix and its derivations.
#[derive(Clone, Debug)]
pub struct Scope {
    identifiers: TestIdentifiers,
}

impl Scope {
    /// Allocate a fresh, process-independent scope for a named suite.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::GeneratedIri`] when the composed
    /// `urn:ngsi-ld:test:` prefix does not parse as an IRI.
    pub fn allocate(suite: &TestSuiteName) -> Result<Scope, IdentifierError> {
        Ok(Scope {
            identifiers: TestIdentifiers::allocate(suite)?,
        })
    }

    /// Borrow the structured identifier allocator backing this scope.
    #[must_use]
    pub const fn identifiers(&self) -> &TestIdentifiers {
        &self.identifiers
    }

    /// Compose a wire identifier below this run's non-empty isolated prefix.
    #[must_use]
    pub fn id(&self, local: &str) -> String {
        format!("{}:{local}", self.identifiers.prefix())
    }

    /// Borrow the non-empty cleanup prefix as wire text.
    #[must_use]
    pub fn id_prefix(&self) -> &str {
        self.identifiers.prefix().as_str()
    }

    /// Allocate one typed resource identifier under this run's prefix.
    ///
    /// # Errors
    ///
    /// Returns [`IdentifierError::GeneratedIri`] when the prefix, the kind
    /// token, and the label do not compose into a valid IRI.
    pub fn resource(&self, kind: ResourceKind, label: &ResourceLabel) -> Result<IriBuf, IdentifierError> {
        self.identifiers.resource(kind, label)
    }

    /// A `RegExp` `idPattern` (clause 4.9) matching exactly the ids this run
    /// composes with [`Scope::id`], for a subscription or registration
    /// `entities` selector: every such id begins with the prefix followed by a
    /// colon, so `^<prefix>:` admits the run's own resources and nothing a
    /// concurrent test owns. The prefix is a URN whose only characters are
    /// alphanumerics, `:` and `-`, none of which carry `RegExp` meaning, so the
    /// pattern needs no escaping.
    #[must_use]
    pub fn id_pattern(&self) -> String {
        format!("^{}:", self.id_prefix())
    }

    /// Append this run's [`Scope::id_pattern`] to a query string as an
    /// `idPattern` parameter, URL-encoded, so a collection query returns only
    /// the entities the run created. Handles both an empty query and one that
    /// already carries parameters.
    #[must_use]
    pub fn scope_query(&self, query: &str) -> String {
        // A query that already carries its own `idPattern` is scoped by the
        // caller -- a test exercising the `idPattern` parameter itself, whose
        // pattern targets its own entities. Appending a second `idPattern`
        // would contradict it, so the caller's scoping stands.
        if query.contains("idPattern=") {
            return query.to_owned();
        }
        let pattern = urlencoding::encode(&self.id_pattern()).into_owned();
        if query.is_empty() {
            format!("idPattern={pattern}")
        } else {
            format!("{query}&idPattern={pattern}")
        }
    }

    /// An Entity type unique to this run, derived from the isolation prefix.
    ///
    /// A scenario that registers a Context Source (clause 5.12) or a
    /// Subscription (clause 5.8) and then selects by Entity type is repeatable
    /// only while that type is unique to the run: a shared literal type matches
    /// the run's own earlier executions and every concurrent test that uses it.
    /// The prefix is the token the harness already isolates every identifier
    /// under, so the derived type needs no second source of uniqueness. Only the
    /// trailing alphanumeric characters are taken, keeping the result a short
    /// valid term.
    #[must_use]
    pub fn entity_type(&self, base: &str) -> String {
        let unique: String = self.id_prefix().chars().filter(char::is_ascii_alphanumeric).rev().take(12).collect();
        format!("{base}{unique}")
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        identifiers::{ResourceKind, ResourceLabel, TestSuiteName},
        scope::Scope,
    };
    use pretty_assertions::assert_ne;

    fn scope() -> Scope {
        let suite = TestSuiteName::parse("regression").unwrap_or_else(|error| panic!("static suite name is valid: {error}"));
        Scope::allocate(&suite).unwrap_or_else(|error| panic!("generated scope is valid: {error}"))
    }

    #[test]
    fn composed_ids_sit_below_the_run_prefix() {
        let scope = scope();
        let id = scope.id("Building:1");
        assert!(id.starts_with(scope.id_prefix()), "{id} must be composed below {}", scope.id_prefix());
        assert!(id.ends_with(":Building:1"));
    }

    #[test]
    fn separate_scopes_never_share_a_prefix() {
        assert_ne!(scope().id_prefix().to_owned(), scope().id_prefix().to_owned());
    }

    #[test]
    fn derived_entity_types_are_unique_per_run_and_stay_valid_terms() {
        let base = "Building";
        let first = scope().entity_type(base);
        let second = scope().entity_type(base);
        assert_ne!(first, second);
        assert!(first.starts_with(base));
        assert!(
            first.chars().all(|character| character.is_ascii_alphanumeric()),
            "{first} must be a bare alphanumeric term usable as a JSON-LD type token",
        );
    }

    #[test]
    fn the_id_pattern_anchors_on_the_run_prefix_and_matches_its_own_ids() {
        let scope = scope();
        let pattern = scope.id_pattern();
        assert!(pattern.starts_with('^'));
        assert_eq!(pattern, format!("^{}:", scope.id_prefix()));
        assert!(scope.id("Building:1").starts_with(&pattern[1..]));
    }

    #[test]
    fn scope_query_appends_the_url_encoded_pattern_to_any_query() {
        let scope = scope();
        let empty = scope.scope_query("");
        assert!(empty.starts_with("idPattern="));
        assert!(!empty.contains('&'));

        let filtered = scope.scope_query("type=Building&count=true");
        assert!(filtered.starts_with("type=Building&count=true&idPattern="));
        assert!(filtered.contains("%3A"), "the colons of the URN prefix must be percent-encoded: {filtered}");
        assert!(
            !filtered.contains(":^") && !filtered.ends_with(':'),
            "the pattern must be encoded, not raw: {filtered}"
        );
    }

    #[test]
    fn scope_query_leaves_a_query_that_already_carries_an_id_pattern_untouched() {
        let scope = scope();
        let own = "idPattern=urn:ngsi-ld:Building:mine.*&type=Building";
        assert_eq!(scope.scope_query(own), own, "a caller's own idPattern must not gain a second one");
    }

    #[test]
    fn typed_resources_are_namespaced_below_the_prefix() {
        let scope = scope();
        let label = ResourceLabel::parse("A").unwrap_or_else(|error| panic!("static label is valid: {error}"));
        let subscription = scope
            .resource(ResourceKind::Subscription, &label)
            .unwrap_or_else(|error| panic!("generated resource is valid: {error}"));
        assert!(subscription.as_str().starts_with(scope.id_prefix()));
        assert!(subscription.as_str().contains(":subscription:"));
    }
}
