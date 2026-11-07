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
//!
//! The `id` every refusal of an item is reported against follows the same
//! rule. An item writing it as `id` or `@id` names it under any context, so it
//! is known before the context loads; an item writing neither can only name it
//! under a term its own context defines for `@id`, which is read once that
//! context was admitted. Only an item naming no `id` under its context either
//! leaves a clause 5.2.17 `BatchEntityError` nothing to name, and fails the
//! request.

use crate::{
    batch_body_refusal::BatchBodyRefusal,
    batch_item_refusal::BatchItemRefusal,
    body_envelope::BodyEnvelope,
    not_json_document::NotJsonDocument,
    request_decode_bounds::{RequestDecodeBounds, read_within},
    undecodable_body::UndecodableBody,
};
use andromeda_jsonld::batch_contexts::{AdmittedBatchContext, BatchContext};
use andromeda_ngsi_ld_lexicon::{identifier_length::IdentifierTooLong, uri::Uri};
use andromeda_ngsi_ld_model::{attribute::member_reading::MemberReading, entity::Entity};
use andromeda_ngsi_ld_problem_details::batch_operation::BatchEntityError;
use andromeda_ngsi_ld_representation::{compact_node::context_reading::ContextReading, embedded_context_admission::EmbeddedContext};
use axum::body::Bytes;

/// The `id` a batch item is reported against.
#[derive(Debug)]
pub enum ItemIdentifier {
    /// The `id` the item wrote as `id` or `@id`, which every context reads as
    /// the `id` (clause 4.4).
    Written(Uri),
    /// The item wrote neither, so its `id` is a term its context defines for
    /// `@id`, read once that context was admitted.
    Aliased {
        /// The item's zero-based position in the array, which the refusal of
        /// an item naming no `id` names.
        index: usize,
        /// The refusal the item drew before its `id` was known, reported
        /// against that `id`.
        held: Option<BatchItemRefusal>,
    },
}

/// One batch item that passed the batch rules, with the context it is read
/// against.
///
/// Items naming one and the same context share one [`BatchContext`],
/// identified once when the batch was parsed, so a batch whose items agree
/// resolves its context once.
#[derive(Debug)]
pub struct BatchItem {
    /// The `id` every refusal of the item is reported against.
    pub identifier: ItemIdentifier,
    /// The text of the item, its `@context` member included, which the decode
    /// skips.
    pub text: Bytes,
    /// The context the item is read against.
    pub context: BatchContext,
    /// The first `@context` the item embedded below its own top level (clause
    /// 5.5.7), which is the item's own refusal: several items share one
    /// [`BatchContext`], so the finding never travels with it.
    pub embedded: Option<EmbeddedContext>,
    /// The most positions a geometry of the item may hold, when the stack
    /// published it.
    pub decode_bounds: Option<RequestDecodeBounds>,
}

/// Why a batch item whose context was admitted is not read.
#[derive(Debug)]
pub enum UnreadItem {
    /// The item is refused against its `id`, and the rest of the batch goes
    /// on.
    Refused(Box<BatchEntityError>),
    /// The item names no URI `id` under its context either, which fails the
    /// request as a whole.
    Unidentified(BatchBodyRefusal),
}

impl BatchItem {
    /// The `id` of an item whose context was refused, which an item knows
    /// only when it wrote it as `id` or `@id`.
    ///
    /// # Errors
    ///
    /// Returns [`BatchBodyRefusal::MissingId`] for an item that wrote neither:
    /// no context defines a term for `@id` it could be read under.
    pub fn written_id(self) -> Result<Uri, BatchBodyRefusal> {
        match self.identifier {
            ItemIdentifier::Written(entity_id) => Ok(entity_id),
            ItemIdentifier::Aliased { index, .. } => Err(BatchBodyRefusal::MissingId { index }),
        }
    }

    /// The item, read under `context`, which admitted the context it names.
    ///
    /// # Errors
    ///
    /// Returns [`UnreadItem::Unidentified`] for an item whose `id` is under no
    /// term `context` reads as `id`, or is no URI, and [`UnreadItem::Refused`]
    /// for one whose `id` read under `context` is too long to be stored, or
    /// that drew a refusal before its `id` was known.
    pub fn admitted(self, context: AdmittedBatchContext) -> Result<AdmittedItem, UnreadItem> {
        let entity_id = match self.identifier {
            ItemIdentifier::Written(entity_id) => entity_id,
            ItemIdentifier::Aliased { index, held } => {
                let entity_id = aliased_id(&self.text, index, context.terms().reading()).map_err(UnreadItem::Unidentified)?;
                let refused = IdentifierTooLong::check(entity_id.as_str()).map_err(BatchItemRefusal::from).err().or(held);
                if let Some(refusal) = refused {
                    return Err(UnreadItem::Refused(Box::new(refusal.against(entity_id))));
                }
                entity_id
            }
        };
        Ok(AdmittedItem {
            entity_id,
            text: self.text,
            context,
            decode_bounds: self.decode_bounds,
        })
    }
}

