//! Entity-member group keys for `aggrGroupBy` (extension grammar §4.2).
//!
//! The grouping grammar the temporal aggregation grouping extension adds is
//! modelled on the clause 4.23.3 Entity Ordering Language: a `GroupKey` is
//! either a clause 4.9 `ValuePath` or one of a fixed set of structural entity
//! members. These members name parts of the entity or instance representation
//! that carry no attribute IRI, so — like `orderBy`'s `id`/`type`/`scope` — they
//! are read from their own columns rather than expanded against the `@context`.
//!
//! Two members go beyond the ordering vocabulary because grouping addresses the
//! temporal instance rather than the entity: `datasetId` (clause 4.5.6, the
//! per-instance dataset the instance belongs to) and `unitCode` (clause 4.5.2,
//! the per-instance unit that makes a Property carry mixed units). Grouping by
//! `unitCode` is the correctness fix the study is built around: it turns the
//! mixed-unit refusal into separate groups.

use strum::{Display, EnumString};

/// A structural entity or instance member usable as an `aggrGroupBy` key.
///
/// The camelCase spellings are the tokens a client writes: they cross the API
/// boundary as query text and match the member names the NGSI-LD representation
/// uses, so the casing follows the spec-facing rule rather than kebab-case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
#[strum(serialize_all = "camelCase")]
pub enum EntityMember {
    /// The entity identifier (clause 4.5.1). Degenerate within one entity's
    /// temporal representation, but legal.
    Id,
    /// The entity types (clause 4.5.1). Groups the attribute family.
    Type,
    /// The entity scopes (clause 4.18).
    Scope,
    /// The per-instance dataset identifier (clause 4.5.6).
    DatasetId,
    /// The per-instance unit code (clause 4.5.2).
    UnitCode,
}

impl EntityMember {
    /// Classify a bare, path-free key name as an entity member, if it is one.
    ///
    /// Mirrors `orderBy`'s keyword rule: a member is only a member when the key
    /// names it and nothing else, so `datasetId.foo` descends into an attribute
    /// spelled `datasetId` rather than naming the instance's dataset member.
    /// Returns `None` for any other name, which the caller then reads as a
    /// `ValuePath`.
    #[must_use]
    pub fn classify(name: &str) -> Option<EntityMember> {
        name.parse().ok()
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::grouping::entity_member::EntityMember;
    use pretty_assertions::assert_eq;
    use std::str::FromStr;

    #[test]
    fn every_member_token_round_trips_through_its_camel_case_spelling() {
        for (member, text) in [
            (EntityMember::Id, "id"),
            (EntityMember::Type, "type"),
            (EntityMember::Scope, "scope"),
            (EntityMember::DatasetId, "datasetId"),
            (EntityMember::UnitCode, "unitCode"),
        ] {
            assert_eq!(member.to_string(), text, "{member:?}");
            assert_eq!(EntityMember::from_str(text).expect("member parses"), member, "{text}");
            assert_eq!(EntityMember::classify(text), Some(member), "{text}");
        }
    }

    #[test]
    fn a_name_that_is_not_a_member_token_is_left_for_the_value_path() {
        for name in ["speed", "observedAt", "id ", "DATASETID", "unitcode"] {
            assert_eq!(EntityMember::classify(name), None, "{name}");
        }
    }
}
