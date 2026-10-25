//! The Attribute names the clause 4.5.4 simplified representation renders.
//!
//! Every representation but the simplified one is written from the compaction
//! of the whole expanded Entity, and that compaction is what decides an
//! Attribute's rendered name. Clause C.6 states the one case where the two
//! differ: a `@context` that coerces a datatype onto a term, "`"testedAt":
//! {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id":
//! "http://example.org/test/testedAt"}`", cannot name a reified Property,
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

use crate::{entity_validation_reason::EntityValidationReason, error::EntityError, query::EntityQueryService};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_compaction::name_compaction::NameCompaction;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use indexmap::IndexMap;

/// The compaction of single names against the processed request context:
/// the short name Table 5.2.28-1 and clause C.6 give an expanded name.
///
/// # Errors
///
/// Reports [`EntityError::JsonLd`] when the request context does not resolve
/// or process (clause 5.5.7).
pub(crate) async fn request_name_compaction(engine: &JsonLdEngine, context: &ContextSpec) -> Result<NameCompaction, EntityError> {
    Ok(NameCompaction::new(engine.compactor(context).processed_context().await?))
}

impl EntityQueryService {
    /// The compaction of single names against the request context, for
    /// [`apply_simplified_names`].
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when the request context does not
    /// resolve or process (clause 5.5.7).
    pub async fn name_compaction(&self, context: &ContextSpec) -> Result<NameCompaction, EntityError> {
        request_name_compaction(self.engine(), context).await
    }
}

/// Rename an Entity's Attributes to the short names the context declares for
/// them, for a response written in the clause 4.5.4 simplified
/// representation.
///
/// # Errors
///
/// Reports [`EntityError::Validation`] when a declared term is not a name
/// the clause 4.6.2 grammar admits, which would leave the Attribute
/// unaddressable under the very context that named it.
pub fn apply_simplified_names(entity: &mut Entity, names: &NameCompaction) -> Result<(), EntityError> {
    if !entity.attrs.keys().any(is_fully_qualified) {
        return Ok(());
    }

    let mut renamed: IndexMap<AttributeName, _> = IndexMap::with_capacity(entity.attrs.len());
    for (name, value) in entity.attrs.drain(..) {
        let rendered = if is_fully_qualified(&name) { declared_name(&name, names)? } else { name };
        renamed.insert(rendered, value);
    }
    entity.attrs = renamed;
    Ok(())
}

/// The short name the context declares for a fully qualified Attribute name,
/// or the name itself when it declares none.
fn declared_name(name: &AttributeName, names: &NameCompaction) -> Result<AttributeName, EntityError> {
    let compacted = names.compact_attr_iri(name.as_str());
    compacted.parse().map_err(|error| {
        EntityError::Validation(EntityValidationReason::DeclaredName {
            requested: name.clone(),
            source: error,
        })
    })
}

/// Whether a rendered Attribute name is still the expanded IRI.
///
/// Clause 4.6.2's `AttrName` production admits no colon, so a name carrying a
/// scheme separator is one no term of the request's `@context` shortened.
fn is_fully_qualified(name: &AttributeName) -> bool {
    name.as_str().contains(':')
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::query::{
        service_test_double::service,
        simplified_names::{apply_simplified_names, is_fully_qualified},
    };
    use andromeda_json_literal::JsonLiteral;
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
        ContextSpec::from_body(
            JsonLiteral::admit(
                &json!({
                    "brandName": "http://example.org/vehicle/brandName",
                    "isParked": {"@type": "@id", "@id": "http://example.org/common/isParked"},
                    "testedAt": {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": "http://example.org/test/testedAt"}
                })
                .to_string(),
            )
            .expect("an admissible literal"),
        )
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
        let context = coercing_context();
        let names = service.name_compaction(&context).await.expect("the context processes");
        let mut entity = entity(&["http://example.org/test/testedAt", "http://example.org/common/isParked"]);

        apply_simplified_names(&mut entity, &names).expect("the declared terms are NGSI-LD names");

        let rendered: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec!["testedAt", "isParked"]);
    }

    /// A name the `@context` does not declare stays fully qualified, and a name
    /// the document compaction already shortened is left exactly as it is.
    #[tokio::test]
    async fn a_name_no_term_declares_is_left_as_the_compaction_produced_it() {
        let service = service();
        let context = coercing_context();
        let names = service.name_compaction(&context).await.expect("the context processes");
        let mut entity = entity(&["http://example.org/parking/status", "brandName"]);

        apply_simplified_names(&mut entity, &names).expect("no declared term is ungrammatical");

        let rendered: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec!["http://example.org/parking/status", "brandName"]);
    }

    /// The order the Attributes were rendered in is what a client reads them
    /// back in, so renaming keeps it.
    #[tokio::test]
    async fn renaming_keeps_the_order_the_attributes_were_rendered_in() {
        let service = service();
        let context = coercing_context();
        let names = service.name_compaction(&context).await.expect("the context processes");
        let mut entity = entity(&["brandName", "http://example.org/test/testedAt", "http://example.org/parking/status"]);

        apply_simplified_names(&mut entity, &names).expect("the declared terms are NGSI-LD names");

        let rendered: Vec<&str> = entity.attrs.keys().map(AttributeName::as_str).collect();
        pretty_assertions::assert_eq!(rendered, vec!["brandName", "testedAt", "http://example.org/parking/status"]);
    }
}
