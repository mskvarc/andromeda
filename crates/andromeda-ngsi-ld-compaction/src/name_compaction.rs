//! Compacting one expanded NGSI-LD name to the term a context spells it with.
//!
//! An Entity Type name and an Attribute name share the clause 4.6.2 grammar and
//! both sit in vocabulary position. The first answer is JSON-LD's own: the term
//! IRI compaction selects for the name as the value of the core context's
//! `attributeList`, which is declared `"@type": "@vocab"` and protected
//! (clause C.9), so no request context can change what it measures. A term
//! that coerces a datatype is never selected for such a value, yet clause C.6
//! still makes it the name of the Attribute, and Table 5.2.28-1 conditions the
//! short name on membership of the `@context` alone; the second answer is
//! therefore the shortest, then lexicographically least, term the request's
//! own contexts declare for the name. A name neither answer shortens is
//! answered verbatim, as clause 4.5.1 permits a full IRI where no term
//! abbreviates it. Only a name in one of the schemes an expanded NGSI-LD name
//! is written in (`http`, `https`, `urn`) is compacted at all; any other name
//! is answered verbatim before either answer is asked.
//!
//! Both answers are read off the processed context, without a document being
//! compacted for them.

use crate::{compacted::Compacted, expanded_item::NodeIdentifier, term_context::TermContext};
use andromeda_jsonld::engine::{compaction_context::CompactionContext, context_url::is_full_iri};
use andromeda_ngsi_ld_vocabulary::{compact_to_expanded, expanded::properties};
use dashmap::DashMap;
use jsonld::{
    Id,
    IriBuf,
    Term,
    compaction::{ValueHint, ValueKind},
};
use std::sync::Arc;

/// Compacts expanded names against one processed request context, each name
/// once per request.
#[derive(Debug)]
pub struct NameCompaction {
    /// The processed request context.
    terms: TermContext,
    /// The core term carrying a name in vocabulary position.
    carrier: Term<IriBuf, jsonld::BlankIdBuf>,
    /// The answer for each name compacted so far.
    names: DashMap<Box<str>, Arc<str>>,
}

impl NameCompaction {
    /// Compact names against `context`, the processed request context of
    /// [`ContextCompactor::processed_context`].
    ///
    /// [`ContextCompactor::processed_context`]: andromeda_jsonld::engine::context_compactor::ContextCompactor::processed_context
    #[must_use]
    pub fn new(context: Arc<CompactionContext>) -> NameCompaction {
        NameCompaction {
            terms: TermContext::new(context),
            carrier: Term::Id(Id::iri(IriBuf::from(properties::ATTRIBUTE_LIST))),
            names: DashMap::new(),
        }
    }

    /// The term the context spells an expanded entity type name with, or the
    /// name itself.
    #[must_use]
    pub fn compact_type_iri(&self, iri: &str) -> Arc<str> {
        self.compact_name(iri)
    }

    /// The term the context spells an expanded attribute name with, or the
    /// name itself.
    #[must_use]
    pub fn compact_attr_iri(&self, iri: &str) -> Arc<str> {
        self.compact_name(iri)
    }

    /// The short name of `iri`, remembered for the request.
    fn compact_name(&self, iri: &str) -> Arc<str> {
        if !is_full_iri(iri) {
            return Arc::from(iri);
        }
        if let Some(name) = self.names.get(iri) {
            return Arc::clone(name.value());
        }
        let name: Arc<str> = match IriBuf::new(iri.to_owned()) {
            Ok(parsed) => self
                .vocabulary_position_term(&Id::iri(parsed), iri)
                .or_else(|| self.declared_term(iri).map(Arc::from))
                .unwrap_or_else(|| Arc::from(iri)),
            Err(_) => Arc::from(iri),
        };
        self.names.insert(Box::from(iri), Arc::clone(&name));
        name
    }

    /// The term JSON-LD's own compaction selects for the name in vocabulary
    /// position, unless it selects none or the name itself.
    fn vocabulary_position_term(&self, id: &NodeIdentifier, iri: &str) -> Option<Arc<str>> {
        let key = self
            .terms
            .property_key(&self.carrier, ValueHint::new(ValueKind::Node { id: Some(id) }))
            .ok()??;
        let name = match self.terms.node_reference(id, &key).ok()? {
            Compacted::String(name) => name,
            Compacted::Array(items) => match items.into_iter().next()? {
                Compacted::String(name) => name,
                Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::Array(_) | Compacted::Object(_) | Compacted::Geometry(_) => {
                    return None;
                }
            },
            Compacted::Null | Compacted::Boolean(_) | Compacted::Number(_) | Compacted::Object(_) | Compacted::Geometry(_) => return None,
        };
        (&*name != iri).then(|| Arc::from(name))
    }

    /// The shortest, then lexicographically least, term a request context
    /// declares for the name.
    ///
    /// The core context's own terms are not candidates: a name the core
    /// context declares is never hidden by a coercion, so vocabulary-position
    /// compaction has already answered it.
    fn declared_term(&self, iri: &str) -> Option<&str> {
        self.terms
            .context()
            .definitions()
            .iter()
            .filter_map(|binding| {
                let term = binding.term().as_str();
                let definition = binding.definition();
                let declares = !definition.reverse_property()
                    && compact_to_expanded(term).is_none()
                    && definition
                        .value()
                        .is_some_and(|value| value.as_iri().is_some_and(|value| value.as_str() == iri));
                declares.then_some(term)
            })
            .min_by(|left, right| (left.len(), *left).cmp(&(right.len(), *right)))
    }
}

