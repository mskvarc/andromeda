//! The clause 5.5.4 screening a written payload passes before the operation
//! does anything else with it.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.4 requires every operation taking a
//! JSON-LD document as input to raise `BadRequestData` when "the data included
//! by the JSON-LD document is not syntactically correct, according to the
//! @context or the API data type definitions", and every write clause states it
//! as its first bullet: clause 5.6.1.4 opens with "Execute the behaviour
//! defined in clause 5.5.4 on JSON-LD validation" and only then forwards
//! matching input data to Context Sources, as do clauses 5.6.2.4, 5.6.3.4,
//! 5.6.7.4 to 5.6.9.4, 5.6.17.4, 5.6.18.4, 5.6.19.4 and 5.6.20.4. Two things
//! follow from that order, and both are what this module exists for:
//!
//! - a representation this broker refuses never reaches a Context Source;
//! - an operation whose local part is left with nothing to do -- because an
//!   exclusive or redirect registration claimed every attribute -- has still
//!   run the validation, so it cannot answer 201 or 204 to a payload clause
//!   5.5.4 refuses.
//!
//! The rules split by the representation they are decidable on, and one
//! admission runs both halves so a rule added later cannot land on one half
//! only:
//!
//! - Two are decidable only on the compact payload a client wrote, before
//!   JSON-LD expansion rewrites the member names, and
//!   [`validate_fragment_attribute_types`] holds both: the attribute types of
//!   clause 5.2.4, and the rule that `entityIdSealed` and `entityTypeSealed`
//!   belong to the `ngsildproof` Property of clause 4.5.2.2 alone. Clauses
//!   4.5.3.2 and 4.5.3.3 sharpen the second one for a Relationship, normalized
//!   and concise alike, into "'entityIdSealed' and 'entityTypeSealed' shall
//!   never be present" -- an unconditional prohibition, since the `ngsildproof`
//!   exception is a Property's.
//! - A third is decidable on the compact payload for a different reason: the
//!   `@context` clause 5.5.7 admits only at the top level of an NGSI-LD
//!   document is lifted out of the payload by the transport layer, so every one
//!   still standing in what arrives here is embedded.
//!   [`reject_embedded_contexts_in_fragment`] holds it. Its companion rule --
//!   that the user `@context` declares no JSON-LD Scoped Context -- reads the
//!   request's contexts rather than its payload and needs them resolved, so
//!   `andromeda_entity::write_admission` owns it beside the expanded rules.
//! - The rest read the expanded payload -- where an NGSI-LD Null may stand,
//!   what the `scope` member of clause 4.18 may hold, what an attribute
//!   instance's envelope may carry, and whether a `GeoProperty` geometry is
//!   well formed -- and `andromeda_entity::write_admission` owns them, because
//!   which of them an operation is exempt from is decided by that operation's
//!   own clause.
//!
//! Only the report differs between the two shapes of write. Clause 5.6.1
//! answers a single write with a problem details body, while clauses 5.6.7.4
//! to 5.6.9.4 and 5.6.20.4 run the single-entity behaviour "for each of the
//! NGSI-LD Entities included in the input Array" and add "a new
//! `BatchEntityError` containing the failed Entity ID and the related
//! `ProblemDetails`" for each one that fails, leaving the rest of the batch
//! intact. The screening is therefore run once here and projected twice.
//!
//! What the screening expanded is what the local write stores: each admission
//! hands back the admitted payload `andromeda_entity` defines, and the write
//! path consumes it rather than expanding the payload again. A payload the
//! extractor decoded into its typed compact form is screened and expanded from
//! that form, never rendered back into JSON for either.

use crate::{
    compact_screening::{CompactRefusal, screen_compact_entity, screen_compact_patch},
    error::EntityHttpClassify,
};
use andromeda_entity::{
    EntityService,
    admitted_write::{AdmittedAttribute, AdmittedEntity, WritePayload},
    batch::admitted_batch::{AdmittedBatch, SubmittedEntity},
    error::EntityError,
    write_kind::{AttributeWriteKind, BatchWriteKind, EntityWriteKind},
};
use andromeda_http_extraction::entity_batch::BatchItem;
use andromeda_http_response::{batch_projection::batch_entity_error, classified_report::ClassifiedReport};
use andromeda_jsonld::{batch_contexts::BatchContextId, context_spec::ContextSpec};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::{validate_attribute_node, validate_fragment_attribute_types},
    embedded_context_admission::{reject_embedded_contexts_in_attribute, reject_embedded_contexts_in_fragment},
};
use serde_json::Value;
use thiserror::Error;

