//! The clause 5.5.4 screening a write payload passes before anything else is
//! done with it.
//!
//! Clause 5.6.1.4 opens with "Execute the behaviour defined in clause 5.5.4 on
//! JSON-LD validation" and only then forwards matching input data to Context
//! Sources; clauses 5.6.2.4, 5.6.3.4, 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.17.4,
//! 5.6.18.4 and 5.6.20.4 state the same two steps in the same order. The
//! screening is therefore the operation's first step and not the local write's
//! own business, and two things follow from the order:
//!
//! - a representation this broker refuses is never handed to a Context Source;
//! - an operation whose local part is left with nothing to do, every
//!   attribute claimed by an exclusive or redirect registration, has still
//!   run the validation, so it cannot answer 201 or 204 to a payload clause
//!   5.5.4 refuses.
//!
//! One admission runs every rule, so a rule added later cannot be added to
//! half of the write paths. The rules that read the written document are the
//! entity model's own (`EntityDocument::admit`, `InstanceDocument::admit_fragment`
//! and the decoding that produced the document); the payload budget is
//! counted on the same document. The transport layer runs them ahead of its
//! forwarding phase through [`EntityService::admit_entity_write`],
//! [`EntityService::admit_attribute_write`] and
//! `EntityService::admit_batch_write`, which expand the payload the way the
//! write path stores it and hand back what they expanded; the write paths
//! accept only such an admitted payload, so no
//! write stores a payload the screening did not read. What a client wrote
//! before expansion, the attribute types of clause 5.2.4, the seals of
//! clause 4.5.2.2 and the `@context` clause 5.5.7 admits only at the
//! document's top level, is decided on the compact payload, which is the
//! transport layer's own half of the same admission.
//!
//! Every admission takes the request's `@context` as an [`AdmittedContext`]:
//! what clause 5.5.7 refuses in a user `@context` is a property of the request
//! rather than of any one entity in it, decided once by
//! `andromeda_jsonld::user_context_admission` before the payload is read.

use crate::{
    EntityService,
    admitted_write::{AdmittedAttribute, AdmittedEntity, WritePayload},
    batch::UpsertPolicy,
    compact_geometry_admission::admit_compact_geometries,
    document_census::{admit_entity_document, admit_instance_document},
    error::EntityError,
    write_kind::{AttributeWriteKind, EntityWriteKind},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_entity::{
    admission::{
        attribute_nesting_admission::{admit_document_nesting, admit_instance_nesting},
        instance_completeness::InstanceCompleteness,
        null_sentinel_admission::NullSentinelAdmission,
        written_families::WrittenFamilies,
    },
    document::EntityDocument,
};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;

/// One of the write operations whose payload is a whole Entity or an
/// entity-shaped Fragment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EntityWrite {
    /// Create Entity (clause 5.6.1).
    Create,
    /// Append Attributes (clause 5.6.3).
    AppendAttributes,
    /// Update Attributes (clause 5.6.2).
    UpdateAttributes,
    /// Replace Entity (clause 5.6.18).
    ReplaceEntity,
    /// Merge Entity (clause 5.6.17).
    MergeEntity,
}

/// One of the two write operations whose payload is a bare Attribute Fragment.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeWrite {
    /// Partial Update Attribute (clause 5.6.4).
    PartialUpdate,
    /// Replace Attribute (clause 5.6.19).
    Replace,
}

/// One of the batch write operations, whose payload is an array of Entities.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BatchWrite {
    /// Batch Entity Creation (clause 5.6.7).
    Create,
    /// Batch Entity Update (clause 5.6.9).
    Update,
    /// Batch Entity Creation or Update (clause 5.6.8), in the mode requested.
    Upsert(UpsertPolicy),
    /// Batch Entity Merge (clause 5.6.20).
    Merge,
}

impl EntityWrite {
    /// Which of the clause 5.5.4 NGSI-LD Null prohibitions this operation is
    /// exempt from.
    ///
    /// Clause 5.5.4 excepts "NGSI-LD Fragments used in partial update and merge
    /// operations", and only clauses 5.6.2, 5.6.4 and 5.6.17 carry the sentence
    /// "NGSI-LD Nulls should be supported by this operation". Clause 5.6.18.4
    /// carries its negation and everything else is silent.
    pub(crate) const fn null_sentinel_admission(self) -> NullSentinelAdmission {
        match self {
            EntityWrite::Create | EntityWrite::AppendAttributes | EntityWrite::ReplaceEntity => NullSentinelAdmission::Refused,
            EntityWrite::UpdateAttributes => NullSentinelAdmission::PartialUpdateFragment,
            EntityWrite::MergeEntity => NullSentinelAdmission::MergeFragment,
        }
    }

