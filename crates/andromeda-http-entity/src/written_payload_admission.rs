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

use crate::error::EntityHttpClassify;
use andromeda_entity::{
    EntityService,
    error::EntityError,
    write_admission::{AttributeWrite, BatchWrite, EntityWrite},
};
use andromeda_http_response::{batch_projection::batch_entity_error, classified_report::ClassifiedReport, internal_failure_summary::InternalFailureSummary};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::Entity;
use andromeda_ngsi_ld_problem_details::{batch_operation::BatchEntityError, error_type::NgsiLdErrorType};
use andromeda_ngsi_ld_representation::{
    attribute_type_validation::{AttributeTypeError, validate_attribute_node, validate_fragment_attribute_types},
    embedded_context_admission::{EmbeddedContextError, reject_embedded_contexts_in_attribute, reject_embedded_contexts_in_fragment},
};
use serde_json::Value;
use thiserror::Error;

/// Why a payload a client wrote is not admitted.
#[derive(Debug, Error)]
pub(crate) enum WrittenPayloadRefusal {
    /// The decoded entity could not be rendered back into the compact
    /// representation the pre-expansion rules are stated over.
    #[error("the entity representation could not be rendered for validation: {0}")]
    Rendering(#[from] serde_json::Error),
    /// An attribute node is not one clauses 5.2.4 and 4.5.2.2 admit.
    #[error(transparent)]
    AttributeType(#[from] AttributeTypeError),
    /// A `@context` stands below the top level of the document (clause 5.5.7).
    #[error(transparent)]
    EmbeddedContext(#[from] EmbeddedContextError),
    /// The expanded payload puts an NGSI-LD Null, a `scope`, an attribute
    /// envelope or a geometry where the operation does not admit it.
    #[error(transparent)]
    Placement(#[from] EntityError),
}

impl EntityHttpClassify for WrittenPayloadRefusal {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        match self {
            // Clause 5.5.6: a failure of the broker's own is an InternalError,
            // logged at the disclosure boundary rather than handed to the client.
            WrittenPayloadRefusal::Rendering(source) => ClassifiedReport::internal(source, InternalFailureSummary::RESPONSE_RENDERING),
            // Clause 5.5.4: data that is not syntactically correct according to
            // the API data type definitions is BadRequestData.
            WrittenPayloadRefusal::AttributeType(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            // Clause 5.5.7: a part of the user @context outside the document's
            // top level "should result in an error of type BadRequestData".
            WrittenPayloadRefusal::EmbeddedContext(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            WrittenPayloadRefusal::Placement(source) => source.classify_for_entity_http(),
        }
    }
}

/// Screen one entity-shaped payload -- a whole Entity or an entity-shaped
/// Fragment -- before it is forwarded or stored.
///
/// # Errors
///
/// Returns the first attribute node clauses 5.2.4 and 4.5.2.2 do not admit, and
/// the first placement clause 5.5.4 refuses for `write`.
pub(crate) async fn admit_written_entity(
    service: &EntityService,
    write: EntityWrite,
    payload: &Value,
    context: &ContextSpec,
) -> Result<(), WrittenPayloadRefusal> {
    validate_fragment_attribute_types(payload)?;
    reject_embedded_contexts_in_fragment(payload)?;
    service.admit_entity_write(write, payload, context).await?;
    Ok(())
}

/// Screen the bare Attribute Fragment of clauses 5.6.4 and 5.6.19 before it is
/// forwarded or stored.
///
/// # Errors
///
/// Returns the first refusal on the same terms as [`admit_written_entity`].
pub(crate) async fn admit_written_attribute(
    service: &EntityService,
    write: AttributeWrite,
    attribute: &AttributeName,
    payload: &Value,
    context: &ContextSpec,
) -> Result<(), WrittenPayloadRefusal> {
    validate_attribute_node(attribute.as_str(), payload)?;
    reject_embedded_contexts_in_attribute(attribute.as_str(), payload)?;
    service.admit_attribute_write(write, attribute, payload, context).await?;
    Ok(())
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
/// user `@context` clause 5.5.7 does not admit (a JSON-LD Scoped Context)
/// belongs to no entity in the array, so it fails the operation instead of
/// being projected onto each of them. An `@context` that could not be retrieved
/// is the other outcome, and it is not returned as an error at all: clause
/// 5.6.7.5 requires the batch to answer 207 with a per-entity `BatchEntityError`
/// for it, so the one refusal is projected onto every submitted entity and an
/// empty admitted set is returned.
pub(crate) async fn admitted_batch_entities(
    service: &EntityService,
    write: BatchWrite,
    entities: Vec<Entity>,
    context: &ContextSpec,
    refused: &mut Vec<BatchEntityError>,
) -> Result<Vec<Entity>, WrittenPayloadRefusal> {
    if let Err(error) = service.admit_write_context(context).await {
        let refusal = WrittenPayloadRefusal::Placement(error);
        // Clause 5.5.6 makes an unretrievable user `@context` an
        // `LdContextNotAvailable`, and clause 5.6.7.5 answers a batch with a
        // per-entity `BatchEntityError` rather than failing the whole array.
        // The refusal is the same for every entity -- the `@context` they share
        // could not be resolved -- so it is projected onto each. A Scoped
        // Context (clause 5.5.7) classifies as `BadRequestData`, which is the
        // request's own fault and still fails the operation.
        if refusal.classify_for_entity_http().error_type() == NgsiLdErrorType::LdContextNotAvailable {
            for entity in entities {
                refused.push(batch_entity_error(entity.id, &refusal, EntityHttpClassify::classify_for_entity_http));
            }
            return Ok(Vec::new());
        }
        return Err(refusal);
    }
    let mut admitted = Vec::with_capacity(entities.len());
    for entity in entities {
        match render_and_validate(&entity) {
            Ok(()) => admitted.push(entity),
            Err(refusal) => refused.push(batch_entity_error(entity.id, &refusal, EntityHttpClassify::classify_for_entity_http)),
        }
    }
    let placements = service.admit_batch_write(write, &admitted, context).await;
    if placements.is_empty() {
        return Ok(admitted);
    }
    for (id, error) in placements {
        admitted.retain(|entity| entity.id != id);
        refused.push(batch_entity_error(
            id,
            &WrittenPayloadRefusal::Placement(error),
            EntityHttpClassify::classify_for_entity_http,
        ));
    }
    Ok(admitted)
}

/// The compact half of the admission for one decoded entity.
///
/// An entity-shaped payload is decoded into an [`Entity`] before any handler
/// runs, so the compact JSON the pre-expansion rules are stated over is
/// recovered by rendering that entity back. A member the model does not name
/// survives the decode verbatim, which is exactly what a forged seal is, and
/// comes back unchanged.
fn render_and_validate(entity: &Entity) -> Result<(), WrittenPayloadRefusal> {
    let representation: Value = serde_json::to_value(entity)?;
    validate_fragment_attribute_types(&representation)?;
    reject_embedded_contexts_in_fragment(&representation)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::written_payload_admission::render_and_validate;
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::{Value, json};

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
        assert!(render_and_validate(&entity(&payload)).is_ok());
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
            let refusal = render_and_validate(&entity(&payload)).expect_err("a seal on a Relationship is refused");
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
            let refusal = render_and_validate(&entity(&payload)).expect_err("a seal on a concise Relationship is refused");
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
        assert!(render_and_validate(&entity(&payload)).is_ok());
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
        let refusal = render_and_validate(&entity(&payload)).expect_err("an unknown attribute type is refused");
        assert!(refusal.to_string().contains("NotAnAttributeType"), "{refusal}");
    }
}
