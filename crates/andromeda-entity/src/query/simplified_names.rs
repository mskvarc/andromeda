//! The Attribute names the clause 4.5.4 simplified representation renders.
//!
//! Every representation but the simplified one is written from the compaction
//! of the whole expanded Entity, and that compaction is what decides an
//! Attribute's rendered name. Clause C.6 states the one case where the two
//! differ: a `@context` that coerces a datatype onto a term — "`"testedAt":
//! {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id":
//! "http://example.org/test/testedAt"}`" — cannot name a reified Property,
//! "because in this case testedAt is a complex JSON object, which cannot be
//! compacted to a DateTime type as the @context specifies. Thus, the full URI
//! http://example.org/test/testedAt is kept, instead of the short name
//! testedAt", and the clause then singles out the representation where the
//! obstacle is absent: "However, in the simplified (keyValue) representation
//! case, such an @context with the specification of testedAt could be used, as
//! there is no reification."
//!
//! So the simplified rendering asks what term the `@context` declares for the
//! name, which is the same question Table 5.2.28-1 asks of every discovery
//! surface. Only a name the document compaction left fully qualified is asked
//! about: one it already shortened is rendered exactly as the normalized
//! representation renders it, so the two cannot disagree about a name neither
//! clause distinguishes.

use crate::{error::EntityError, query::EntityQueryService};
use andromeda_jsonld::{compaction::compact_attr_iri, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use indexmap::IndexMap;

impl EntityQueryService {
    /// Rename an Entity's Attributes to the short names `context` declares for
    /// them, for a response written in the clause 4.5.4 simplified
    /// representation.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::Validation`] when a declared term is not a name
    /// the clause 4.6.2 grammar admits, which would leave the Attribute
    /// unaddressable under the very context that named it.
    pub async fn apply_simplified_names(&self, entity: &mut Entity, context: &ContextSpec) -> Result<(), EntityError> {
        if !entity.attrs.keys().any(is_fully_qualified) {
            return Ok(());
        }

        let mut renamed: IndexMap<AttributeName, _> = IndexMap::with_capacity(entity.attrs.len());
        for (name, value) in entity.attrs.drain(..) {
            let rendered = if is_fully_qualified(&name) {
                self.declared_name(&name, context).await?
            } else {
                name
            };
            renamed.insert(rendered, value);
        }
        entity.attrs = renamed;
        Ok(())
    }

    /// The short name `context` declares for a fully qualified Attribute name,
    /// or the name itself when it declares none.
    async fn declared_name(&self, name: &AttributeName, context: &ContextSpec) -> Result<AttributeName, EntityError> {
        let compacted = compact_attr_iri(self.engine(), name.as_str(), context).await;
        compacted
            .parse()
            .map_err(|error| EntityError::validation(format!("the context declares '{compacted}' for {name}, which is not an NGSI-LD name: {error}")))
    }
}

/// Whether a rendered Attribute name is still the expanded IRI.
///
/// Clause 4.6.2's `AttrName` production admits no colon, so a name carrying a
/// scheme separator is one no term of the request's `@context` shortened.
fn is_fully_qualified(name: &AttributeName) -> bool {
    name.as_str().contains(':')
}

#[cfg(test)]
mod tests {
    use crate::query::{service_test_double::service, simplified_names::is_fully_qualified};
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_ngsi_ld_model::{attribute::instance::AttributeValue, entity::Entity};
    use indexmap::IndexMap;
    use serde_json::json;

    fn name(value: &str) -> AttributeName {
        value.parse().expect("valid attribute name")
    }

    /// The coercing `@context` clause C.6 prints, beside a plain declaration
    /// and the term whose definition coerces an identifier.
    fn coercing_context() -> ContextSpec {
        ContextSpec::none().with_body_context(Some(json!({
            "brandName": "http://example.org/vehicle/brandName",
            "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
            "testedAt": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"}
        })))
    }

    /// One Entity whose Attribute keys are the names the document compaction
    /// of the normalized representation produced, in the order it produced
    /// them.
    fn entity(keys: &[&str]) -> Entity {
        let attrs: IndexMap<AttributeName, AttributeValue> = keys
            .iter()
            .map(|key| {
                let value = serde_json::from_value(json!({"type": "Property", "value": 1})).expect("a well-formed Property");
                (name(key), value)
            })
            .collect();
        Entity::builder()
            .id("urn:ngsi-ld:Vehicle:B9211".parse().expect("a valid Entity identifier"))
            .type_(serde_json::from_value(json!("Vehicle")).expect("a valid Entity type"))
            .attrs(attrs)
            .build()
    }

    /// Clause 4.6.2 admits no colon in an `AttrName`, so the separator is what
    /// tells an expanded name from a compacted one.
    #[test]
    fn a_name_carrying_a_scheme_separator_is_the_one_that_did_not_compact() {
        assert!(is_fully_qualified(&name("http://example.org/test/testedAt")));
        assert!(is_fully_qualified(&name("https://uri.etsi.org/ngsi-ld/default-context/speed")));
        assert!(!is_fully_qualified(&name("speed")));
        assert!(!is_fully_qualified(&name("testedAt")));
    }

    /// Clause C.6: the name the coercion hid from the document compaction is
    /// the one the simplified representation renders short.
    #[tokio::test]
    async fn a_coerced_term_names_the_attribute_of_the_simplified_representation() {
        let service = service();
        let mut entity = entity(&["http://example.org/test/testedAt", "http://example.org/common/isParked"]);

        service
            .apply_simplified_names(&mut entity, &coercing_context())
            .await
            .expect("the declared terms are NGSI-LD names");

        let rendered: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec!["testedAt", "isParked"]);
    }

    /// A name the `@context` does not declare stays fully qualified, and a name
    /// the document compaction already shortened is left exactly as it is.
    #[tokio::test]
    async fn a_name_no_term_declares_is_left_as_the_compaction_produced_it() {
        let service = service();
        let mut entity = entity(&["http://example.org/parking/status", "brandName"]);

        service
            .apply_simplified_names(&mut entity, &coercing_context())
            .await
            .expect("no declared term is ungrammatical");

        let rendered: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec!["http://example.org/parking/status", "brandName"]);
    }

    /// The order the Attributes were rendered in is what a client reads them
    /// back in, so renaming keeps it.
    #[tokio::test]
    async fn renaming_keeps_the_order_the_attributes_were_rendered_in() {
        let service = service();
        let mut entity = entity(&["brandName", "http://example.org/test/testedAt", "http://example.org/parking/status"]);

        service
            .apply_simplified_names(&mut entity, &coercing_context())
            .await
            .expect("the declared terms are NGSI-LD names");

        let rendered: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec!["brandName", "testedAt", "http://example.org/parking/status"]);
    }
}