    /// The concise rewrite this operation's payload goes through before it is
    /// expanded: a merge keeps a first-level NGSI-LD Null member as the
    /// deletion of that member (clause 5.5.12), every other write reads it as
    /// a concise value.
    pub(crate) const fn normalization(self) -> Normalization {
        match self {
            EntityWrite::Create | EntityWrite::AppendAttributes | EntityWrite::UpdateAttributes | EntityWrite::ReplaceEntity => Normalization::Entity,
            EntityWrite::MergeEntity => Normalization::MemberDeletions,
        }
    }
}

impl AttributeWrite {
    /// Whether the Fragment must stand as a complete instance.
    ///
    /// Clause 5.6.4.4 patches the members the Fragment names (clause 5.5.8);
    /// clause 5.6.19.4 replaces the instance with it, after the clause 5.5.4
    /// validation and before any forwarding, so a Fragment without the value
    /// member its family requires (Table 5.2.5-1) is refused here.
    pub(crate) const fn instance_completeness(self) -> InstanceCompleteness {
        match self {
            AttributeWrite::PartialUpdate => InstanceCompleteness::Partial,
            AttributeWrite::Replace => InstanceCompleteness::Complete,
        }
    }

    /// Which of the clause 5.5.4 NGSI-LD Null prohibitions this operation is
    /// exempt from.
    ///
    /// Clause 5.6.4.4 carries "NGSI-LD Nulls should be supported by this
    /// operation"; clause 5.6.19 replaces a whole Attribute and carries no such
    /// sentence, which leaves it under the clause 5.2.1 default.
    pub(crate) const fn null_sentinel_admission(self) -> NullSentinelAdmission {
        match self {
            AttributeWrite::PartialUpdate => NullSentinelAdmission::PartialUpdateFragment,
            AttributeWrite::Replace => NullSentinelAdmission::Refused,
        }
    }
}

impl BatchWrite {
    /// Which of the clause 5.5.4 NGSI-LD Null prohibitions this operation is
    /// exempt from.
    ///
    /// Clause 5.6.8.4 is the one that splits: its update mode runs "the
    /// behaviour defined by clause 5.6.2 [...] limited to a local operation",
    /// and clause 5.6.2.4 supports NGSI-LD Nulls, while its replace mode
    /// replaces the Entity "completely", the clause 5.6.18 whose clause
    /// 5.6.18.4 states that Nulls are not supported. The update mode's
    /// admission covers only the Entity that exists: one it finds absent is
    /// created by clause 5.6.1, which supports no Null, and is refused when its
    /// payload carries one. Clauses 5.6.7.4 and
    /// 5.6.9.4 run clauses 5.6.1 and 5.6.3, neither of which supports them, and
    /// clause 5.6.20.4 runs the merge of clause 5.6.17, which does.
    pub(crate) const fn null_sentinel_admission(self) -> NullSentinelAdmission {
        match self {
            BatchWrite::Create | BatchWrite::Update | BatchWrite::Upsert(UpsertPolicy::Replace) => NullSentinelAdmission::Refused,
            BatchWrite::Upsert(UpsertPolicy::Update) => NullSentinelAdmission::PartialUpdateFragment,
            BatchWrite::Merge => NullSentinelAdmission::MergeFragment,
        }
    }

    /// The concise rewrite every entity of this operation goes through before
    /// it is expanded.
    ///
    /// Clause 5.6.20.4 runs the Merge Entity of clause 5.6.17 for each entity,
    /// so a batch merge keeps a first-level NGSI-LD Null member as the deletion
    /// of that member (clause 5.5.12) exactly as
    /// [`EntityWrite::normalization`] does for a single merge; every other
    /// batch operation reads it as a concise value.
    pub(crate) const fn normalization(self) -> Normalization {
        match self {
            BatchWrite::Create | BatchWrite::Update | BatchWrite::Upsert(_) => Normalization::Entity,
            BatchWrite::Merge => Normalization::MemberDeletions,
        }
    }
}

/// Screen one written entity document, its instances being of `families`,
/// under clause 5.5.4, the payload budget and the attribute nesting.
///
/// # Errors
///
/// Reports [`EntityError::PayloadBudget`] for a document past `budget`,
/// [`EntityError::AttributeNesting`] for an attribute nesting deeper than it
/// admits, and
/// the [`EntityError::Validation`] of an NGSI-LD Null the operation does not
/// admit, an instance its family does not admit, a geospatial member written
/// as another family than `GeoProperty`, or a `scope` member outside clause
/// 4.18; the HTTP binding answers it with the `BadRequestData` of clause
/// 5.5.4.
pub(crate) fn admit_written_entity(
    budget: EntityPayloadBudget,
    document: &EntityDocument,
    admission: NullSentinelAdmission,
    families: WrittenFamilies,
) -> Result<(), EntityError> {
    admit_entity_document(budget, document)?;
    admit_document_nesting(budget.attribute_nesting, document)?;
    document.admit(admission, families)?;
    Ok(())
}