/// Why a payload a client wrote is not admitted.
#[derive(Debug, Error)]
pub(crate) enum WrittenPayloadRefusal {
    /// The compact payload breaks clause 5.2.4, 4.5.2.2 or 5.5.7.
    #[error(transparent)]
    Compact(#[from] CompactRefusal),
    /// The expanded payload puts an NGSI-LD Null, a `scope`, an attribute
    /// envelope or a geometry where the operation does not admit it.
    #[error(transparent)]
    Placement(#[from] EntityError),
}

impl EntityHttpClassify for WrittenPayloadRefusal {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        match self {
            // Clause 5.5.4: data that is not syntactically correct according to
            // the API data type definitions is BadRequestData, and clause 5.5.7
            // gives a part of the user @context outside the document's top
            // level the same type.
            WrittenPayloadRefusal::Compact(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            WrittenPayloadRefusal::Placement(source) => source.classify_for_entity_http(),
        }
    }
}

/// An entity-shaped payload as the transport decoded it, which the compact
/// half of the screening reads before the expanded half runs.
pub(crate) trait WrittenEntityPayload: WritePayload {
    /// Screen the payload under the rules decidable only on its compact form.
    ///
    /// # Errors
    ///
    /// Returns the first refusal of clause 5.2.4, 4.5.2.2 or 5.5.7.
    fn screen(&self) -> Result<(), CompactRefusal>;
}

impl WrittenEntityPayload for Value {
    fn screen(&self) -> Result<(), CompactRefusal> {
        validate_fragment_attribute_types(self)?;
        reject_embedded_contexts_in_fragment(self)?;
        Ok(())
    }
}

impl WrittenEntityPayload for &Entity {
    fn screen(&self) -> Result<(), CompactRefusal> {
        screen_compact_entity(self)
    }
}

impl WrittenEntityPayload for &EntityPatch {
    fn screen(&self) -> Result<(), CompactRefusal> {
        screen_compact_patch(self)
    }
}

/// Screen one entity-shaped payload -- a whole Entity or an entity-shaped
/// Fragment -- for the write `W` before it is forwarded or stored.
///
/// # Errors
///
/// Returns the first attribute node clauses 5.2.4 and 4.5.2.2 do not admit, and
/// the first placement clause 5.5.4 refuses for `W`.
pub(crate) async fn admit_written_entity<W: EntityWriteKind>(
    service: &EntityService,
    payload: impl WrittenEntityPayload,
    context: &ContextSpec,
) -> Result<AdmittedEntity<W>, WrittenPayloadRefusal> {
    payload.screen()?;
    Ok(service.admit_entity_write(payload, context).await?)
}

/// Screen the bare Attribute Fragment of clauses 5.6.4 and 5.6.19 written to
/// `attribute` for the write `W` before it is forwarded or stored.
///
/// # Errors
///
/// Returns the first refusal on the same terms as [`admit_written_entity`].
pub(crate) async fn admit_written_attribute<'a, W: AttributeWriteKind>(
    service: &EntityService,
    attribute: &'a AttributeName,
    payload: Value,
    context: &ContextSpec,
) -> Result<AdmittedAttribute<'a, W>, WrittenPayloadRefusal> {
    validate_attribute_node(attribute.as_str(), &payload).map_err(CompactRefusal::from)?;
    reject_embedded_contexts_in_attribute(attribute.as_str(), &payload).map_err(CompactRefusal::from)?;
    Ok(service.admit_attribute_write(attribute, payload, context).await?)
}

