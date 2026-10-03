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
//! - an operation whose local part is left with nothing to do -- every
//!   attribute claimed by an exclusive or redirect registration -- has still
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
//! accept only such an admitted payload (see `crate::admitted_write`), so no
//! write stores a payload the screening did not read. What a client wrote
//! before expansion -- the attribute types of clause 5.2.4, the seals of
//! clause 4.5.2.2 and the `@context` clause 5.5.7 admits only at the
//! document's top level -- is decided on the compact payload, which is the
//! transport layer's own half of the same admission.
//!
//! [`EntityService::admit_write_context`] is the one rule here that reads the
//! request's `@context` instead of its payload: clause 5.5.7 refuses a user
//! `@context` declaring a JSON-LD Scoped Context, and that is a property of the
//! request rather than of any one entity in it.

use crate::{
    EntityService,
    admitted_write::{AdmittedAttribute, AdmittedEntity, WritePayload},
    batch::UpsertPolicy,
    document_census::{admit_entity_document, admit_instance_document},
    error::EntityError,
    write_kind::{AttributeWriteKind, EntityWriteKind},
};
use andromeda_jsonld::{context_spec::ContextSpec, scoped_context_admission::reject_scoped_user_contexts};
use andromeda_ngsi_ld_entity::{
    admission::{
        attribute_nesting_admission::{admit_document_nesting, admit_instance_nesting},
        null_sentinel_admission::NullSentinelAdmission,
    },
    document::EntityDocument,
};
use andromeda_ngsi_ld_expansion::{entity_expansion::EntityExpansion, normalization::Normalization};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::attribute_strip::AttributeStrip;
use andromeda_ngsi_ld_representation::payload_budget::EntityPayloadBudget;
use serde_json::Value;

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
            EntityWrite::MergeEntity => Normalization::MergeFragment,
        }
    }
}

impl AttributeWrite {
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
    /// replaces the Entity "completely" -- the clause 5.6.18 whose clause
    /// 5.6.18.4 states that Nulls are not supported. Clauses 5.6.7.4 and
    /// 5.6.9.4 run clauses 5.6.1 and 5.6.3, neither of which supports them, and
    /// clause 5.6.20.4 runs the merge of clause 5.6.17, which does.
    pub(crate) const fn null_sentinel_admission(self) -> NullSentinelAdmission {
        match self {
            BatchWrite::Create | BatchWrite::Update | BatchWrite::Upsert(UpsertPolicy::Replace) => NullSentinelAdmission::Refused,
            BatchWrite::Upsert(UpsertPolicy::Update) => NullSentinelAdmission::PartialUpdateFragment,
            BatchWrite::Merge => NullSentinelAdmission::MergeFragment,
        }
    }
}

/// Screen one written entity document under clause 5.5.4, the payload budget
/// and the attribute nesting.
///
/// # Errors
///
/// Reports [`EntityError::PayloadBudget`] for a document past `budget`,
/// [`EntityError::AttributeNesting`] for an attribute nesting deeper than it
/// admits, and
/// the [`EntityError::Validation`] of an NGSI-LD Null the operation does not
/// admit, an instance its family does not admit, or a `scope` member outside
/// clause 4.18; the HTTP binding answers it with the `BadRequestData` of
/// clause 5.5.4.
pub(crate) fn admit_written_entity(budget: EntityPayloadBudget, document: &EntityDocument, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    admit_entity_document(budget, document)?;
    admit_document_nesting(budget.attribute_nesting, document)?;
    document.admit(admission)?;
    Ok(())
}

impl EntityService {
    /// Refuse a request `@context` clause 5.5.7 does not admit, before the
    /// operation forwards anything or expands the payload against it.
    ///
    /// Clause 5.5.7 states that the user `@context` "shall not contain JSON-LD
    /// Scoped Contexts", and that a part of a user `@context` breaking that
    /// "should result in an error of type `BadRequestData`, because JSON-LD
    /// Scoped Contexts and nested embedded @context could be used to modify
    /// terms defined in the Core @context or to reshape NGSI-LD Elements during
    /// the expansion of terms". It is the request's context that is refused and
    /// not one entity's, which is why this stands beside the per-payload
    /// screening rather than inside it: a batch whose `@context` is refused has
    /// no admissible entity in it.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] carrying
    /// [`JsonLdError::ScopedUserContext`](andromeda_jsonld::error::JsonLdError::ScopedUserContext),
    /// which the HTTP binding answers with `BadRequestData`, and the typed
    /// context-loading failure when one of the request's contexts does not
    /// resolve.
    pub async fn admit_write_context(&self, context: &ContextSpec) -> Result<(), EntityError> {
        reject_scoped_user_contexts(&self.engine, context).await?;
        Ok(())
    }