impl EntityService {
    /// Run the clause 5.5.4 screening of `W` over the entity-shaped payload a
    /// client sent, before the operation forwards any of it, and keep what it
    /// expanded for the local write.
    ///
    /// The payload is expanded here the way `W`'s own path expands it, so the
    /// screening reads the document the write applies. A merge fragment is
    /// the one payload not screened in its final shape: Table 6.5.3.4-1's
    /// `format` and `lang` are applied against the stored entity later, and
    /// that rewrite neither introduces an NGSI-LD Null nor removes one. It
    /// does give a simplified fragment the attribute types the target holds,
    /// so such a fragment is screened with `families` left to the target, and
    /// its families are judged when the rewritten fragment is admitted.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::PayloadBudget`] when a typed geometry or the
    /// expanded document is past the budget, [`EntityError::JsonLd`] when the
    /// payload cannot be expanded against `context`, and the
    /// [`EntityError::Validation`] of a document clause 5.5.4 refuses for `W`.
    pub async fn admit_entity_write<W: EntityWriteKind>(
        &self,
        payload: impl WritePayload<Member = W::Member>,
        context: &AdmittedContext,
        families: WrittenFamilies,
    ) -> Result<AdmittedEntity<W>, EntityError> {
        // Clause 6.3.2: a typed geometry past the budget is refused before the
        // expansion copies it into the written document.
        payload.admit_geometries(self.payload_budget.geometry_positions)?;
        let expansion = EntityExpansion::prepare(&self.engine, context)
            .await?
            .bounded_by(self.payload_budget.json_values);
        let document = payload.expand(&expansion, W::WRITE.normalization()).await?;
        admit_written_entity(self.payload_budget, &document, W::WRITE.null_sentinel_admission(), families)?;
        Ok(AdmittedEntity::new(document))
    }

