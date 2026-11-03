//! Refusing a typed write whose geometry is past the position budget before
//! it is expanded.
//!
//! ETSI GS CIM 009 v1.9.1 clause 6.3.2 answers a request whose input "was too
//! large" with 413 `RequestEntityTooLarge`. A typed compact Entity or Entity
//! fragment already holds each `GeoProperty` geometry as its positions, so the
//! position budget is read off it before the expansion copies a geometry into
//! the written document: an oversized geometry costs its decoding and nothing
//! after it.

use andromeda_ngsi_ld_expansion::written_member::{WrittenMember, WrittenMemberRef};
use andromeda_ngsi_ld_geometry::{form::GeometryForm, geometry_census::GeometryPositionLimit};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_model::attribute::{
    geo_property_value::GeoPropertyValue,
    instance::{Attribute, AttributeValue},
    merge_member::MergeMember,
    merge_value::MergeValue,
    patched::Patched,
};
use andromeda_ngsi_ld_representation::payload_budget::{GeometryOwner, PayloadBudgetExceeded};
use std::str::FromStr;

/// Admit the typed attribute members of a compact write whose `GeoProperty`
/// geometries, values, previous values, and those of sub-attributes at any
/// depth, each hold no more positions than `limit`.
///
/// # Errors
///
/// Returns [`PayloadBudgetExceeded::GeometryPositions`] naming the first
/// attribute, as written, whose geometry is past `limit`.
pub fn admit_compact_geometries<'a, M: WrittenMember + 'a>(
    limit: GeometryPositionLimit,
    members: impl IntoIterator<Item = (&'a str, &'a M)>,
) -> Result<(), PayloadBudgetExceeded> {
    members.into_iter().try_for_each(|(name, member)| match member.written() {
        WrittenMemberRef::Attribute(member) => admit_member(limit, name, member),
        WrittenMemberRef::Merge(member) => admit_merge_member(limit, name, member),
    })
}

/// Admit every instance of the attribute member `name`.
fn admit_member(limit: GeometryPositionLimit, name: &str, member: &AttributeValue) -> Result<(), PayloadBudgetExceeded> {
    member.instances().try_for_each(|instance| {
        if let Attribute::GeoProperty(geo_property) = instance {
            [Some(&geo_property.value), geo_property.previous_value.as_ref()]
                .into_iter()
                .flatten()
                .try_for_each(|value| admit_value(limit, name, value))?;
        }
        instance
            .common()
            .sub_attributes
            .iter()
            .try_for_each(|(sub_name, sub_member)| admit_member(limit, sub_name.as_str(), sub_member))
    })
}

/// Admit every instance of the merge member `name`.
fn admit_merge_member(limit: GeometryPositionLimit, name: &str, member: &MergeMember) -> Result<(), PayloadBudgetExceeded> {
    member.instances().try_for_each(|instance| {
        if let Some(Patched::Set(MergeValue::GeoProperty(value))) = &instance.value {
            admit_value(limit, name, value)?;
        }
        instance
            .sub_attributes
            .iter()
            .try_for_each(|(sub_name, sub_member)| admit_merge_member(limit, sub_name.as_str(), sub_member))
    })
}

/// Admit the `GeoProperty` value of the attribute `name`: a geometry read
/// past the position limit is already known to hold too many, and any other
/// is counted.
fn admit_value(limit: GeometryPositionLimit, name: &str, value: &GeoPropertyValue) -> Result<(), PayloadBudgetExceeded> {
    match value {
        GeoPropertyValue::Oversized(_) => Err(refusal(limit, name)),
        GeoPropertyValue::Geometry(_) | GeoPropertyValue::Encoded(_) | GeoPropertyValue::Null => {
            value.form().map_or(Ok(()), |form| admit_form(limit, name, form))
        }
    }
}

/// Admit one geometry of the attribute `name`.
fn admit_form(limit: GeometryPositionLimit, name: &str, form: &GeometryForm) -> Result<(), PayloadBudgetExceeded> {
    if limit.admits(form.position_count()) {
        return Ok(());
    }
    Err(refusal(limit, name))
}

