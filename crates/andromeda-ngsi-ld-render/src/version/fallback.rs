//! The clause 4.3.6.8 fallback tables, as one typed rule set.
//!
//! Tables 4.3.6.8-1, -2 and -3 say what a payload written for an older
//! specification version leaves out or rewrites: Entity members a target
//! never defined, attribute families it never defined (generalized to the
//! family they specialize), the instances of a multi-attribute it could not
//! tell apart, and the members of a Property or Relationship instance it never
//! defined. Every rule is local to one entity, one attribute or one instance,
//! so whoever writes Entity data asks this table member by member, and the
//! thresholds live here once.

use crate::version::adapted_payload::AdaptedPayload;
use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion};

/// An Entity's own member that a fallback can leave out (Table 4.3.6.8-1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityMember {
    /// The Entity's `expiresAt`, introduced at 1.9.
    ExpiresAt,
    /// The Entity's `scope`, introduced at 1.4.
    Scope,
}

/// A member of a Property or Relationship instance that a fallback can leave
/// out (Tables 4.3.6.8-2 and -3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceMember {
    /// `datasetId`, introduced at 1.3.
    DatasetId,
    /// `observedAt`, introduced at 1.3.
    ObservedAt,
    /// A Property's `unitCode`, introduced at 1.3.
    UnitCode,
    /// `expiresAt`, introduced at 1.9.
    ExpiresAt,
    /// A Property's `valueType`, introduced at 1.9.
    ValueType,
    /// A Relationship's `objectType`, introduced at 1.8.
    ObjectType,
}

/// What a payload written for an older NGSI-LD version leaves out or rewrites
/// (clause 4.3.6.8): the target version, and which NGSI-LD Element the payload
/// is, since two rows read differently on the Temporal Evolution of an Entity
/// (see [`AdaptedPayload`]).
///
/// There is no fallback for a target at or above the version this broker
/// conforms to: such a payload is already authoritative.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VersionFallback {
    /// The version the payload is written for.
    target: SpecVersion,
    /// Which NGSI-LD Element the payload is.
    payload: AdaptedPayload,
}

impl VersionFallback {
    /// The fallback a payload of `payload` kind written for `target` takes, or
    /// `None` when `target` is the conformant version or newer.
    #[must_use]
    pub const fn new(target: SpecVersion, payload: AdaptedPayload) -> Option<VersionFallback> {
        if target.precedes(SpecVersion::CONFORMANT) {
            Some(VersionFallback { target, payload })
        } else {
            None
        }
    }

    /// The version the payload is written for.
    #[must_use]
    pub const fn target(self) -> SpecVersion {
        self.target
    }

    /// Which NGSI-LD Element the payload is.
    #[must_use]
    pub const fn payload(self) -> AdaptedPayload {
        self.payload
    }

    /// Whether the target predates `version`.
    const fn predates(self, version: SpecVersion) -> bool {
        self.target.precedes(version)
    }

    /// Whether an Entity keeps `member` (Table 4.3.6.8-1): `expiresAt` from
    /// 1.9, `scope` from 1.4.
    #[must_use]
    pub const fn keeps_entity_member(self, member: EntityMember) -> bool {
        match member {
            EntityMember::ExpiresAt => !self.predates(SpecVersion::V1_9),
            EntityMember::Scope => !self.predates(SpecVersion::V1_4),
        }
    }

    /// Whether an Entity's `type` is written as its first value alone (Table
    /// 4.3.6.8-1 note 1): a multi-valued `type` exists from 1.3 onwards.
    #[must_use]
    pub const fn writes_single_type(self) -> bool {
        self.predates(SpecVersion::V1_3)
    }

    /// Whether the instances of a multi-attribute are written as one (Table
    /// 4.3.6.8-1 notes 2 and 3): instances separated by `datasetId` exist from
    /// 1.3 onwards, and an older target receives one of them, "with preference
    /// to the default instance". A temporal sequence of instances is no such
    /// set and is never collapsed.
    #[must_use]
    pub const fn collapses_instances(self) -> bool {
        self.predates(SpecVersion::V1_3) && self.payload.collapses_instances()
    }