/// Keep the batch entities the write rules admit, reporting each refused one
/// against its own identifier.
///
/// The refusals join the errors the batch has already collected, so the
/// clause 5.2.16 result carries them beside the items that failed to decode.
/// Screening runs before the forwarding phase of clauses 5.6.7.4 to 5.6.9.4
/// and 5.6.20.4: a representation this broker refuses is not handed to a
/// context source either.
///
/// # Errors
///
/// Returns the one refusal that is the request's rather than an entity's: a
/// user `@context` clause 5.5.7 does not admit (a JSON-LD Scoped Context) is a
/// fault of the request, so it fails the operation instead of being projected
/// onto the entities naming it. An `@context` that could not be retrieved is
/// the other outcome, and it is not returned as an error at all: clause
/// 5.6.7.5 requires the batch to answer 207 with a per-entity `BatchEntityError`
/// for it, so the refusal is projected onto every entity naming that context,
/// and the entities naming another one proceed.
pub(crate) async fn admitted_batch_entities<W: BatchWriteKind>(
    service: &EntityService,
    kind: W,
    items: Vec<BatchItem>,
    refused: &mut Vec<BatchEntityError>,
) -> Result<AdmittedBatch<W>, WrittenPayloadRefusal> {
    let submitted = screened_batch_entities(async |context: &ContextSpec| service.admit_write_context(context).await, items, refused).await?;
    let (admitted, placements) = service.admit_batch_write(kind, submitted).await;
    for (id, error) in placements {
        refused.push(batch_entity_error(
            id.into(),
            &WrittenPayloadRefusal::Placement(error),
            EntityHttpClassify::classify_for_entity_http,
        ));
    }
    Ok(admitted)
}

/// The compact screening of a batch's entities, each distinct context they
/// name admitted once through `admit_context`.
///
/// # Errors
///
/// As [`admitted_batch_entities`]: a context refused for anything but being
/// unretrievable fails the operation.
async fn screened_batch_entities(
    mut admit_context: impl AsyncFnMut(&ContextSpec) -> Result<(), EntityError>,
    items: Vec<BatchItem>,
    refused: &mut Vec<BatchEntityError>,
) -> Result<Vec<SubmittedEntity>, WrittenPayloadRefusal> {
    let mut decided: Vec<(BatchContextId, ContextAdmission)> = Vec::new();
    let mut submitted = Vec::with_capacity(items.len());
    for BatchItem { entity, context } in items {
        let position = if let Some(position) = decided.iter().position(|(seen, _)| *seen == context.id()) {
            position
        } else {
            let admission = context_admission(admit_context(context.spec()).await)?;
            decided.push((context.id(), admission));
            decided.len() - 1
        };
        if let Some((_, ContextAdmission::Unavailable(refusal))) = decided.get(position) {
            refused.push(batch_entity_error(entity.id.into(), refusal, EntityHttpClassify::classify_for_entity_http));
            continue;
        }
        match screen_compact_entity(&entity) {
            Ok(()) => submitted.push(SubmittedEntity { entity, context }),
            Err(refusal) => refused.push(batch_entity_error(
                entity.id.into(),
                &WrittenPayloadRefusal::Compact(refusal),
                EntityHttpClassify::classify_for_entity_http,
            )),
        }
    }
    Ok(submitted)
}

/// What the clause 5.5.7 admission of one context a batch names decided.
enum ContextAdmission {
    /// The context resolved and declares nothing clause 5.5.7 refuses.
    Admitted,
    /// The context could not be retrieved (clause 5.5.6), which every entity
    /// naming it is answered with.
    Unavailable(WrittenPayloadRefusal),
}