/// The refusal of a geometry of the attribute `name` past `limit`.
fn refusal(limit: GeometryPositionLimit, name: &str) -> PayloadBudgetExceeded {
    PayloadBudgetExceeded::GeometryPositions {
        owner: AttributeName::from_str(name).map_or(GeometryOwner::Unnamed, GeometryOwner::Attribute),
        limit,
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::compact_geometry_admission::admit_compact_geometries;
    use andromeda_ngsi_ld_geometry::geometry_census::GeometryPositionLimit;
    use andromeda_ngsi_ld_model::{
        attribute::merge_member::MergeMember,
        entity::{Entity, EntityPatch},
        request_body::{DecodeBounds, RequestBody},
    };
    use andromeda_ngsi_ld_number::json_value::value_census::JsonValueLimit;
    use andromeda_ngsi_ld_representation::{
        compact_node::context_reading::ContextReading,
        payload_budget::{GeometryOwner, PayloadBudgetExceeded},
    };
    use serde_json::{Value, json};
    use std::num::NonZeroUsize;

    fn limit(positions: usize) -> GeometryPositionLimit {
        GeometryPositionLimit::new(NonZeroUsize::new(positions).expect("a positive limit"))
    }

    fn bounds(positions: usize) -> DecodeBounds {
        DecodeBounds::new(limit(positions), JsonValueLimit::new(NonZeroUsize::new(1_000_000).expect("a positive limit")))
    }

    /// A Polygon of one closed ring of five positions.
    fn square() -> Value {
        json!({"type": "Polygon", "coordinates": [[[0, 0], [1, 0], [1, 1], [0, 1], [0, 0]]]})
    }

    fn entity(written: Value) -> Entity {
        serde_json::from_value(written).expect("a valid entity")
    }

    fn owner(refusal: PayloadBudgetExceeded) -> String {
        match refusal {
            PayloadBudgetExceeded::GeometryPositions {
                owner: GeometryOwner::Attribute(name),
                ..
            } => name.to_string(),
            other @ (PayloadBudgetExceeded::GeometryPositions { .. } | PayloadBudgetExceeded::EntityJsonValues { .. }) => {
                panic!("not a named geometry refusal: {other:?}")
            }
        }
    }

    /// Clause 6.3.2: a `location` past the limit is refused naming the
    /// attribute as written; one at the limit is admitted.
    #[test]
    fn a_location_past_the_limit_is_refused_by_its_written_name() {
        let located = entity(json!({"id": "urn:ngsi-ld:Area:1", "type": "Area", "location": {"type": "GeoProperty", "value": square()}}));
        let refusal = admit_compact_geometries(limit(4), located.attribute_members()).expect_err("five positions exceed four");
        pretty_assertions::assert_eq!(owner(refusal), "location");
        admit_compact_geometries(limit(5), located.attribute_members()).expect("five positions are within five");
    }

    /// Every instance and every sub-attribute geometry is counted, each on
    /// its own.
    #[test]
    fn a_sub_attribute_geometry_and_every_instance_are_counted() {
        let nested = entity(json!({
            "id": "urn:ngsi-ld:Area:1",
            "type": "Area",
            "site": {
                "type": "Property",
                "value": 1,
                "footprint": {"type": "GeoProperty", "value": square()}
            }
        }));
        pretty_assertions::assert_eq!(
            owner(admit_compact_geometries(limit(4), nested.attribute_members()).expect_err("the footprint is over")),
            "footprint"
        );
        let several = entity(json!({
            "id": "urn:ngsi-ld:Area:1",
            "type": "Area",
            "zone": [
                {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [1, 2]}},
                {"type": "GeoProperty", "datasetId": "urn:ngsi-ld:d:1", "value": square()}
            ]
        }));
        assert!(admit_compact_geometries(limit(4), several.attribute_members()).is_err());
        admit_compact_geometries(limit(5), several.attribute_members()).expect("each geometry is within five");
    }

    /// A Property whose value merely looks like a geometry holds no geometry.
    #[test]
    fn a_property_value_is_no_geometry() {
        let valued =
            entity(json!({"id": "urn:ngsi-ld:Area:1", "type": "Area", "shape": {"type": "Property", "value": {"kind": "square", "side": [1, 1, 1, 1, 1, 1]}}}));
        admit_compact_geometries(limit(1), valued.attribute_members()).expect("no geometry is counted");
    }

    /// An entity fragment is counted as an entity is.
    #[test]
    fn a_fragment_is_counted_as_an_entity_is() {
        let patch: EntityPatch = serde_json::from_value(json!({"observationSpace": {"type": "GeoProperty", "value": square()}})).expect("a valid fragment");
        pretty_assertions::assert_eq!(
            owner(admit_compact_geometries(limit(4), patch.attribute_members()).expect_err("over")),
            "observationSpace"
        );
    }

    /// A merge fragment is counted as an entity is: a concise `GeoProperty`,
    /// and one nested as a sub-attribute of an instance that leaves its own
    /// value out (clause 5.5.12).
    #[test]
    fn a_merge_fragment_is_counted_as_an_entity_is() {
        let fragment: EntityPatch<MergeMember> = serde_json::from_value(json!({
            "location": square(),
            "site": {"type": "Property", "footprint": {"type": "GeoProperty", "value": square()}}
        }))
        .expect("a valid merge fragment");
        pretty_assertions::assert_eq!(
            owner(admit_compact_geometries(limit(4), fragment.attribute_members()).expect_err("over")),
            "footprint"
        );
        admit_compact_geometries(limit(5), fragment.attribute_members()).expect("each geometry is within five");
    }
    /// A geometry the decoding read past the limit is refused by its written
    /// name without its coordinates being counted.
    #[test]
    fn a_geometry_read_past_the_limit_is_refused_by_its_written_name() {
        let written = json!({"id": "urn:ngsi-ld:Area:1", "type": "Area", "location": {"type": "GeoProperty", "value": square()}});
        let text = written.to_string();
        let located =
            <Entity as RequestBody>::from_bounded_request_text(text.as_bytes(), ContextReading::keyword_spellings(), bounds(1)).expect("the body decodes");
        let refusal = admit_compact_geometries(limit(1), located.attribute_members()).expect_err("read past the limit");
        pretty_assertions::assert_eq!(owner(refusal), "location");
    }
}