    /// Run the clause 5.5.4 screening of `W` over the bare Attribute Fragment
    /// of clauses 5.6.4 and 5.6.19 written to `attribute`, before the
    /// operation forwards it, and keep what it expanded for the local write.
    ///
    /// Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, which the
    /// transport decoded into the typed member the operation writes: a
    /// replaced attribute (clause 5.6.19), or the merge member a partial update
    /// writes (clause 5.6.4). The expansion discards the output-only
    /// `previous...` member clauses 5.2.5 to 5.2.38 require an implementation
    /// to ignore, and expands the Fragment under the name the request URI
    /// carries, because a term's own definition decides what the members below
    /// it mean.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when the fragment cannot be expanded against
    /// `context`, [`EntityError::PayloadBudget`] when it is past the budget,
    /// [`EntityError::AttributeNesting`] when it nests deeper than the budget
    /// admits, and the [`EntityError::Validation`] of a fragment clause 5.5.4
    /// refuses for `W`.
    pub async fn admit_attribute_write<'a, W: AttributeWriteKind>(
        &self,
        attribute: &'a AttributeName,
        payload: &W::Member,
        context: &AdmittedContext,
    ) -> Result<AdmittedAttribute<'a, W>, EntityError> {
        // Clause 6.3.2: a typed geometry past the budget is refused before the
        // expansion copies it into the written document.
        admit_compact_geometries(self.payload_budget.geometry_positions, [(attribute.as_str(), payload)])?;
        let expansion = EntityExpansion::prepare(&self.engine, context)
            .await?
            .bounded_by(self.payload_budget.json_values);
        let fragment = expansion.attribute(attribute, payload).await?;
        admit_instance_document(self.payload_budget, fragment.name(), fragment.instance())?;
        admit_instance_nesting(self.payload_budget.attribute_nesting, fragment.name(), fragment.instance())?;
        fragment
            .instance()
            .admit_fragment(fragment.name(), W::WRITE.null_sentinel_admission(), W::WRITE.instance_completeness())?;
        Ok(AdmittedAttribute::new(attribute, fragment))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission_fixture::{admitted_context, admitted_written},
        batch::UpsertPolicy,
        document_census::admit_entity_document,
        entity_validation_reason::EntityValidationReason,
        error::EntityError,
        notification_recording_harness::{budgeted_recording_service, geometry_budget, recording_service, value_budget},
        write_admission::{AttributeWrite, BatchWrite, EntityWrite},
        write_kind::{CreateEntity, PartialUpdateAttribute},
    };
    use andromeda_json_literal::piece::JsonPiece;
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::{
        admission::{instance_completeness::InstanceCompleteness, null_sentinel_admission::NullSentinelAdmission, written_families::WrittenFamilies},
        document::{EntityDocument, instance_document::InstanceDocument},
        refusal::EntityRefusal,
    };
    use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier_length::IDENTIFIER_BYTE_LIMIT};
    use andromeda_ngsi_ld_model::{
        attribute::{instance::AttributeValue, member_reading::MemberReading, merge_member::MergeMember},
        entity::Entity,
    };
    use andromeda_ngsi_ld_representation::payload_budget::{GeometryOwner, PayloadBudgetExceeded};
    use serde_json::{Value, json, value::RawValue};
    use std::slice::from_ref;

    /// Decode an expanded entity document and screen it under `admission`,
    /// as the admission screens what it expanded.
    fn admit_expanded_entity(expanded: &Value, admission: NullSentinelAdmission) -> Result<(), EntityError> {
        let document = EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON"))?;
        document.admit(admission, WrittenFamilies::Final)?;
        Ok(())
    }

    /// Decode an expanded attribute fragment of `attribute` and screen it
    /// as the operation `write` screens it.
    fn admit_expanded_attribute(expanded: &Value, attribute: &AttributeName, write: AttributeWrite) -> Result<(), EntityError> {
        let instance = InstanceDocument::from_slice(attribute, &serde_json::to_vec(expanded).expect("JSON"))?;
        instance.admit_fragment(attribute, write.null_sentinel_admission(), write.instance_completeness())?;
        Ok(())
    }

    /// An identifier one octet past the identifier bound.
    fn identifier_past_the_limit() -> String {
        let prefix = "urn:ngsi-ld:Thing:";
        format!("{prefix}{}", "a".repeat(IDENTIFIER_BYTE_LIMIT + 1 - prefix.len()))
    }

    /// Assert the admission refused the payload as `BadRequestData` naming the
    /// refused length (clause 5.5.4).
    fn assert_refused_past_the_limit(result: Result<(), EntityError>) {
        let error = result.expect_err("the admission refuses an identifier past the bound");
        assert!(matches!(error, EntityError::Validation(_)), "{error:?}");
        assert!(error.to_string().contains("1025 bytes"), "{error}");
    }

    /// One expanded entity carrying a single Property whose value is `value`.
    fn entity_with(value: &Value) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.org/Device"],
            "https://example.org/member": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [value]
            }]
        }])
    }

    fn attribute() -> AttributeName {
        "https://example.org/member".parse().expect("the fixture names a valid attribute name")
    }

    /// Clause 5.5.4 excepts only the fragments of a partial update and a merge,
    /// so the operations that submit a whole Entity refuse the sentinel and the
    /// two that submit a Fragment admit it.
    #[test]
    fn every_entity_write_takes_the_admission_its_clause_gives_it() {
        for (write, admission) in [
            (EntityWrite::Create, NullSentinelAdmission::Refused),
            (EntityWrite::AppendAttributes, NullSentinelAdmission::Refused),
            (EntityWrite::ReplaceEntity, NullSentinelAdmission::Refused),
            (EntityWrite::UpdateAttributes, NullSentinelAdmission::PartialUpdateFragment),
            (EntityWrite::MergeEntity, NullSentinelAdmission::MergeFragment),
        ] {
            pretty_assertions::assert_eq!(write.null_sentinel_admission(), admission, "{write:?}");
        }
    }

    #[test]
    fn every_attribute_write_takes_the_admission_its_clause_gives_it() {
        pretty_assertions::assert_eq!(
            AttributeWrite::PartialUpdate.null_sentinel_admission(),
            NullSentinelAdmission::PartialUpdateFragment
        );
        pretty_assertions::assert_eq!(AttributeWrite::Replace.null_sentinel_admission(), NullSentinelAdmission::Refused);
    }

    /// Clause 5.6.8.4 routes an existing Entity to clause 5.6.2 in update mode
    /// and to a complete replacement in replace mode, and only the first of the
    /// two supports NGSI-LD Nulls.
    #[test]
    fn the_two_upsert_modes_take_different_admissions() {
        pretty_assertions::assert_eq!(
            BatchWrite::Upsert(UpsertPolicy::Update).null_sentinel_admission(),
            NullSentinelAdmission::PartialUpdateFragment
        );
        pretty_assertions::assert_eq!(
            BatchWrite::Upsert(UpsertPolicy::Replace).null_sentinel_admission(),
            NullSentinelAdmission::Refused
        );
        pretty_assertions::assert_eq!(BatchWrite::Create.null_sentinel_admission(), NullSentinelAdmission::Refused);
        pretty_assertions::assert_eq!(BatchWrite::Update.null_sentinel_admission(), NullSentinelAdmission::Refused);
        pretty_assertions::assert_eq!(BatchWrite::Merge.null_sentinel_admission(), NullSentinelAdmission::MergeFragment);
    }

    /// Clause 5.6.20.4 runs clause 5.6.17 per entity, so a batch merge rewrites
    /// its entities as Merge Entity rewrites its fragment, and only it does.
    #[test]
    fn a_batch_merge_rewrites_its_entities_as_merge_fragments() {
        for (write, normalization) in [
            (BatchWrite::Create, Normalization::Entity),
            (BatchWrite::Update, Normalization::Entity),
            (BatchWrite::Upsert(UpsertPolicy::Replace), Normalization::Entity),
            (BatchWrite::Upsert(UpsertPolicy::Update), Normalization::Entity),
            (BatchWrite::Merge, Normalization::MemberDeletions),
        ] {
            pretty_assertions::assert_eq!(write.normalization(), normalization, "{write:?}");
        }
        pretty_assertions::assert_eq!(BatchWrite::Merge.normalization(), EntityWrite::MergeEntity.normalization());
    }

    /// The one admission runs every rule: the same payload is refused for its
    /// NGSI-LD Null under a create and admitted under a partial update.
    #[test]
    fn the_entity_admission_refuses_a_null_value_outside_a_fragment() {
        let payload = entity_with(&json!({"@value": "urn:ngsi-ld:null"}));
        let error = admit_expanded_entity(&payload, EntityWrite::Create.null_sentinel_admission()).expect_err("clause 5.5.4 refuses it here");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        admit_expanded_entity(&payload, EntityWrite::UpdateAttributes.null_sentinel_admission()).expect("a partial update deletes with it");
    }

    /// A malformed geometry and a scope outside clause 4.18 are refused by the
    /// same step, so no write path can be wired to one rule and not the other.
    #[test]
    fn the_entity_admission_refuses_a_malformed_geometry_and_an_unstorable_scope() {
        // RFC 7946 section 3.1.1 gives a position two or three ordinates, and
        // clause 4.7 adopts the RFC whole.
        let geometry = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.org/Device"],
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{
                    "@type": ["https://purl.org/geojson/vocab#Point"],
                    "https://purl.org/geojson/vocab#coordinates": [{"@list": [{"@value": 1.0}, {"@value": 2.0}, {"@value": 3.0}, {"@value": 4.0}]}]
                }]
            }]
        }]);
        assert!(admit_expanded_entity(&geometry, EntityWrite::Create.null_sentinel_admission()).is_err());

        let scope =
            json!([{"@id": "urn:ngsi-ld:Device:1", "@type": ["https://example.org/Device"], "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/A B"}]}]);
        assert!(admit_expanded_entity(&scope, EntityWrite::Create.null_sentinel_admission()).is_err());
    }

    #[test]
    fn the_attribute_admission_refuses_a_null_value_outside_a_fragment() {
        let fragment = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": "urn:ngsi-ld:null"}]
        }]);
        let error = admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::Replace).expect_err("clause 5.6.19 replaces a whole attribute");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::PartialUpdate).expect("clause 5.6.4 supports NGSI-LD Nulls");
    }

    /// Clause 5.6.19.4 runs the clause 5.5.4 validation of a replacement, which
    /// is a whole instance (Table 5.2.5-1): a Fragment naming only a `unitCode`
    /// is read as a Property without its value and refused, where the partial
    /// update of clause 5.6.4 patches the member it names.
    #[test]
    fn the_replace_admission_refuses_a_fragment_without_a_value() {
        let fragment = json!([{"https://uri.etsi.org/ngsi-ld/unitCode": [{"@value": "C62"}]}]);
        let error = admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::Replace).expect_err("a replacement carries its value");
        assert!(matches!(error, EntityError::Validation(_)), "{error:?}");
        assert!(error.to_string().contains("Property"), "{error}");
        assert!(error.to_string().contains("'value'"), "{error}");
        admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::PartialUpdate).expect("a partial update patches the member it names");
    }

    /// Clause 5.6.19.4 for a Fragment written as plain JSON: `{}` and a
    /// Fragment naming only a `unitCode` are not a replacement the compact
    /// reading of the transport accepts (the typed member of a replacement is a
    /// whole instance), while the partial update of clause 5.6.4 admits both
    /// through the whole admission, normalization and expansion included (an
    /// empty Fragment is a JSON Merge Patch that changes nothing, clause 5.4).
    #[tokio::test]
    async fn a_replacement_written_as_plain_json_without_a_value_is_refused() {
        let (service, _recorded) = recording_service().await;
        let context = admitted_context(&service, &ContextSpec::core_only()).await;
        let speed: AttributeName = "speed".parse().expect("a valid attribute name");
        for written in [json!({}), json!({"unitCode": "C62"})] {
            let raw = RawValue::from_string(written.to_string()).expect("the fragment is JSON");
            let refused = AttributeValue::from_fragment_node(speed.as_str(), JsonPiece::new(&raw)).expect_err("a replacement carries its value");
            assert!(refused.to_string().contains("value"), "{written}: {refused}");

            let patch = MergeMember::from_fragment_node(speed.as_str(), JsonPiece::new(&raw)).expect("the fragment decodes");
            service
                .admit_attribute_write::<PartialUpdateAttribute>(&speed, &patch, &context)
                .await
                .expect("a partial update patches the members it names");
        }
    }

    /// Clause 5.6.19.4: the Fragment's family decides which member is missing.
    #[test]
    fn a_replacement_without_its_family_value_is_refused_with_that_familys_refusal() {
        let cases = [
            (json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Relationship"]}]), "Relationship"),
            (json!([{"@type": ["https://uri.etsi.org/ngsi-ld/Property"]}]), "'value'"),
            (json!([{"@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"]}]), "GeoProperty"),
            (json!([{"@type": ["https://uri.etsi.org/ngsi-ld/LanguageProperty"]}]), "LanguageProperty"),
            (json!([{}]), "Property"),
        ];
        for (fragment, expected) in cases {
            let error = admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::Replace).expect_err("the value member is missing");
            assert!(matches!(error, EntityError::Validation(_)), "{fragment}: {error:?}");
            assert!(error.to_string().contains(expected), "{fragment}: {error}");
        }
    }

    #[test]
    fn every_attribute_write_states_whether_it_needs_a_complete_instance() {
        pretty_assertions::assert_eq!(AttributeWrite::PartialUpdate.instance_completeness(), InstanceCompleteness::Partial);
        pretty_assertions::assert_eq!(AttributeWrite::Replace.instance_completeness(), InstanceCompleteness::Complete);
    }

    #[test]
    fn the_entity_admission_refuses_an_expanded_type_past_the_identifier_limit() {
        let payload = json!([{"@id": "urn:ngsi-ld:Device:1", "@type": [identifier_past_the_limit()]}]);
        assert_refused_past_the_limit(admit_expanded_entity(&payload, EntityWrite::Create.null_sentinel_admission()));
    }

    #[test]
    fn the_entity_admission_refuses_an_expanded_entity_id_past_the_identifier_limit() {
        let payload = json!([{"@id": identifier_past_the_limit(), "@type": ["https://example.org/Device"]}]);
        assert_refused_past_the_limit(admit_expanded_entity(&payload, EntityWrite::Create.null_sentinel_admission()));
    }

    #[test]
    fn the_entity_admission_refuses_an_expanded_dataset_id_past_the_identifier_limit() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.org/Device"],
            "https://example.org/member": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": identifier_past_the_limit()}]
            }]
        }]);
        assert_refused_past_the_limit(admit_expanded_entity(&payload, EntityWrite::AppendAttributes.null_sentinel_admission()));
    }

    #[test]
    fn the_entity_admission_refuses_an_expanded_instance_id_past_the_identifier_limit() {
        let payload = json!([{
            "@id": "urn:ngsi-ld:Device:1",
            "@type": ["https://example.org/Device"],
            "https://example.org/member": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
                "https://uri.etsi.org/ngsi-ld/instanceId": [{"@id": identifier_past_the_limit()}]
            }]
        }]);
        assert_refused_past_the_limit(admit_expanded_entity(&payload, EntityWrite::Create.null_sentinel_admission()));
    }

    #[test]
    fn the_attribute_admission_refuses_a_dataset_id_past_the_identifier_limit() {
        let fragment = json!([{
            "@type": ["https://uri.etsi.org/ngsi-ld/Property"],
            "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": 1}],
            "https://uri.etsi.org/ngsi-ld/datasetId": [{"@id": identifier_past_the_limit()}]
        }]);
        assert_refused_past_the_limit(admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::PartialUpdate));
    }

    /// A Vehicle carrying `speed` and `brandName`, as a client writes it.
    fn written_vehicle() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "speed": {"type": "Property", "value": 1},
            "brandName": {"type": "Property", "value": "x"}
        })
    }

    /// The expanded names `written_vehicle` carries its attributes under in the
    /// core context.
    fn vehicle_attributes() -> (AttributeName, AttributeName) {
        (
            "https://uri.etsi.org/ngsi-ld/default-context/speed".parse().expect("a valid attribute name"),
            "https://uri.etsi.org/ngsi-ld/default-context/brandName"
                .parse()
                .expect("a valid attribute name"),
        )
    }

    /// Clause 5.6.1.4 and following: a payload no proxied attribute was taken
    /// out of is written exactly as the admission expanded it.
    #[tokio::test]
    async fn an_untouched_payload_is_the_admitted_one() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let expected = admitted_written::<CreateEntity>(&service, written_vehicle(), &context).await.into_document();
        let admitted = admitted_written::<CreateEntity>(&service, written_vehicle(), &context).await;
        let absent: AttributeName = "https://example.org/colour".parse().expect("a valid attribute name");
        let remainder = admitted.local_remainder(&[absent]).expect("something remains");
        pretty_assertions::assert_eq!(remainder.into_document(), expected);
    }

    /// A narrowed payload is the admitted document without the attribute the
    /// proxied registration took, and is not admitted again.
    #[tokio::test]
    async fn a_narrowed_payload_loses_the_proxied_attribute() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let admitted = admitted_written::<CreateEntity>(&service, written_vehicle(), &context).await;
        let (speed, brand) = vehicle_attributes();
        let remainder = admitted.local_remainder(from_ref(&speed)).expect("something remains");
        let names: Vec<&AttributeName> = remainder.document().attributes().keys().collect();
        pretty_assertions::assert_eq!(names, vec![&brand]);
        assert!(!remainder.document().attributes().contains_key(&speed));
    }

    /// Clause 5.6.1.4 and following: a payload left with no attribute leaves
    /// the local phase nothing to write.
    #[tokio::test]
    async fn an_emptied_payload_leaves_the_local_phase_nothing_to_write() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let admitted = admitted_written::<CreateEntity>(&service, written_vehicle(), &context).await;
        let (speed, brand) = vehicle_attributes();
        assert!(admitted.local_remainder(&[speed, brand]).is_none());
    }

    /// A Vehicle located by a square: one ring of five positions.
    fn located_vehicle() -> Entity {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "location": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}}
        }))
        .expect("a valid entity")
    }

    /// Clause 6.3.2: an oversized geometry is refused as such, while the same
    /// payload within the budget is admitted.
    #[tokio::test]
    async fn the_create_admission_refuses_an_oversized_geometry() {
        let tight = budgeted_recording_service(geometry_budget(4));
        let refusal = tight
            .admit_entity_write::<CreateEntity>(
                &located_vehicle(),
                &admitted_context(&tight, &ContextSpec::core_only()).await,
                WrittenFamilies::Final,
            )
            .await
            .expect_err("five positions exceed a budget of four");
        assert!(
            matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions { .. })),
            "{refusal:?}"
        );
        let roomy = budgeted_recording_service(geometry_budget(5));
        roomy
            .admit_entity_write::<CreateEntity>(
                &located_vehicle(),
                &admitted_context(&roomy, &ContextSpec::core_only()).await,
                WrittenFamilies::Final,
            )
            .await
            .expect("five positions are within a budget of five");
    }

    /// Clause 5.5.4: the NGSI-LD Null is an absolute IRI that expansion
    /// leaves as written, so a compact `type` naming it reaches the admission
    /// as the Null, alone or beside another type, and a create is refused.
    #[tokio::test]
    async fn the_create_admission_refuses_the_null_as_an_entity_type() {
        let (service, _writes) = recording_service().await;
        let context = admitted_context(&service, &ContextSpec::core_only()).await;
        for types in [json!("urn:ngsi-ld:null"), json!(["Vehicle", "urn:ngsi-ld:null"])] {
            let entity: Entity = serde_json::from_value(json!({"id": "urn:ngsi-ld:Vehicle:1", "type": types})).expect("the compact model reads it");
            let refusal = service
                .admit_entity_write::<CreateEntity>(&entity, &context, WrittenFamilies::Final)
                .await
                .expect_err("clause 5.5.4 refuses a first-level Null in a create");
            assert!(
                matches!(&refusal, EntityError::Validation(EntityValidationReason::Refused(refused)) if matches!(**refused, EntityRefusal::TypeNull)),
                "{types}: {refusal:?}"
            );
        }
    }

    /// Assert that, for every limit from one value to `up_to`, the admission of
    /// `probe` answers as the census of its expanded document does, and that
    /// the limits tried hold both an admission and a refusal.
    async fn assert_walk_refuses_where_census_refuses(probe: &Entity, up_to: usize) {
        let (unbounded, _writes) = recording_service().await;
        let document = EntityExpansion::prepare(&unbounded.engine, &ContextSpec::core_only())
            .await
            .expect("the core context processes")
            .entity(probe, Normalization::Entity)
            .await
            .expect("the probe expands");
        let mut answers = Vec::new();
        for values in 1..=up_to {
            let census = admit_entity_document(value_budget(values), &document);
            let service = budgeted_recording_service(value_budget(values));
            let admission = service
                .admit_entity_write::<CreateEntity>(probe, &admitted_context(&service, &ContextSpec::core_only()).await, WrittenFamilies::Final)
                .await;
            match (&census, &admission) {
                (Ok(()), Ok(_))
                | (Err(PayloadBudgetExceeded::EntityJsonValues { .. }), Err(EntityError::PayloadBudget(PayloadBudgetExceeded::EntityJsonValues { .. }))) => {}
                (census, admission) => panic!("under {values} values the census answers {census:?} and the admission {admission:?}"),
            }
            answers.push(census.is_ok());
        }
        assert!(
            answers.contains(&false) && answers.contains(&true),
            "the probe crosses the limits tried: {answers:?}"
        );
    }

    /// Clause 6.3.2: the walk counts an entity's values while it expands them
    /// and refuses at exactly the limit the census of the whole expanded
    /// document refuses at, so counting early changes no answer.
    #[tokio::test]
    async fn the_admission_refuses_values_at_the_limit_the_census_of_the_document_refuses_at() {
        let probe: Entity = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Probe:1",
            "type": "Probe",
            "scope": ["/a", "/b"],
            "n": {"type": "Property", "value": [1, 2, 3, 4, 5, 6, 7, 8], "observedAt": "2024-01-01T00:00:00Z"},
            "m": {"type": "Property", "value": {"a": [1, 2], "b": {"c": true}}, "unitCode": "KMH"},
            "l": {"type": "ListProperty", "valueList": [1, [2, 3]]}
        }))
        .expect("a valid entity");
        assert_walk_refuses_where_census_refuses(&probe, 40).await;
    }

    /// Clause 6.3.2: an `object`, an `objectList`, a `vocab`, a `languageMap`
    /// and a `scope` are counted before they are expanded, by the rule the
    /// census counts them by, so the walk refuses at exactly the limit the
    /// census refuses at and an entity at the limit is admitted.
    #[tokio::test]
    async fn the_admission_counts_object_vocab_language_and_scope_members_as_the_census_does() {
        let probe: Entity = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Probe:1",
            "type": "Probe",
            "scope": ["/a", "/b", "/c"],
            "owner": {"type": "Relationship", "object": ["urn:ngsi-ld:Person:1", "urn:ngsi-ld:Person:2"]},
            "friends": {"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Person:3", "urn:ngsi-ld:Person:4", "urn:ngsi-ld:Person:5"]},
            "kind": {"type": "VocabProperty", "vocab": ["a", "b"]},
            "name": {"type": "LanguageProperty", "languageMap": {"en": "a", "fr": ["b", "c"], "@none": "d"}}
        }))
        .expect("a valid entity");
        assert_walk_refuses_where_census_refuses(&probe, 60).await;
    }

    /// Clause 4.5.2.3: a concise `GeoProperty` is its geometry alone, which
    /// the normalization wraps into the instance the census reads, so its
    /// positions are budgeted as a normalized one's are.
    #[tokio::test]
    async fn a_concise_geo_property_is_budgeted_after_normalization() {
        let concise: Entity = serde_json::from_value(json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "location": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}
        }))
        .expect("a valid entity");
        let tight = budgeted_recording_service(geometry_budget(4));
        let refusal = tight
            .admit_entity_write::<CreateEntity>(&concise, &admitted_context(&tight, &ContextSpec::core_only()).await, WrittenFamilies::Final)
            .await
            .expect_err("five positions exceed a budget of four");
        assert!(
            matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions { .. })),
            "{refusal:?}"
        );
        let roomy = budgeted_recording_service(geometry_budget(5));
        roomy
            .admit_entity_write::<CreateEntity>(&concise, &admitted_context(&roomy, &ContextSpec::core_only()).await, WrittenFamilies::Final)
            .await
            .expect("five positions are within a budget of five");
    }

    /// Clause 6.3.2: a typed entity's geometry past the budget is refused,
    /// naming the attribute as the client wrote it.
    #[tokio::test]
    async fn a_typed_geometry_past_the_budget_is_refused_naming_the_written_attribute() {
        let entity = located_vehicle();
        let tight = budgeted_recording_service(geometry_budget(4));
        let refusal = tight
            .admit_entity_write::<CreateEntity>(&entity, &admitted_context(&tight, &ContextSpec::core_only()).await, WrittenFamilies::Final)
            .await
            .expect_err("five positions exceed a budget of four");
        let EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions {
            owner: GeometryOwner::Attribute(owner),
            ..
        }) = refusal
        else {
            panic!("the refusal is the geometry budget: {refusal:?}");
        };
        pretty_assertions::assert_eq!(owner.as_str(), "location");
    }
}