/// The `id` the item read as `envelope`, at `index` in the array, writes as
/// `id` or under a term its reader took for `id`, or `None` for an object
/// writing none.
///
/// # Errors
///
/// Returns the [`BatchBodyRefusal`] for an item that has no URI to stand in a
/// clause 5.2.17 `entityId`: a null item (clauses 5.6.7.4 to 5.6.10.4), an
/// item that is no object, or an `id` that is not a URI (Tables 5.2.4-1 and
/// 5.2.17-1). A URI of any length is returned: one too long to be stored is
/// still one an error can be reported against.
pub(crate) fn written_identifier(envelope: &BodyEnvelope, index: usize) -> Result<Option<Uri>, BatchBodyRefusal> {
    match envelope {
        BodyEnvelope::Null => Err(BatchBodyRefusal::NullItem { index }),
        BodyEnvelope::Object(object) => object
            .id()
            .map(|written| written.parse::<Uri>().map_err(|source| BatchBodyRefusal::MalformedId { index, source }))
            .transpose(),
        BodyEnvelope::Array(_) | BodyEnvelope::Scalar => Err(BatchBodyRefusal::MissingId { index }),
    }
}

/// The `id` the item `text`, at `index` in the array, writes under a term
/// `reading` reads as `id`.
///
/// # Errors
///
/// As [`written_identifier`], and [`BatchBodyRefusal::MissingId`] for an item
/// writing its `id` under no such term.
fn aliased_id(text: &[u8], index: usize, reading: ContextReading<'_>) -> Result<Uri, BatchBodyRefusal> {
    let envelope = BodyEnvelope::read_under(text, reading).map_err(|source| BatchBodyRefusal::NotJson(NotJsonDocument(source)))?;
    written_identifier(&envelope, index)?.ok_or(BatchBodyRefusal::MissingId { index })
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
    /// The most positions a geometry of the item may hold, when the stack
    /// published it.
    decode_bounds: Option<RequestDecodeBounds>,
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

    /// The `id` the item carried, which a refusal of it before it is decoded
    /// is reported against.
    #[must_use]
    pub fn into_entity_id(self) -> Uri {
        self.entity_id
    }

    /// Decode the item as an entity through the terms of its context.
    #[must_use]
    pub fn decoded<M: MemberReading>(self) -> DecodedItem<M> {
        let entity = entity_of(&self.text, self.context.terms().reading(), self.decode_bounds);
        DecodedItem {
            entity_id: self.entity_id,
            context: self.context,
            entity,
        }
    }
}