/// What the admission of one context a batch names decided, from its outcome.
///
/// # Errors
///
/// Returns the refusal that fails the whole operation: anything but an
/// unretrievable context, which is the entities' failure rather than the
/// request's (clause 5.6.7.5).
fn context_admission(outcome: Result<(), EntityError>) -> Result<ContextAdmission, WrittenPayloadRefusal> {
    match outcome {
        Ok(()) => Ok(ContextAdmission::Admitted),
        Err(error) => {
            let refusal = WrittenPayloadRefusal::Placement(error);
            if refusal.classify_for_entity_http().error_type() == NgsiLdErrorType::LdContextNotAvailable {
                Ok(ContextAdmission::Unavailable(refusal))
            } else {
                Err(refusal)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        compact_screening::{CompactRefusal, screen_compact_entity},
        written_payload_admission::{WrittenPayloadRefusal, screened_batch_entities},
    };
    use andromeda_entity::error::EntityError;
    use andromeda_http_extraction::entity_batch::BatchItem;
    use andromeda_jsonld::{batch_contexts::BatchContexts, context_spec::ContextSpec, context_term::ContextTerm, error::JsonLdError};
    use andromeda_ngsi_ld_model::entity::Entity;
    use andromeda_ngsi_ld_representation::attribute_type_validation::AttributeTypeError;
    use iri_rs::IriBuf;
    use serde_json::{Value, json};

    /// A context the fake admission cannot retrieve.
    const UNRETRIEVABLE: &str = "https://contexts.example/unretrievable.jsonld";

    /// A context the fake admission finds to declare a scoped context.
    const SCOPED: &str = "https://contexts.example/scoped.jsonld";

    /// A context the fake admission admits.
    const ADMITTED: &str = "https://contexts.example/admitted.jsonld";

    /// Batch items, each an entity `id` read under the context URL it wrote.
    fn items(entities: &[(&str, &str)]) -> Vec<BatchItem> {
        let mut contexts = BatchContexts::per_entity();
        entities
            .iter()
            .map(|(id, context)| BatchItem {
                entity: entity(&json!({ "id": id, "type": "Building" })),
                context: contexts.context_of(Some(Value::from(*context))),
            })
            .collect()
    }

    /// The admission the fake decides for a context naming `url`.
    fn admission_of(context: &ContextSpec) -> Result<(), EntityError> {
        let named = |url: &str| *context == ContextSpec::from_body(Value::from(url));
        if named(UNRETRIEVABLE) {
            return Err(EntityError::JsonLd(JsonLdError::ContextFetchDisabled {
                url: IriBuf::new(UNRETRIEVABLE.to_owned()).expect("a valid IRI"),
            }));
        }
        if named(SCOPED) {
            return Err(EntityError::JsonLd(JsonLdError::ScopedUserContext {
                term: ContextTerm::new("speed"),
            }));
        }
        Ok(())
    }

    /// Clause 5.6.7.5: an unretrievable context is the failure of the entities
    /// naming it, answered per entity; the entities naming another proceed.
    #[tokio::test]
    async fn only_the_entities_naming_an_unretrievable_context_are_refused() {
        let mut refused = Vec::new();
        let rendered = screened_batch_entities(
            async |context: &ContextSpec| admission_of(context),
            items(&[
                ("urn:ngsi-ld:Building:a", ADMITTED),
                ("urn:ngsi-ld:Building:b", UNRETRIEVABLE),
                ("urn:ngsi-ld:Building:c", ADMITTED),
                ("urn:ngsi-ld:Building:d", UNRETRIEVABLE),
            ]),
            &mut refused,
        )
        .await
        .expect("an unretrievable context does not fail the operation");

        let admitted: Vec<String> = rendered.iter().map(|item| item.entity.id.to_string()).collect();
        pretty_assertions::assert_eq!(admitted, vec!["urn:ngsi-ld:Building:a", "urn:ngsi-ld:Building:c"]);
        let refused_ids: Vec<String> = refused.iter().map(|error| error.entity_id.to_string()).collect();
        pretty_assertions::assert_eq!(refused_ids, vec!["urn:ngsi-ld:Building:b", "urn:ngsi-ld:Building:d"]);
        let types: Vec<Value> = refused
            .iter()
            .filter_map(|error| serde_json::to_value(error).ok())
            .map(|error| error["error"]["type"].clone()) // clone: the type is compared after the error is dropped
            .collect();
        pretty_assertions::assert_eq!(types.len(), 2);
        assert!(
            types
                .iter()
                .all(|found| found.as_str().is_some_and(|uri| uri.ends_with("/LdContextNotAvailable"))),
            "{types:?}"
        );
    }

    /// Clause 5.5.7: a scoped user context is a fault of the request, so it
    /// fails the operation rather than being projected onto entities.
    #[tokio::test]
    async fn a_scoped_context_fails_the_operation() {
        let mut refused = Vec::new();
        let outcome = screened_batch_entities(
            async |context: &ContextSpec| admission_of(context),
            items(&[("urn:ngsi-ld:Building:a", ADMITTED), ("urn:ngsi-ld:Building:b", SCOPED)]),
            &mut refused,
        )
        .await;

        assert!(
            matches!(outcome, Err(WrittenPayloadRefusal::Placement(_))),
            "the scoped context fails the batch"
        );
    }

    /// Entities naming one and the same context have it admitted once.
    #[tokio::test]
    async fn a_shared_context_is_admitted_once() {
        let mut admissions = 0_usize;
        let mut refused = Vec::new();
        let rendered = screened_batch_entities(
            async |context: &ContextSpec| {
                admissions += 1;
                admission_of(context)
            },
            items(&[
                ("urn:ngsi-ld:Building:a", ADMITTED),
                ("urn:ngsi-ld:Building:b", ADMITTED),
                ("urn:ngsi-ld:Building:c", ADMITTED),
            ]),
            &mut refused,
        )
        .await
        .expect("the batch is admitted");

        pretty_assertions::assert_eq!(admissions, 1);
        pretty_assertions::assert_eq!(rendered.len(), 3);
        assert!(refused.is_empty(), "{refused:?}");
    }

    /// The entity a written payload decodes into, as the extractor produces it.
    fn entity(payload: &Value) -> Entity {
        serde_json::from_value(payload.clone()).expect("the payload decodes into an entity") // clone: `from_value` consumes the payload the test keeps
    }

    #[test]
    fn an_entity_whose_attributes_are_all_admissible_passes() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "temperature": {"type": "Property", "value": 21.5, "unitCode": "CEL"},
            "belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}}
        });
        assert!(screen_compact_entity(&entity(&payload)).is_ok());
    }

    /// Clause 4.5.3.2, "Prohibited": on a Relationship in the normalized
    /// representation, "'entityIdSealed' and 'entityTypeSealed' shall never be
    /// present".
    #[test]
    fn a_seal_on_a_normalized_relationship_is_refused() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let payload = json!({
                "id": "urn:ngsi-ld:Building:1",
                "type": "Building",
                "belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Building:1"}
            });
            let refusal = screen_compact_entity(&entity(&payload)).expect_err("a seal on a Relationship is refused");
            assert!(refusal.to_string().contains(member), "{member}: {refusal}");
        }
    }

    /// Clause 4.5.3.3 repeats the same prohibition for the concise
    /// representation, where the Relationship declares no `type` at all and its
    /// family is inferred from `object`.
    #[test]
    fn a_seal_on_a_concise_relationship_is_refused() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let payload = json!({
                "id": "urn:ngsi-ld:Building:1",
                "type": "Building",
                "belongsTo": {"object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Building:1"}
            });
            let refusal = screen_compact_entity(&entity(&payload)).expect_err("a seal on a concise Relationship is refused");
            assert!(refusal.to_string().contains(member), "{member}: {refusal}");
        }
    }

    /// Clause 4.5.2.2 defines `ngsildproof` as "a Property with the
    /// non-reified subproperties `entityIdSealed` and `entityTypeSealed`", so
    /// the pair is admitted there and only there.
    #[test]
    fn the_seals_of_a_genuine_ngsildproof_property_are_admitted() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "belongsTo": {
                "type": "Relationship",
                "object": "urn:ngsi-ld:Site:1",
                "ngsildproof": {
                    "type": "Property",
                    "value": {"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022"},
                    "entityIdSealed": "urn:ngsi-ld:Building:1",
                    "entityTypeSealed": "Building"
                }
            }
        });
        assert!(screen_compact_entity(&entity(&payload)).is_ok());
    }

    /// Clause 5.2.4: a node declaring a token outside the eight attribute
    /// types is not an attribute, whatever else it carries.
    #[test]
    fn an_attribute_declaring_a_token_outside_clause_5_2_4_is_refused() {
        let payload = json!({
            "id": "urn:ngsi-ld:Building:1",
            "type": "Building",
            "temperature": {"type": "NotAnAttributeType", "value": 21.5}
        });
        let refusal = screen_compact_entity(&entity(&payload)).expect_err("an unknown attribute type is refused");
        assert!(
            matches!(refusal, CompactRefusal::AttributeType(AttributeTypeError::NotAnAttributeType { .. })),
            "{refusal:?}"
        );
        assert!(refusal.to_string().contains("/temperature"), "{refusal}");
    }
}
