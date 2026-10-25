//! The expanded IRI of each attribute key a projection may be matched against.
//!
//! [`ProjectionTerm::matches_attribute`] needs an attribute key's expanded IRI, and expanding a key needs the request `@context`. Anything applying a projection outside the layer that owns the JSON-LD engine (the renderer applying a clause 4.21 nested projection while it nests linked entities, above all) therefore needs that expansion handed to it already resolved. This index is that hand-over and the one place that expands the keys: each distinct key once per request, through the expander the consumer binds to the request `@context`.
//!
//! [`ProjectionTerm::matches_attribute`]: crate::projection::term::ProjectionTerm::matches_attribute

use crate::{error::QueryError, projection::term::ProjectionTerm, term_expander::TermExpander};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use iri_rs::IriBuf;
use std::{collections::HashMap, future::Future};

/// Attribute keys mapped to the IRI they expand to under one request `@context`. The index is built from the keys the payload carries, so an absent key is one no entity in the response holds and simply matches no projection term.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AttributeIriIndex {
    /// Each recorded attribute key and the IRI it expands to.
    expansions: HashMap<AttributeName, IriBuf>,
}

impl AttributeIriIndex {
    /// Create an empty index.
    #[must_use]
    pub fn new() -> AttributeIriIndex {
        AttributeIriIndex { expansions: HashMap::new() }
    }

    /// The index of `keys` expanded through `expander`.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError`] when a key cannot be expanded through the
    /// context.
    pub fn of_keys<'k, E: TermExpander>(
        keys: impl IntoIterator<Item = &'k str>,
        expander: &'k E,
    ) -> impl Future<Output = Result<AttributeIriIndex, QueryError>> + Send + 'k {
        let mut index = AttributeIriIndex::new();
        let expansion = index.missing_keys(keys);
        async move {
            index.expand(expansion, expander).await?;
            Ok(index)
        }
    }

    /// Expands into the index every key of `keys` it does not cover yet. A key that is no attribute name (a clause 4.21 keyword member, or a spelling the clause 4.6.2 grammar refuses) is left out, which only means no term can match it.
    ///
    /// # Errors
    ///
    /// Returns [`QueryError`] when a key cannot be expanded through the
    /// context.
    pub fn cover<'a, E: TermExpander>(
        &'a mut self,
        keys: impl IntoIterator<Item = &'a str>,
        expander: &'a E,
    ) -> impl Future<Output = Result<(), QueryError>> + Send + 'a {
        let expansion = self.missing_keys(keys);
        self.expand(expansion, expander)
    }

    /// The distinct attribute names among `keys` the index does not cover, in
    /// first-seen order. Read before any expansion starts, so the iterator is
    /// never held across one.
    fn missing_keys<'k>(&self, keys: impl IntoIterator<Item = &'k str>) -> Vec<(&'k str, AttributeName)> {
        let mut missing: Vec<(&'k str, AttributeName)> = Vec::new();
        for key in keys {
            if ProjectionTerm::keyword(key).is_some() || self.expanded(key).is_some() || missing.iter().any(|(seen, _)| *seen == key) {
                continue;
            }
            if let Ok(name) = key.parse::<AttributeName>() {
                missing.push((key, name));
            }
        }
        missing
    }

    /// Expand and record each of `missing`.
    async fn expand<E: TermExpander>(&mut self, missing: Vec<(&str, AttributeName)>, expander: &E) -> Result<(), QueryError> {
        for (key, name) in missing {
            let iri = expander.expand_term(key).await?;
            self.insert(name, iri);
        }
        Ok(())
    }

    /// Record the IRI `key` expands to, replacing any earlier expansion of the
    /// same key.
    pub fn insert(&mut self, key: AttributeName, expanded: IriBuf) {
        self.expansions.insert(key, expanded);
    }

    /// The IRI `key` expands to, or `None` when the index does not cover it.
    #[must_use]
    pub fn expanded(&self, key: &str) -> Option<&IriBuf> {
        self.expansions.get(key)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{error::QueryError, projection::attribute_iri_index::AttributeIriIndex, term_expander::TermExpander};
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::{
        future::{Future, ready},
        sync::Mutex,
    };

    fn iri(value: &str) -> IriBuf {
        IriBuf::new(value.to_string()).expect("valid IRI")
    }

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// A recorded key is looked up by the plain key text a rendered value carries.
    #[test]
    fn a_recorded_key_resolves_to_its_expansion() {
        let mut index = AttributeIriIndex::new();
        index.insert(name("speed"), iri("https://example.com/speed"));

        assert_eq!(index.expanded("speed"), Some(&iri("https://example.com/speed")));
    }

    /// A key the index never saw resolves to nothing rather than to a guess.
    #[test]
    fn an_unrecorded_key_resolves_to_nothing() {
        let index = AttributeIriIndex::new();

        assert!(index.expanded("speed").is_none());
    }

    /// An expander prefixing every term, counting what it was asked.
    #[derive(Default)]
    struct CountingExpander {
        asked: Mutex<Vec<String>>,
    }

    impl TermExpander for CountingExpander {
        fn expand_term(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            self.asked.lock().expect("never poisoned").push(term.to_owned());
            ready(Ok(IriBuf::new(format!("https://example.com/{term}")).expect("valid IRI")))
        }

        fn expand_type(&self, term: &str) -> impl Future<Output = Result<IriBuf, QueryError>> + Send {
            ready(Ok(IriBuf::new(format!("https://example.com/types/{term}")).expect("valid IRI")))
        }
    }

    #[tokio::test]
    async fn every_distinct_key_is_expanded_once() {
        let expander = CountingExpander::default();
        let index = AttributeIriIndex::of_keys(["name", "owner", "name"], &expander).await.expect("expands");
        assert_eq!(*expander.asked.lock().expect("never poisoned"), vec!["name", "owner"]);
        assert_eq!(index.expanded("owner").map(IriBuf::as_str), Some("https://example.com/owner"));
    }

    #[tokio::test]
    async fn a_key_that_is_no_attribute_name_is_left_out() {
        let expander = CountingExpander::default();
        let index = AttributeIriIndex::of_keys(["name", "not an attribute"], &expander).await.expect("expands");
        assert!(index.expanded("not an attribute").is_none());
        assert_eq!(index.expanded("name").map(IriBuf::as_str), Some("https://example.com/name"));
    }

    #[tokio::test]
    async fn structural_members_are_not_indexed() {
        let expander = CountingExpander::default();
        let index = AttributeIriIndex::of_keys(["id", "type", "scope"], &expander).await.expect("expands");
        assert!(expander.asked.lock().expect("never poisoned").is_empty());
        assert!(index.expanded("id").is_none());
    }

    #[tokio::test]
    async fn covering_expands_only_the_keys_not_yet_indexed() {
        let expander = CountingExpander::default();
        let mut index = AttributeIriIndex::of_keys(["name"], &expander).await.expect("expands");
        index.cover(["name", "owner"], &expander).await.expect("expands");
        assert_eq!(*expander.asked.lock().expect("never poisoned"), vec!["name", "owner"]);
    }
}