#[cfg(test)]
mod tests {
    use crate::name_compaction::NameCompaction;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_vocabulary::expanded::classes;
    use serde_json::json;

    /// The names `context` compacts to, under an engine that resolves only
    /// what is embedded, never the network.
    async fn names(context: &ContextSpec) -> NameCompaction {
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("engine builds");
        NameCompaction::new(engine.compactor(context).processed_context().await.expect("the context processes"))
    }

    /// The clause C.2.4 vocabulary, carried in the request body so the test
    /// needs no network.
    fn example_context() -> ContextSpec {
        ContextSpec::from_body(json!({
            "Vehicle": "http://example.org/vehicle/Vehicle",
            "brandName": "http://example.org/vehicle/brandName",
            "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
            "testedAt": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"}
        }))
    }

    #[tokio::test]
    async fn a_name_that_is_not_an_iri_is_answered_verbatim() {
        let names = names(&ContextSpec::none()).await;
        pretty_assertions::assert_eq!(names.compact_type_iri("Vehicle").as_ref(), "Vehicle");
        pretty_assertions::assert_eq!(names.compact_attr_iri("speed").as_ref(), "speed");
    }

    #[tokio::test]
    async fn a_core_context_type_iri_compacts_back_to_its_term() {
        pretty_assertions::assert_eq!(
            names(&ContextSpec::none()).await.compact_type_iri(classes::PROPERTY.as_str()).as_ref(),
            "Property"
        );
    }

    /// Clause 4.5.1: a full IRI is a valid rendering where no term abbreviates
    /// it.
    #[tokio::test]
    async fn an_iri_no_context_abbreviates_is_answered_verbatim() {
        let names = names(&ContextSpec::core_only()).await;
        let iri = "https://example.com/vocab#unknownTerm";
        pretty_assertions::assert_eq!(names.compact_type_iri(iri).as_ref(), iri);
        pretty_assertions::assert_eq!(names.compact_attr_iri(iri).as_ref(), iri);
    }

    /// Table 5.2.28-1 conditions the short name on membership of the
    /// `@context`; a `@type: @id` term definition is membership, so the name
    /// compacts exactly as a plain declaration does.
    #[tokio::test]
    async fn a_term_declared_with_an_identifier_coercion_still_names_its_attribute() {
        let names = names(&example_context()).await;
        pretty_assertions::assert_eq!(names.compact_attr_iri("http://example.org/common/isParked").as_ref(), "isParked");
        pretty_assertions::assert_eq!(names.compact_attr_iri("http://example.org/vehicle/brandName").as_ref(), "brandName");
    }

    /// Clause C.6: a term coercing a datatype is not a candidate for a reified
    /// value, but it is still what the `@context` calls the Attribute.
    #[tokio::test]
    async fn a_term_declared_with_a_datatype_coercion_still_names_its_attribute() {
        let names = names(&example_context()).await;
        pretty_assertions::assert_eq!(names.compact_attr_iri("http://example.org/test/testedAt").as_ref(), "testedAt");
    }

    /// A name no supplied context declares stays a Fully Qualified Name, which
    /// is what clauses C.5.10 and C.5.11 print for the one attribute their
    /// `@context` leaves out.
    #[tokio::test]
    async fn a_name_the_supplied_context_does_not_declare_stays_fully_qualified() {
        let iri = "http://example.org/parking/status";
        pretty_assertions::assert_eq!(names(&example_context()).await.compact_attr_iri(iri).as_ref(), iri);
    }

    /// An entity type name takes the same route as an attribute name, so the
    /// two surfaces cannot answer the same `@context` differently.
    #[tokio::test]
    async fn an_entity_type_name_compacts_through_the_same_declaration() {
        let names = names(&example_context()).await;
        pretty_assertions::assert_eq!(names.compact_type_iri("http://example.org/vehicle/Vehicle").as_ref(), "Vehicle");
    }

    /// Two terms declaring one IRI are ordered as JSON-LD orders compaction
    /// candidates: shortest first, then lexicographically least.
    #[tokio::test]
    async fn the_shortest_declaration_of_one_iri_is_the_name_rendered() {
        let names = names(&ContextSpec::from_body(json!({
            "zz": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"},
            "aaa": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"},
            "yy": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"}
        })))
        .await;
        pretty_assertions::assert_eq!(names.compact_attr_iri("http://example.org/test/testedAt").as_ref(), "yy");
    }

    /// One compaction serves every name of a request: a name asked twice is
    /// answered as it was the first time.
    #[tokio::test]
    async fn a_name_asked_again_is_answered_as_before() {
        let names = names(&example_context()).await;
        for iri in [
            "http://example.org/vehicle/brandName",
            "http://example.org/test/testedAt",
            "http://example.org/common/isParked",
            "http://example.org/parking/status",
        ] {
            let first = names.compact_attr_iri(iri);
            pretty_assertions::assert_eq!(names.compact_attr_iri(iri), first, "{iri}");
        }
    }
}