    /// Run the clause 5.5.4 screening of `W` over the entity-shaped payload a
    /// client sent, before the operation forwards any of it, and keep what it
    /// expanded for the local write.
    ///
    /// The payload is expanded here the way `W`'s own path expands it, so the
    /// screening reads the document the write applies. A merge fragment is
    /// the one payload not screened in its final shape: Table 6.5.3.4-1's
    /// `format` and `lang` are applied against the stored entity later, and
    /// that rewrite neither introduces an NGSI-LD Null nor removes one.
    ///
    /// # Errors
    ///
    /// Reports the refusal of [`EntityService::admit_write_context`], and those
    /// of [`EntityService::admit_remaining_entity`].
    pub async fn admit_entity_write<W: EntityWriteKind>(&self, payload: impl WritePayload, context: &ContextSpec) -> Result<AdmittedEntity<W>, EntityError> {
        self.admit_write_context(context).await?;
        self.admit_remaining_entity(payload, context).await
    }

    /// Run the clause 5.5.4 screening of `W` over what remains of a payload
    /// once the forwarding phase took the attributes proxied registrations own
    /// out of it.
    ///
    /// The request `@context` was admitted with the whole payload, so only the
    /// payload rules run again, over the document the local write now applies.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::PayloadBudget`] when a typed geometry or the
    /// expanded document is past the budget, [`EntityError::JsonLd`] when the
    /// payload cannot be expanded against `context`, and the [`EntityError::Validation`] of a
    /// document clause 5.5.4 refuses for `W`.
    pub async fn admit_remaining_entity<W: EntityWriteKind>(
        &self,
        payload: impl WritePayload,
        context: &ContextSpec,
    ) -> Result<AdmittedEntity<W>, EntityError> {
        // Clause 6.3.2: a typed geometry past the budget is refused before the
        // expansion copies it into the written document.
        payload.admit_geometries(self.payload_budget.geometry_positions)?;
        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let document = payload.expand(&expansion, W::WRITE.normalization()).await?;
        admit_written_entity(self.payload_budget, &document, W::WRITE.null_sentinel_admission())?;
        Ok(AdmittedEntity::new(document))
    }

    /// The payload the local phase of a distributed write stores, given what
    /// stripping the proxied attributes left of the payload `admitted` was
    /// expanded from (clauses 5.6.1.4 and following: "any remaining input
    /// data").
    ///
    /// An untouched payload is the admitted one; a narrowed one is `remainder`,
    /// admitted anew; an emptied one leaves the local phase nothing to write,
    /// which is `None`.
    ///
    /// # Errors
    ///
    /// The inner result reports the refusals of
    /// [`EntityService::admit_remaining_entity`] for a narrowed payload.
    pub async fn admit_local_remainder<W: EntityWriteKind>(
        &self,
        admitted: AdmittedEntity<W>,
        strip: AttributeStrip,
        remainder: impl WritePayload,
        context: &ContextSpec,
    ) -> Option<Result<AdmittedEntity<W>, EntityError>> {
        match strip {
            AttributeStrip::Untouched => Some(Ok(admitted)),
            AttributeStrip::Narrowed => Some(self.admit_remaining_entity(remainder, context).await),
            AttributeStrip::Emptied => None,
        }
    }

