//! The expanded IRI of every attribute key a narrowing matches a projection
//! against.
//!
//! A clause 4.21 term carries an expanded IRI while a node is keyed by the
//! names its representation uses, so matching the two needs the request
//! `@context`. The engine holds no JSON-LD processor; the consumer hands it an
//! expander bound to that context, and the index is built here, once per
//! request, one expansion per distinct key.

use crate::keyword::Keyword;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_query::{error::QueryError, projection::attribute_iri_index::AttributeIriIndex, term_expander::TermExpander};
use indexmap::IndexSet;
use std::future::Future;

/// The index of `keys` expanded through `expander`.
///
/// A key that is no attribute name -- one of the clause 4.21 keyword members,
/// or a spelling the clause 4.6.2 grammar refuses -- is left out, which only
/// means no term can match it.
///
/// # Errors
///
/// Returns [`QueryError`] when a key cannot be expanded through the context.
pub fn attribute_iri_index<'k, E: TermExpander>(
    keys: impl IntoIterator<Item = &'k str>,
    expander: &'k E,
) -> impl Future<Output = Result<AttributeIriIndex, QueryError>> + Send + 'k {
    let keys: IndexSet<&'k str> = keys.into_iter().filter(|key| Keyword::of_key(key).is_none()).collect();
    async move {
        let mut index = AttributeIriIndex::new();
        for key in keys {
            let Ok(name) = key.parse::<AttributeName>() else {
                continue;
            };
            let iri = expander.expand_term(key).await?;
            index.insert(name, iri);
        }
        Ok(index)
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_index::attribute_iri_index;
    use andromeda_ngsi_ld_query::{error::QueryError, term_expander::TermExpander};
    use iri_rs::IriBuf;
    use pretty_assertions::assert_eq;
    use std::{
        future::{Future, ready},
        sync::Mutex,
    };

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
        let index = attribute_iri_index(["name", "owner", "name"], &expander).await.expect("expands");
        assert_eq!(*expander.asked.lock().expect("never poisoned"), vec!["name", "owner"]);
        assert_eq!(index.expanded("owner").map(IriBuf::as_str), Some("https://example.com/owner"));
    }

    #[tokio::test]
    async fn a_key_that_is_no_attribute_name_is_left_out() {
        let expander = CountingExpander::default();
        let index = attribute_iri_index(["name", "not an attribute"], &expander).await.expect("expands");
        assert!(index.expanded("not an attribute").is_none());
        assert_eq!(index.expanded("name").map(IriBuf::as_str), Some("https://example.com/name"));
    }

    #[tokio::test]
    async fn structural_members_are_not_indexed() {
        let expander = CountingExpander::default();
        let index = attribute_iri_index(["id", "type", "scope"], &expander).await.expect("expands");
        assert!(expander.asked.lock().expect("never poisoned").is_empty());
        assert!(index.expanded("id").is_none());
    }
}
