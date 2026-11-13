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
//! - an operation whose local part is left with nothing to do (because an
//!   exclusive or redirect registration claimed every attribute) has still
//!   run the validation, so it cannot answer 201 or 204 to a payload clause
//!   5.5.4 refuses.
//!
//! The rules split by the representation they are decidable on, and one
//! admission runs both halves so a rule added later cannot land on one half
//! only:
//!
//! - Two are decidable only on the compact payload a client wrote, before
//!   JSON-LD expansion rewrites the member names, and the decode of the
//!   request holds both, so a decoded payload satisfies them: the attribute
//!   types of clause 5.2.4, and the rule that `entityIdSealed` and
//!   `entityTypeSealed` belong to the `ngsildproof` Property of clause 4.5.2.2
//!   alone. Clauses 4.5.3.2 and 4.5.3.3 sharpen the second one for a
//!   Relationship, normalized and concise alike, into "'entityIdSealed' and
//!   'entityTypeSealed' shall never be present": an unconditional
//!   prohibition, since the `ngsildproof` exception is a Property's.
//! - A third is decidable on the compact payload because no context can define
//!   the keys it refuses: the clause 4.6.4 refusal of a key JSON-LD expansion
//!   would drop or rewrite past recovery, which the compact screening reads off
//!   the decoded payload. Clause 5.5.7's two rules on the user `@context` (no
//!   JSON-LD Scoped Context, and no part of it below the document's top level)
//!   are the request's, not the payload's: the user context admission owns both,
//!   every admission here takes the context it admitted, and a batch entity that
//!   embedded an `@context` is refused on its own once the context it names was
//!   admitted.
//! - The rest read the expanded payload (where an NGSI-LD Null may stand,
//!   what the `scope` member of clause 4.18 may hold, what an attribute
//!   instance's envelope may carry, which family the geospatial members of
//!   Table 5.2.4-1 are written as, and whether a `GeoProperty` geometry is
//!   well formed), and the entity write admission owns them, because
//!   which of them an operation is exempt from is decided by that operation's
//!   own clause. A merge fragment in the simplified representation is the one
//!   payload whose attribute families are not yet known here: Table 6.5.3.4-1
//!   gives each bare value the type the target holds, so the family rule is
//!   held to the fragment once the local merge rewrites it against the target.
//!
//! Only the report differs between the two shapes of write. Clause 5.6.1
//! answers a single write with a problem details body, while clauses 5.6.7.4
//! to 5.6.9.4 and 5.6.20.4 run the single-entity behaviour "for each of the
//! NGSI-LD Entities included in the input Array" and add "a new
//! `BatchEntityError` containing the failed Entity ID and the related
//! `ProblemDetails`" for each one that fails, leaving the rest of the batch
//! intact. The screening is therefore one admission, projected twice.
//!
//! What the screening expanded is what the local write stores: each admission
//! hands back the admitted payload `andromeda_entity` defines, and the write
//! path consumes it rather than expanding the payload again. Every payload is
//! the typed compact form the extractor decoded, screened and expanded from
//! that form, never rendered back into JSON for either.

use crate::{
    compact_screening::{screen_compact_attribute, screen_compact_entity, screen_compact_patch},
    error::EntityHttpClassify,
};
use andromeda_entity::{
    EntityService,
    admitted_write::{AdmittedAttribute, AdmittedEntity, WritePayload},
    error::EntityError,
    write_kind::{AttributeWriteKind, EntityWriteKind},
};
use andromeda_failure_classification::classified_report::ClassifiedReport;
use andromeda_http_extraction::{batch_body_refusal::BatchBodyRefusal, blocking_decode::DecodeTaskFailure};
use andromeda_http_response::request_refusal::RequestRefusal;
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_entity::admission::written_families::WrittenFamilies;
use andromeda_ngsi_ld_expansion::written_member::WrittenMember;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::entity::{Entity, EntityPatch};
use andromeda_ngsi_ld_problem_details::error_type::NgsiLdErrorType;
use andromeda_ngsi_ld_representation::compact_key_admission::CompactKeyError;
use thiserror::Error;