/// The entity the item `text` is, read through `reading`, its geometries
/// recorded within `decode_bounds`.
///
/// # Errors
///
/// Returns the refusal of an item that is no entity, naming the member it
/// failed at.
pub(crate) fn entity_of<M: MemberReading>(
    text: &[u8],
    reading: ContextReading<'_>,
    decode_bounds: Option<RequestDecodeBounds>,
) -> Result<Entity<M>, BatchItemRefusal> {
    read_within::<Entity<M>>(decode_bounds, text, reading)
        .map_err(|source| BatchItemRefusal::from(UndecodableBody::of_text::<Entity<M>>(text, reading, source)))
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
    use crate::{
        batch_body_refusal::BatchBodyRefusal,
        batch_item_decoding::{AdmittedItem, BatchItem, ItemIdentifier, UnreadItem},
        batch_item_refusal::BatchItemRefusal,
    };
    use andromeda_json_literal::JsonLiteral;
    use andromeda_jsonld::{batch_contexts::BatchContexts, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::identifier_length::IDENTIFIER_BYTE_LIMIT;
    use andromeda_ngsi_ld_model::attribute::instance::AttributeValue;
    use axum::body::Bytes;

    /// The item `text` of an `application/ld+json` batch, identified as
    /// `identifier`, read under the inline `context` it names.
    async fn admitted(identifier: ItemIdentifier, text: String, context: &str) -> Result<AdmittedItem, UnreadItem> {
        let own = BatchContexts::per_entity().context_of(Some(JsonLiteral::admit(context).expect("an admissible context")));
        let engine = JsonLdEngine::builder().allow_remote(false).build().expect("the offline engine builds");
        let admitted = own.admit_for_request(&engine).await.expect("the context is admitted");
        let item = BatchItem {
            identifier,
            text: Bytes::from(text),
            context: own,
            embedded: None,
            decode_bounds: None,
        };
        item.admitted(admitted)
    }

    /// The `id` of the item `text` of an `application/ld+json` batch, read
    /// under the inline `context` it names, or the refusal of it.
    async fn entity_id(text: &'static str, context: &str) -> Result<String, String> {
        let written = ItemIdentifier::Written("urn:ngsi-ld:Vehicle:0".parse().expect("a valid URI"));
        let decoded = admitted(written, text.to_owned(), context)
            .await
            .expect("the item is read")
            .decoded::<AttributeValue>();
        decoded.entity.map(|entity| entity.id.to_string()).map_err(|refusal| refusal.to_string())
    }

    /// An item at position 1 that wrote its `id` under neither spelling,
    /// holding `held`.
    const fn aliased(held: Option<BatchItemRefusal>) -> ItemIdentifier {
        ItemIdentifier::Aliased { index: 1, held }
    }

    /// Clause 4.4 and JSON-LD 1.1 section 4.1.3: a term the item's own context
    /// defines for `@id` names its `id`, which the item is decoded and reported
    /// under.
    #[tokio::test]
    async fn an_id_written_under_an_alias_of_its_own_context_is_the_items_id() {
        let text = r#"{"eid": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "@context": {"eid": "@id"}}"#;
        let item = admitted(aliased(None), text.to_owned(), r#"{"eid": "@id"}"#).await.expect("the item is read");
        let decoded = item.decoded::<AttributeValue>();
        pretty_assertions::assert_eq!(decoded.entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
        pretty_assertions::assert_eq!(decoded.entity.expect("the item is an entity").id.to_string(), "urn:ngsi-ld:Vehicle:2");
    }

    /// Clause 5.6.7.4: a refusal drawn before the `id` was known is reported
    /// against the `id` the context reads.
    #[tokio::test]
    async fn a_held_refusal_is_reported_against_the_aliased_id() {
        let text = r#"{"eid": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "@context": {"eid": "@id"}}"#;
        let refusal = admitted(aliased(Some(BatchItemRefusal::NoContextInLdJson)), text.to_owned(), r#"{"eid": "@id"}"#).await;
        let Err(UnreadItem::Refused(error)) = refusal else {
            panic!("the item is refused against its id: {refusal:?}");
        };
        pretty_assertions::assert_eq!(error.entity_id.as_str(), "urn:ngsi-ld:Vehicle:2");
        pretty_assertions::assert_eq!(error.error.detail, "an application/ld+json batch item must contain @context");
    }

    /// Table 5.2.17-1 sets no length on `entityId`: an aliased `id` too long
    /// to be stored is still the one its refusal names.
    #[tokio::test]
    async fn an_aliased_id_too_long_to_be_stored_is_refused_against_itself() {
        let long_id = format!("urn:ngsi-ld:Vehicle:{}", "x".repeat(IDENTIFIER_BYTE_LIMIT));
        let text = format!(r#"{{"eid": "{long_id}", "type": "Vehicle", "@context": {{"eid": "@id"}}}}"#);
        let refusal = admitted(aliased(None), text, r#"{"eid": "@id"}"#).await;
        let Err(UnreadItem::Refused(error)) = refusal else {
            panic!("the item is refused against its id: {refusal:?}");
        };
        pretty_assertions::assert_eq!(error.entity_id.as_str(), long_id.as_str());
    }

    /// Table 5.2.4-1: an item naming no `id` under its context either has
    /// nothing to be reported against, and fails the request.
    #[tokio::test]
    async fn an_item_naming_no_id_under_its_context_is_unidentified() {
        for (text, context) in [
            (r#"{"eid": "urn:ngsi-ld:Vehicle:2", "type": "Vehicle", "@context": {}}"#, "{}"),
            (r#"{"eid": 7, "type": "Vehicle", "@context": {"eid": "@id"}}"#, r#"{"eid": "@id"}"#),
        ] {
            let refusal = admitted(aliased(None), text.to_owned(), context).await;
            assert!(
                matches!(refusal, Err(UnreadItem::Unidentified(BatchBodyRefusal::MissingId { index: 1 }))),
                "{text}: {refusal:?}"
            );
        }
        let malformed = r#"{"eid": "not a uri", "type": "Vehicle", "@context": {"eid": "@id"}}"#;
        let refusal = admitted(aliased(None), malformed.to_owned(), r#"{"eid": "@id"}"#).await;
        assert!(
            matches!(refusal, Err(UnreadItem::Unidentified(BatchBodyRefusal::MalformedId { index: 1, .. }))),
            "{refusal:?}"
        );
    }

    /// Clause 5.6.7.5: an item whose context was refused is reported against
    /// the `id` it wrote as `id` or `@id`, and fails the request without one.
    #[test]
    fn only_a_written_id_survives_a_refused_context() {
        let item = |identifier| BatchItem {
            identifier,
            text: Bytes::from_static(b"{}"),
            context: BatchContexts::per_entity().context_of(None),
            embedded: None,
            decode_bounds: None,
        };
        let written = item(ItemIdentifier::Written("urn:ngsi-ld:Vehicle:1".parse().expect("a valid URI")));
        pretty_assertions::assert_eq!(written.written_id().expect("a written id").as_str(), "urn:ngsi-ld:Vehicle:1");
        assert!(matches!(item(aliased(None)).written_id(), Err(BatchBodyRefusal::MissingId { index: 1 })));
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