    /// The family an instance of `family` is written as (Table 4.3.6.8-1):
    /// itself, or, when the target predates the version that introduced it,
    /// the family it specializes -- every Property subclass a Property, and a
    /// `ListRelationship` a Relationship.
    #[must_use]
    pub const fn family(self, family: AttributeType) -> AttributeType {
        match family {
            AttributeType::LanguageProperty if self.predates(SpecVersion::V1_4) => AttributeType::Property,
            AttributeType::VocabProperty | AttributeType::JsonProperty | AttributeType::ListProperty if self.predates(SpecVersion::V1_8) => {
                AttributeType::Property
            }
            AttributeType::ListRelationship if self.predates(SpecVersion::V1_8) => AttributeType::Relationship,
            AttributeType::Property
            | AttributeType::GeoProperty
            | AttributeType::Relationship
            | AttributeType::LanguageProperty
            | AttributeType::VocabProperty
            | AttributeType::JsonProperty
            | AttributeType::ListProperty
            | AttributeType::ListRelationship => family,
        }
    }

    /// Whether an instance written as `family` keeps `member`: Table 4.3.6.8-3
    /// for the Relationship families, Table 4.3.6.8-2 for every other one.
    ///
    /// `observedAt` is the one row that does not reach a temporal instance:
    /// clause 4.5.7 makes it the Temporal Property placing the instance in
    /// time, so removing it would leave no temporal representation at all.
    #[must_use]
    pub const fn keeps_instance_member(self, family: AttributeType, member: InstanceMember) -> bool {
        let relationship = matches!(family, AttributeType::Relationship | AttributeType::ListRelationship);
        match member {
            InstanceMember::DatasetId => !self.predates(SpecVersion::V1_3),
            InstanceMember::ObservedAt => !(self.predates(SpecVersion::V1_3) && self.payload.removes_observed_at()),
            InstanceMember::UnitCode => relationship || !self.predates(SpecVersion::V1_3),
            InstanceMember::ExpiresAt => !self.predates(SpecVersion::V1_9),
            InstanceMember::ValueType => relationship || !self.predates(SpecVersion::V1_9),
            InstanceMember::ObjectType => !relationship || !self.predates(SpecVersion::V1_8),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::version::{
        adapted_payload::AdaptedPayload,
        fallback::{EntityMember, InstanceMember, VersionFallback},
    };
    use andromeda_ngsi_ld_lexicon::{attribute_type::AttributeType, spec_version::SpecVersion};
    use pretty_assertions::assert_eq;

    /// The fallback of an Entity payload written for `target`.
    fn entity(target: SpecVersion) -> VersionFallback {
        VersionFallback::new(target, AdaptedPayload::Entity).expect("a target below the conformant version")
    }

    #[test]
    fn no_fallback_at_or_above_the_conformant_version() {
        assert_eq!(VersionFallback::new(SpecVersion::V1_9, AdaptedPayload::Entity), None);
        assert_eq!(VersionFallback::new(SpecVersion::new(2, 0), AdaptedPayload::Entity), None);
        assert!(VersionFallback::new(SpecVersion::V1_8, AdaptedPayload::Entity).is_some());
    }

    #[test]
    fn entity_expires_at_is_dropped_below_1_9() {
        assert!(!entity(SpecVersion::V1_8).keeps_entity_member(EntityMember::ExpiresAt));
        assert!(!entity(SpecVersion::V1_0).keeps_entity_member(EntityMember::ExpiresAt));
    }

    #[test]
    fn entity_scope_is_dropped_below_1_4() {
        assert!(!entity(SpecVersion::V1_3).keeps_entity_member(EntityMember::Scope));
        assert!(entity(SpecVersion::V1_4).keeps_entity_member(EntityMember::Scope));
    }

    #[test]
    fn a_single_type_is_written_below_1_3() {
        assert!(entity(SpecVersion::V1_0).writes_single_type());
        assert!(!entity(SpecVersion::V1_3).writes_single_type());
    }

    #[test]
    fn instances_collapse_below_1_3_except_for_a_temporal_evolution() {
        let at_1_0 = |payload| VersionFallback::new(SpecVersion::V1_0, payload).map(VersionFallback::collapses_instances);
        assert_eq!(at_1_0(AdaptedPayload::Entity), Some(true));
        assert_eq!(at_1_0(AdaptedPayload::SimplifiedTemporalEvolution), Some(true));
        assert_eq!(at_1_0(AdaptedPayload::TemporalEvolution), Some(false));
        assert!(!entity(SpecVersion::V1_3).collapses_instances());
    }

    #[test]
    fn families_generalize_below_their_introduction() {
        let written = |target, family| entity(target).family(family);
        for target in [SpecVersion::V1_0, SpecVersion::V1_3] {
            assert_eq!(written(target, AttributeType::LanguageProperty), AttributeType::Property);
        }
        assert_eq!(written(SpecVersion::V1_4, AttributeType::LanguageProperty), AttributeType::LanguageProperty);
        for family in [AttributeType::VocabProperty, AttributeType::JsonProperty, AttributeType::ListProperty] {
            for target in [SpecVersion::V1_0, SpecVersion::V1_3, SpecVersion::V1_4] {
                assert_eq!(written(target, family), AttributeType::Property, "{family:?} at {target}");
            }
            assert_eq!(written(SpecVersion::V1_8, family), family);
        }
        assert_eq!(written(SpecVersion::V1_4, AttributeType::ListRelationship), AttributeType::Relationship);
        assert_eq!(written(SpecVersion::V1_8, AttributeType::ListRelationship), AttributeType::ListRelationship);
        for family in [AttributeType::Property, AttributeType::GeoProperty, AttributeType::Relationship] {
            for target in [SpecVersion::V1_0, SpecVersion::V1_3, SpecVersion::V1_4, SpecVersion::V1_8] {
                assert_eq!(written(target, family), family);
            }
        }
    }

    #[test]
    fn property_members_follow_table_4_3_6_8_2() {
        let keeps = |target, member| entity(target).keeps_instance_member(AttributeType::Property, member);
        for member in [InstanceMember::DatasetId, InstanceMember::ObservedAt, InstanceMember::UnitCode] {
            assert!(!keeps(SpecVersion::V1_0, member), "{member:?} at 1.0");
            assert!(keeps(SpecVersion::V1_3, member), "{member:?} at 1.3");
        }
        for member in [InstanceMember::ExpiresAt, InstanceMember::ValueType] {
            assert!(!keeps(SpecVersion::V1_8, member), "{member:?} at 1.8");
        }
        assert!(keeps(SpecVersion::V1_0, InstanceMember::ObjectType), "objectType is no Property member");
    }

    #[test]
    fn relationship_members_follow_table_4_3_6_8_3() {
        let keeps = |target, member| entity(target).keeps_instance_member(AttributeType::Relationship, member);
        for member in [InstanceMember::DatasetId, InstanceMember::ObservedAt] {
            assert!(!keeps(SpecVersion::V1_0, member), "{member:?} at 1.0");
            assert!(keeps(SpecVersion::V1_3, member), "{member:?} at 1.3");
        }
        assert!(!keeps(SpecVersion::V1_4, InstanceMember::ObjectType));
        assert!(keeps(SpecVersion::V1_8, InstanceMember::ObjectType));
        assert!(!keeps(SpecVersion::V1_8, InstanceMember::ExpiresAt));
        assert!(keeps(SpecVersion::V1_0, InstanceMember::UnitCode), "unitCode is no Relationship member");
        assert!(
            entity(SpecVersion::V1_4).keeps_instance_member(AttributeType::ListRelationship, InstanceMember::ValueType),
            "valueType is no ListRelationship member"
        );
    }

    #[test]
    fn a_temporal_evolution_keeps_observed_at() {
        let temporal = VersionFallback::new(SpecVersion::V1_0, AdaptedPayload::TemporalEvolution).expect("a fallback");
        assert!(temporal.keeps_instance_member(AttributeType::Property, InstanceMember::ObservedAt));
        assert!(!temporal.keeps_instance_member(AttributeType::Property, InstanceMember::DatasetId));
    }

    #[test]
    fn an_unsnapped_target_compares_by_order() {
        let at_1_5 = entity(SpecVersion::new(1, 5));
        let at_1_4 = entity(SpecVersion::V1_4);
        assert_eq!(at_1_5.target(), SpecVersion::new(1, 5));
        assert_eq!(at_1_5.keeps_entity_member(EntityMember::Scope), at_1_4.keeps_entity_member(EntityMember::Scope));
        assert_eq!(at_1_5.family(AttributeType::LanguageProperty), at_1_4.family(AttributeType::LanguageProperty));
        assert_eq!(at_1_5.family(AttributeType::VocabProperty), at_1_4.family(AttributeType::VocabProperty));
        assert_eq!(
            at_1_5.keeps_instance_member(AttributeType::Relationship, InstanceMember::ObjectType),
            at_1_4.keeps_instance_member(AttributeType::Relationship, InstanceMember::ObjectType)
        );
    }
}