    /// Run the clause 5.5.4 screening of `W` over the bare Attribute Fragment
    /// of clauses 5.6.4 and 5.6.19 written to `attribute`, before the
    /// operation forwards it, and keep what it expanded for the local write.
    ///
    /// Clauses 5.6.4 and 5.6.19 take a bare Attribute Fragment, so the
    /// entity-level rewrite does not reach it: the expansion applies the
    /// concise form and discards the output-only `previous...` member clauses
    /// 5.2.5 to 5.2.38 require an implementation to ignore, and expands the
    /// Fragment under the name the request URI carries, because a term's own
    /// definition decides what the members below it mean.
    ///
    /// # Errors
    ///
    /// Reports the refusal of [`EntityService::admit_write_context`],
    /// [`EntityError::JsonLd`] when the fragment cannot be expanded against
    /// `context`, [`EntityError::PayloadBudget`] when it is past the budget,
    /// [`EntityError::AttributeNesting`] when it nests deeper than the budget
    /// admits, and the [`EntityError::Validation`] of a fragment clause 5.5.4
    /// refuses for `W`.
    pub async fn admit_attribute_write<'a, W: AttributeWriteKind>(
        &self,
        attribute: &'a AttributeName,
        payload: Value,
        context: &ContextSpec,
    ) -> Result<AdmittedAttribute<'a, W>, EntityError> {
        self.admit_write_context(context).await?;
        let expansion = EntityExpansion::prepare(&self.engine, context).await?;
        let fragment = expansion.attribute_fragment(attribute, payload).await?;
        admit_instance_document(self.payload_budget, fragment.name(), fragment.instance())?;
        admit_instance_nesting(self.payload_budget.attribute_nesting, fragment.name(), fragment.instance())?;
        fragment.instance().admit_fragment(fragment.name(), W::WRITE.null_sentinel_admission())?;
        Ok(AdmittedAttribute::new(attribute, fragment))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        admission_fixture::admitted_entity,
        batch::UpsertPolicy,
        error::EntityError,
        notification_recording_harness::{budgeted_recording_service, geometry_budget, recording_service},
        write_admission::{AttributeWrite, BatchWrite, EntityWrite},
        write_kind::CreateEntity,
    };
    use andromeda_jsonld::context_spec::ContextSpec;
    use andromeda_ngsi_ld_entity::{
        admission::null_sentinel_admission::NullSentinelAdmission,
        document::{EntityDocument, instance_document::InstanceDocument},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier_length::IDENTIFIER_BYTE_LIMIT};
    use andromeda_ngsi_ld_model::{attribute_strip::AttributeStrip, entity::Entity};
    use andromeda_ngsi_ld_representation::payload_budget::{GeometryOwner, PayloadBudgetExceeded};
    use serde_json::{Value, json};

    /// Decode an expanded entity document and screen it under `admission`,
    /// as the admission screens what it expanded.
    fn admit_expanded_entity(expanded: &Value, admission: NullSentinelAdmission) -> Result<(), EntityError> {
        let document = EntityDocument::from_slice(&serde_json::to_vec(expanded).expect("JSON"))?;
        document.admit(admission)?;
        Ok(())
    }

    /// Decode an expanded attribute fragment of `attribute` and screen it
    /// under `admission`.
    fn admit_expanded_attribute(expanded: &Value, attribute: &AttributeName, admission: NullSentinelAdmission) -> Result<(), EntityError> {
        let instance = InstanceDocument::from_slice(attribute, &serde_json::to_vec(expanded).expect("JSON"))?;
        instance.admit_fragment(attribute, admission)?;
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
        let error = admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::Replace.null_sentinel_admission())
            .expect_err("clause 5.6.19 replaces a whole attribute");
        assert!(error.to_string().contains("urn:ngsi-ld:null"), "{error}");
        admit_expanded_attribute(&fragment, &attribute(), AttributeWrite::PartialUpdate.null_sentinel_admission())
            .expect("clause 5.6.4 supports NGSI-LD Nulls");
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
        assert_refused_past_the_limit(admit_expanded_attribute(
            &fragment,
            &attribute(),
            AttributeWrite::PartialUpdate.null_sentinel_admission(),
        ));
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

    /// What the forwarding phase left of `written_vehicle` once `speed` was
    /// claimed by a proxied registration.
    fn remaining_vehicle(brand: &Value) -> Value {
        json!({"id": "urn:ngsi-ld:Vehicle:1", "type": "Vehicle", "brandName": {"type": "Property", "value": brand}})
    }

    /// Clause 5.6.1.4 and following: a payload the forwarding phase left alone
    /// is written exactly as the admission expanded it, without expanding it
    /// again.
    #[tokio::test]
    async fn an_untouched_payload_is_the_admitted_one() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let expected = admitted_entity::<CreateEntity>(&service, written_vehicle(), &context).await.into_document();
        let admitted = admitted_entity::<CreateEntity>(&service, written_vehicle(), &context).await;
        let remainder = service
            .admit_local_remainder(admitted, AttributeStrip::Untouched, remaining_vehicle(&json!("ignored")), &context)
            .await
            .expect("something remains")
            .expect("the admitted payload is kept");
        pretty_assertions::assert_eq!(remainder.into_document(), expected);
    }

    /// A narrowed payload is a different document: what remains is admitted
    /// anew, and the attribute the proxied registration took is gone from it.
    #[tokio::test]
    async fn a_narrowed_payload_is_admitted_anew_from_what_remains() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let admitted = admitted_entity::<CreateEntity>(&service, written_vehicle(), &context).await;
        let remainder = service
            .admit_local_remainder(admitted, AttributeStrip::Narrowed, remaining_vehicle(&json!("x")), &context)
            .await
            .expect("something remains")
            .expect("the remainder is admitted");
        let names: Vec<String> = remainder.into_document().attributes().keys().map(ToString::to_string).collect();
        assert!(!names.iter().any(|name| name.ends_with("/speed")), "{names:?}");
        assert!(names.iter().any(|name| name.ends_with("/brandName")), "{names:?}");
    }

    /// The remainder is screened in its own right, so a remainder clause 5.5.4
    /// refuses is refused rather than written.
    #[tokio::test]
    async fn a_narrowed_payload_is_screened_again() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let admitted = admitted_entity::<CreateEntity>(&service, written_vehicle(), &context).await;
        let refusal = service
            .admit_local_remainder(admitted, AttributeStrip::Narrowed, remaining_vehicle(&json!("urn:ngsi-ld:null")), &context)
            .await
            .expect("something remains")
            .expect_err("a creation admits no NGSI-LD Null");
        assert!(matches!(refusal, EntityError::Validation(_)), "{refusal}");
    }

    /// Clause 5.6.1.4 and following: a payload left with no attribute leaves
    /// the local phase nothing to write.
    #[tokio::test]
    async fn an_emptied_payload_leaves_the_local_phase_nothing_to_write() {
        let (service, _recorded) = recording_service().await;
        let context = ContextSpec::core_only();
        let admitted = admitted_entity::<CreateEntity>(&service, written_vehicle(), &context).await;
        let remainder = service
            .admit_local_remainder(admitted, AttributeStrip::Emptied, remaining_vehicle(&json!("x")), &context)
            .await;
        assert!(remainder.is_none());
    }

    /// A Vehicle located by a square: one ring of five positions.
    fn located_vehicle() -> Value {
        json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "location": {"type": "GeoProperty", "value": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}}
        })
    }

    /// A context the offline engine cannot load, so any expansion against it
    /// fails.
    fn unloadable_context() -> ContextSpec {
        ContextSpec::from_body(json!("https://unloadable.example/context.jsonld"))
    }

    /// Clause 6.3.2: an oversized geometry is refused as such, while the same
    /// payload within the budget is admitted; a context that cannot be loaded
    /// fails the expansion before anything is counted.
    #[tokio::test]
    async fn the_create_admission_refuses_an_oversized_geometry() {
        let refusal = budgeted_recording_service(geometry_budget(4))
            .admit_remaining_entity::<CreateEntity>(located_vehicle(), &ContextSpec::core_only())
            .await
            .expect_err("five positions exceed a budget of four");
        assert!(
            matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions { .. })),
            "{refusal:?}"
        );
        budgeted_recording_service(geometry_budget(5))
            .admit_remaining_entity::<CreateEntity>(located_vehicle(), &ContextSpec::core_only())
            .await
            .expect("five positions are within a budget of five");

        let expansion_failure = budgeted_recording_service(geometry_budget(4))
            .admit_remaining_entity::<CreateEntity>(located_vehicle(), &unloadable_context())
            .await
            .expect_err("the context does not load");
        assert!(matches!(expansion_failure, EntityError::JsonLd(_)), "{expansion_failure:?}");
    }

    /// Clause 4.5.2.3: a concise `GeoProperty` is its geometry alone, which
    /// the normalization wraps into the instance the census reads, so its
    /// positions are budgeted as a normalized one's are.
    #[tokio::test]
    async fn a_concise_geo_property_is_budgeted_after_normalization() {
        let concise = json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "location": {"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]}
        });
        let refusal = budgeted_recording_service(geometry_budget(4))
            .admit_remaining_entity::<CreateEntity>(concise.clone(), &ContextSpec::core_only()) // clone: the same payload is admitted twice
            .await
            .expect_err("five positions exceed a budget of four");
        assert!(
            matches!(refusal, EntityError::PayloadBudget(PayloadBudgetExceeded::GeometryPositions { .. })),
            "{refusal:?}"
        );
        budgeted_recording_service(geometry_budget(5))
            .admit_remaining_entity::<CreateEntity>(concise, &ContextSpec::core_only())
            .await
            .expect("five positions are within a budget of five");
    }

    /// Clause 6.3.2: a typed entity's geometry past the budget is refused
    /// before the expansion, so not even a context that cannot be loaded is
    /// reached, and the refusal names the attribute as the client wrote it.
    #[tokio::test]
    async fn a_typed_geometry_past_the_budget_is_refused_before_the_expansion() {
        let entity: Entity = serde_json::from_value(located_vehicle()).expect("a valid entity");
        let refusal = budgeted_recording_service(geometry_budget(4))
            .admit_remaining_entity::<CreateEntity>(&entity, &unloadable_context())
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
