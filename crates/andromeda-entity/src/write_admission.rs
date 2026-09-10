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
//! half of the write paths. [`admit_expanded_entity`] and
//! [`admit_expanded_attribute`] hold the rules that read the expanded payload,
//! and every write path in this crate reaches them; the transport layer runs
//! the same screening ahead of its forwarding phase through
//! [`EntityService::admit_entity_write`], [`EntityService::admit_attribute_write`]
//! and `EntityService::admit_batch_write`, which expand the payload the way the
//! write path would. What a client wrote before expansion
//! -- the attribute types of clause 5.2.4, the seals of clause 4.5.2.2 and the
//! `@context` clause 5.5.7 admits only at the document's top level -- is decided
//! on the compact payload, which is the transport layer's own half of the same
//! admission.
//!
//! [`EntityService::admit_write_context`] is the one rule here that reads the
//! request's `@context` instead of its payload: clause 5.5.7 refuses a user
//! `@context` declaring a JSON-LD Scoped Context, and that is a property of the
//! request rather than of any one entity in it.

use crate::{
    EntityService,
    attribute_envelope::validate_attribute_envelopes,
    batch::UpsertPolicy,
    error::EntityError,
    geo_property_geometry::validate_geo_property_geometries,
    null_sentinel_admission::{NullSentinelAdmission, reject_null_placements_in_attribute, reject_null_placements_in_entity},
    scope_admission::validate_scope_member,
};
use andromeda_jsonld::{attribute_fragment::expand_attribute_fragment, context_spec::ContextSpec, scoped_context_admission::reject_scoped_user_contexts};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_representation::normalization::{normalize_attribute_node, normalize_entity_attributes, normalize_merge_fragment};
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

    /// Bring a payload into the shape this operation's write path expands, so
    /// the screening reads what the write would store.
    fn normalize(self, payload: &mut Value) {
        match self {
            EntityWrite::Create | EntityWrite::AppendAttributes | EntityWrite::UpdateAttributes | EntityWrite::ReplaceEntity => {
                normalize_entity_attributes(payload);
            }
            EntityWrite::MergeEntity => normalize_merge_fragment(payload),
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

/// Screen one expanded entity-shaped payload under clause 5.5.4.
///
/// # Errors
///
/// Reports [`EntityError::Validation`], which the HTTP binding answers with the
/// `BadRequestData` of clause 5.5.4, for an attribute instance that is not one
/// its family admits, an NGSI-LD Null the operation does not admit, a `scope`
/// member outside clause 4.18, and a malformed `GeoProperty` geometry.
pub(crate) fn admit_expanded_entity(expanded: &Value, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    validate_attribute_envelopes(expanded)?;
    reject_null_placements_in_entity(expanded, admission)?;
    validate_scope_member(expanded, admission)?;
    validate_geo_property_geometries(expanded)
}

/// Screen one expanded bare Attribute Fragment (clauses 5.6.4 and 5.6.19)
/// under clause 5.5.4.
///
/// The `scope` member has no reading here: clauses 5.6.4.4 and 5.6.19.4 make it
/// no attribute a fragment can target, which the two write paths refuse by
/// name before they reach this.
///
/// # Errors
///
/// Reports [`EntityError::Validation`] on the same terms as
/// [`admit_expanded_entity`].
pub(crate) fn admit_expanded_attribute(expanded: &Value, attribute: &AttributeName, admission: NullSentinelAdmission) -> Result<(), EntityError> {
    validate_attribute_envelopes(expanded)?;
    reject_null_placements_in_attribute(expanded, attribute, admission)?;
    validate_geo_property_geometries(expanded)
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

    /// Run the clause 5.5.4 screening of `write` over the entity-shaped payload
    /// a client sent, before the operation forwards any of it.
    ///
    /// The payload is expanded here the way `write`'s own path expands it, so
    /// the screening reads the shape the write would store. A merge fragment is
    /// the one payload not screened in its final shape: Table 6.5.3.4-1's
    /// `format` and `lang` are applied against the stored entity later, and that
    /// rewrite neither introduces an NGSI-LD Null nor removes one.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when the payload cannot be expanded
    /// against `context`, and the [`EntityError::Validation`] of
    /// [`admit_expanded_entity`].
    pub async fn admit_entity_write(&self, write: EntityWrite, payload: &Value, context: &ContextSpec) -> Result<(), EntityError> {
        self.admit_write_context(context).await?;
        let mut payload = payload.clone(); // clone: expansion consumes the document, which the caller still has to forward and to write
        write.normalize(&mut payload);
        let expanded = self.engine.expand(payload, context).await?;
        admit_expanded_entity(&expanded, write.null_sentinel_admission())
    }

    /// Run the clause 5.5.4 screening of `write` over the bare Attribute
    /// Fragment of clauses 5.6.4 and 5.6.19, before the operation forwards it.
    ///
    /// # Errors
    ///
    /// Reports [`EntityError::JsonLd`] when the fragment cannot be expanded
    /// against `context`, and the [`EntityError::Validation`] of
    /// [`admit_expanded_attribute`].
    pub async fn admit_attribute_write(
        &self,
        write: AttributeWrite,
        attribute: &AttributeName,
        payload: &Value,
        context: &ContextSpec,
    ) -> Result<(), EntityError> {
        self.admit_write_context(context).await?;
        let mut payload = payload.clone(); // clone: expansion consumes the document, which the caller still has to forward and to write
        normalize_attribute_node(attribute.as_str(), &mut payload);
        let expanded = expand_attribute_fragment(&self.engine, attribute.as_str(), payload, context).await?;
        admit_expanded_attribute(&expanded, attribute, write.null_sentinel_admission())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        batch::UpsertPolicy,
        null_sentinel_admission::NullSentinelAdmission,
        write_admission::{AttributeWrite, BatchWrite, EntityWrite, admit_expanded_attribute, admit_expanded_entity},
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use serde_json::{Value, json};

    /// One expanded entity carrying a single Property whose value is `value`.
    fn entity_with(value: &Value) -> Value {
        json!([{
            "@id": "urn:ngsi-ld:Device:1",
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
            "https://uri.etsi.org/ngsi-ld/location": [{
                "@type": ["https://uri.etsi.org/ngsi-ld/GeoProperty"],
                "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": {"type": "Point", "coordinates": [1.0, 2.0, 3.0, 4.0]}}]
            }]
        }]);
        assert!(admit_expanded_entity(&geometry, EntityWrite::Create.null_sentinel_admission()).is_err());

        let scope = json!([{"@id": "urn:ngsi-ld:Device:1", "https://uri.etsi.org/ngsi-ld/scope": [{"@value": "/A B"}]}]);
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
}
