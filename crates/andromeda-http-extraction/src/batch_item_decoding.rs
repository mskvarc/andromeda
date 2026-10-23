//! One item of an entity batch, held as the text it was written as until the
//! context it names is admitted, and decoded under that context.
//!
//! Clause 5.5.4 judges an item "according to the @context", and the context
//! of an item is its own `@context` member in an `application/ld+json` batch
//! (clause 5.5.7: "the @context [...] provided by each API call") or the
//! request's `Link` context in an `application/json` one. That context decides
//! more than whether the item is refused: a member written `@id` or `@type`
//! (clause 4.4), or under a term the context defines for a core term, is the
//! core term it stands for. The item is therefore decoded once its context was
//! admitted, through the aliases of that context.

use crate::{batch_item_refusal::BatchItemRefusal, undecodable_body::UndecodableBody};
use andromeda_jsonld::batch_contexts::{AdmittedBatchContext, BatchContext};
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::{attribute::member_reading::MemberReading, entity::Entity, request_body::RequestBody};
use andromeda_ngsi_ld_representation::{core_term_aliases::CoreTermAliases, embedded_context_admission::EmbeddedContext};
use axum::body::Bytes;

/// One batch item that passed the batch rules, with the context it is read
/// against.
///
/// Items naming one and the same context share one [`BatchContext`],
/// identified once when the batch was parsed, so a batch whose items agree
/// resolves its context once.
#[derive(Debug)]
pub struct BatchItem {
    /// The `id` the item carried, which every refusal of it before it is
    /// decoded is reported against.
    pub entity_id: Uri,
    /// The text of the item, its `@context` member included, which the decode
    /// skips.
    pub text: Bytes,
    /// The context the item is read against.
    pub context: BatchContext,
    /// The first `@context` the item embedded below its own top level (clause
    /// 5.5.7), which is the item's own refusal: several items share one
    /// [`BatchContext`], so the finding never travels with it.
    pub embedded: Option<EmbeddedContext>,
}

impl BatchItem {
    /// The item, read under `context`, which admitted the context it names.
    #[must_use]
    pub fn admitted(self, context: AdmittedBatchContext) -> AdmittedItem {
        AdmittedItem {
            entity_id: self.entity_id,
            text: self.text,
            context,
        }
    }
}

/// One batch item whose context was admitted, not yet decoded.
#[derive(Debug)]
pub struct AdmittedItem {
    /// The `id` the item carried.
    entity_id: Uri,
    /// The text of the item.
    text: Bytes,
    /// The context the item is read under.
    context: AdmittedBatchContext,
}

impl AdmittedItem {
    /// How many bytes the item is written in, which the decode costs in
    /// proportion to.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.text.len()
    }

    /// Whether the item is written in no byte, which an item of a JSON array
    /// never is.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    /// Decode the item as an entity through the aliases of its context.
    #[must_use]
    pub fn decoded<M: MemberReading>(self) -> DecodedItem<M> {
        let entity = entity_of(&self.text, self.context.aliases());
        DecodedItem {
            entity_id: self.entity_id,
            context: self.context,
            entity,
        }
    }
}

/// The entity the item `text` is, read through `aliases`.
///
/// # Errors
///
/// Returns the refusal of an item that is no entity, naming the member it
/// failed at.
pub(crate) fn entity_of<M: MemberReading>(text: &[u8], aliases: &CoreTermAliases) -> Result<Entity<M>, BatchItemRefusal> {
    Entity::<M>::from_request_text(text, aliases).map_err(|source| BatchItemRefusal::from(UndecodableBody::of_text::<Entity<M>>(text, aliases, source)))
}

/// One batch item decoded under the context it names.
#[derive(Debug)]
pub struct DecodedItem<M> {
    /// The `id` the item carried, which the refusal of an item that is no
    /// entity is reported against.
    pub entity_id: Uri,
    /// The context the item was read under.
    pub context: AdmittedBatchContext,
    /// The entity, or the refusal of an item that is no entity.
    pub entity: Result<Entity<M>, BatchItemRefusal>,
}

#[cfg(test)]
mod tests {
    use crate::batch_item_decoding::BatchItem;
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{batch_contexts::BatchContexts, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_model::attribute::instance::AttributeValue;
    use axum::body::Bytes;

    /// The `id` of the item `text` of an `application/ld+json` batch, read
    /// under the inline `context` it names, or the refusal of it.
    async fn entity_id(text: &'static str, context: &str) -> Result<String, String> {
        let own = BatchContexts::per_entity().context_of(Some(JsonLiteral::admit(context).expect("an admissible context")));
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
        let admitted = own.admit_for_request(&engine).await.expect("the context is admitted");
        let item = BatchItem {
            entity_id: "urn:ngsi-ld:Vehicle:0".parse().expect("a valid URI"),
            text: Bytes::from_static(text.as_bytes()),
            context: own,
            embedded: None,
        };
        let decoded = item.admitted(admitted).decoded::<AttributeValue>();
        decoded.entity.map(|entity| entity.id.to_string()).map_err(|refusal| refusal.to_string())
    }

    /// Clauses 4.4 and 5.5.7: an item is read under its own context, its
    /// keyword spellings and the aliases the context defines read as the core
    /// terms.
    #[tokio::test]
    async fn a_batch_item_spelled_with_keywords_or_aliases_is_decoded() {
        pretty_assertions::assert_eq!(
            entity_id(r#"{"@id": "urn:ngsi-ld:Vehicle:1", "@type": "Vehicle", "@context": {}}"#, "{}").await,
            Ok("urn:ngsi-ld:Vehicle:1".to_owned())
        );
        pretty_assertions::assert_eq!(
            entity_id(
                r#"{"id": "urn:ngsi-ld:Vehicle:2", "kind": "Vehicle", "speed": {"kind": "Property", "v": 5}, "@context": {"kind": "@type", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}}"#,
                r#"{"kind": "@type", "v": "https://uri.etsi.org/ngsi-ld/hasValue"}"#,
            )
            .await,
            Ok("urn:ngsi-ld:Vehicle:2".to_owned())
        );
    }

    /// Without the context that aliases it, an aliased member is an
    /// attribute the item cannot be read as.
    #[tokio::test]
    async fn an_alias_the_context_does_not_define_is_refused() {
        assert!(
            entity_id(r#"{"id": "urn:ngsi-ld:Vehicle:3", "kind": "Vehicle", "@context": {}}"#, "{}")
                .await
                .is_err(),
            "an entity without a type is refused"
        );
    }
}