/// Why a payload a client wrote is not admitted.
#[derive(Debug, Error)]
pub(crate) enum WrittenPayloadRefusal {
    /// The compact payload carries a key clause 4.6.4 refuses.
    #[error(transparent)]
    Key(#[from] CompactKeyError),
    /// The expanded payload puts an NGSI-LD Null, a `scope`, an attribute
    /// envelope or a geometry where the operation does not admit it.
    #[error(transparent)]
    Placement(#[from] EntityError),
    /// The task decoding the payload beside the executor ended without an
    /// outcome.
    #[error(transparent)]
    Decode(#[from] DecodeTaskFailure),
    /// A batch item names no URI `id` under its context either, so nothing
    /// can be reported against it.
    #[error(transparent)]
    Batch(#[from] BatchBodyRefusal),
}

impl EntityHttpClassify for WrittenPayloadRefusal {
    fn classify_for_entity_http(&self) -> ClassifiedReport {
        match self {
            // Clause 5.5.4: data that is not syntactically correct according to
            // the API data type definitions is BadRequestData, as clause 4.6.4
            // makes a value it refuses.
            WrittenPayloadRefusal::Key(source) => ClassifiedReport::public(NgsiLdErrorType::BadRequestData, source.to_string()),
            WrittenPayloadRefusal::Placement(source) => source.classify_for_entity_http(),
            WrittenPayloadRefusal::Decode(source) => source.report(),
            WrittenPayloadRefusal::Batch(source) => ClassifiedReport::public(source.error_type(), source.to_string()),
        }
    }
}

/// An entity-shaped payload as the transport decoded it, which the compact
/// half of the screening reads before the expanded half runs.
pub(crate) trait WrittenEntityPayload: WritePayload {
    /// Screen the payload under the key rule decidable only on its compact
    /// form.
    ///
    /// # Errors
    ///
    /// Returns the first key clause 4.6.4 refuses.
    fn screen(&self) -> Result<(), CompactKeyError>;
}

impl<M: WrittenMember + Sync> WrittenEntityPayload for &Entity<M> {
    fn screen(&self) -> Result<(), CompactKeyError> {
        screen_compact_entity(self)
    }
}

impl<M: WrittenMember + Sync> WrittenEntityPayload for &EntityPatch<M> {
    fn screen(&self) -> Result<(), CompactKeyError> {
        screen_compact_patch(self)
    }
}

/// Screen one entity-shaped payload (a whole Entity or an entity-shaped
/// Fragment), its instances being of `families`, for the write `W` before it
/// is forwarded or stored.
///
/// # Errors
///
/// Returns the first key clause 4.6.4 refuses, and the first placement clause
/// 5.5.4 refuses for `W`.
pub(crate) async fn admit_written_entity<W: EntityWriteKind>(
    service: &EntityService,
    payload: impl WrittenEntityPayload<Member = W::Member>,
    context: &AdmittedContext,
    families: WrittenFamilies,
) -> Result<AdmittedEntity<W>, WrittenPayloadRefusal> {
    payload.screen()?;
    Ok(service.admit_entity_write(payload, context, families).await?)
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
    payload: &W::Member,
    context: &AdmittedContext,
) -> Result<AdmittedAttribute<'a, W>, WrittenPayloadRefusal> {
    screen_compact_attribute(attribute.as_str(), payload)?;
    Ok(service.admit_attribute_write::<W>(attribute, payload, context).await?)
}

#[cfg(test)]
mod tests {
    use crate::compact_screening::screen_compact_entity;
    use andromeda_ngsi_ld_model::entity::Entity;
    use serde_json::{Value, json};

    /// The entity a written payload decodes into, as the extractor produces it.
    fn entity(payload: &Value) -> Entity {
        serde_json::from_value(payload.clone()).expect("the payload decodes into an entity")
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
    /// present", so the decode refuses it.
    #[test]
    fn a_seal_on_a_normalized_relationship_is_refused() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let payload = json!({
                "id": "urn:ngsi-ld:Building:1",
                "type": "Building",
                "belongsTo": {"type": "Relationship", "object": "urn:ngsi-ld:Site:1", member: "urn:ngsi-ld:Building:1"}
            });
            let refusal = serde_json::from_value::<Entity>(payload).expect_err("a seal on a Relationship is refused");
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
            let refusal = serde_json::from_value::<Entity>(payload).expect_err("a seal on a concise Relationship is refused");
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
        let refusal = serde_json::from_value::<Entity>(payload).expect_err("an unknown attribute type is refused");
        assert!(refusal.to_string().contains("not an NGSI-LD attribute type"), "{refusal}");
        assert!(refusal.to_string().contains("/temperature"), "{refusal}");
    }
}
