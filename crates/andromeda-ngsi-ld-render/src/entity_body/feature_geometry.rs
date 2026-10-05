//! The clause 4.5.16.1 geometry of a `GeoJSON` Feature, selected from the typed
//! entity.
//!
//! [`GeometrySelection`] states the rule over a rendered document: the
//! `GeoProperty` named by `geometryProperty` (else `location`), and of its
//! instances the one a `datasetId` names or the default one, read in whichever
//! representation the Feature's `properties` take. A `GeoProperty` -- the one
//! attribute that can hold a geometry of millions of positions -- is selected
//! here from the typed instances, in the same representation, and its geometry
//! is written from its positions. Any other member is rendered on its own and
//! handed to the document rule, which is exact by construction and costs no
//! more than that one member.

use crate::{
    entity_body::typed_entity::TypedEntity,
    geometry_selection::{GeometrySelection, selects},
};
use andromeda_ngsi_ld_geometry::geometry::Geometry;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::attribute::{
    common_fields::CommonAttributeFields,
    geo_property::GeoProperty,
    instance::{Attribute, AttributeValue},
};
use andromeda_ngsi_ld_query::dataset::DatasetSelector;
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, compact_to_expanded};
use geojson::Geometry as GeoJsonGeometry;
use serde::{Serialize, Serializer};
use serde_json::Map;

/// The geometry a Feature carries.
#[derive(Debug)]
pub(crate) enum FeatureGeometry<'a> {
    /// A `GeoProperty` geometry, written from its positions.
    Typed(&'a Geometry),
    /// A geometry read out of a rendered member.
    Written(GeoJsonGeometry),
    /// No geometry: clause 4.5.16.1 writes it as `null`.
    Undefined,
}

impl Serialize for FeatureGeometry<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            FeatureGeometry::Typed(geometry) => geometry.serialize(serializer),
            FeatureGeometry::Written(geometry) => geometry.serialize(serializer),
            FeatureGeometry::Undefined => serializer.serialize_none(),
        }
    }
}

/// How the instances of the selected member are laid out in the Feature's
/// `properties`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InstanceLayout {
    /// Clause 4.5.5: an array of instances, each naming its own `datasetId`.
    Reified,
    /// Clause 4.5.4: one bare value, or a `dataset` map keyed by `datasetId`.
    Simplified,
}

/// Select the geometry of `entity` by `selection`.
///
/// # Errors
///
/// Returns the failure of rendering the selected member on its own, for a
/// member that is no plain `GeoProperty`.
pub(crate) fn feature_geometry<'a>(entity: &TypedEntity<'a>, selection: GeometrySelection<'_>) -> Result<FeatureGeometry<'a>, serde_json::Error> {
    let named = selection.property_name();
    let located = entity
        .entity
        .attribute_members()
        .find(|(name, _)| *name == named)
        .or_else(|| compact_to_expanded(named).and_then(|expanded| entity.entity.attribute_members().find(|(name, _)| *name == expanded)))
        .map(|(_, member)| member);
    let Some(member) = located else {
        return Ok(FeatureGeometry::Undefined);
    };
    let layout = if entity.format.is_simplified() {
        InstanceLayout::Simplified
    } else {
        InstanceLayout::Reified
    };
    match plain_geo_properties(member) {
        Some(PlainGeoProperties::Single(geo_property)) => Ok(sole_instance(geo_property, layout, selection.dataset_ids())),
        Some(PlainGeoProperties::Several(instances)) => Ok(selected_instance(&instances, layout, selection.dataset_ids())),
        None => rendered_member_geometry(entity, member, named, selection),
    }
}

/// The `GeoProperty` instances a member holds, when it holds nothing else and
/// none of them carries a sub-attribute the clause 4.5.4 `dataset` map could
/// be mistaken for.
enum PlainGeoProperties<'a> {
    /// The one instance of a single attribute.
    Single(&'a GeoProperty),
    /// The instances of a multi-attribute, in the order the entity holds them.
    Several(Vec<&'a GeoProperty>),
}

/// The plain `GeoProperty` instances of `member`, if that is all it holds.
fn plain_geo_properties(member: &AttributeValue) -> Option<PlainGeoProperties<'_>> {
    match member {
        AttributeValue::Single(attribute) => plain_geo_property(attribute).map(PlainGeoProperties::Single),
        AttributeValue::Multiple(instances) => instances
            .iter()
            .map(plain_geo_property)
            .collect::<Option<Vec<_>>>()
            .map(PlainGeoProperties::Several),
        AttributeValue::Raw(_) => None,
    }
}

/// The instance as a `GeoProperty`, if it is one carrying no `dataset` member.
fn plain_geo_property(attribute: &Attribute) -> Option<&GeoProperty> {
    match attribute {
        Attribute::GeoProperty(geo_property) if !carries_dataset_member(&geo_property.common) => Some(geo_property),
        Attribute::GeoProperty(_)
        | Attribute::Property(_)
        | Attribute::Relationship(_)
        | Attribute::VocabProperty(_)
        | Attribute::LanguageProperty(_)
        | Attribute::JsonProperty(_)
        | Attribute::ListProperty(_)
        | Attribute::ListRelationship(_) => None,
    }
}

/// Whether an instance carries a sub-attribute named `dataset`.
fn carries_dataset_member(common: &CommonAttributeFields) -> bool {
    common.sub_attributes.contains_key(compact_properties::DATASET)
}

/// The geometry of a single attribute: its one instance, unless a `datasetId`
/// was asked for that the rendered instance names otherwise.
///
/// The simplified rendering of a single instance is its bare geometry, which
/// names no `datasetId` at all.
fn sole_instance<'a>(geo_property: &'a GeoProperty, layout: InstanceLayout, selectors: &[DatasetSelector]) -> FeatureGeometry<'a> {
    let rendered_dataset_id = match layout {
        InstanceLayout::Reified => geo_property.common.dataset_id.as_ref(),
        InstanceLayout::Simplified => None,
    };
    match rendered_dataset_id {
        Some(dataset_id) if !selectors.is_empty() => {
            if selectors.iter().any(|selector| selects(selector, Some(dataset_id.as_str()))) {
                FeatureGeometry::Typed(&geo_property.value)
            } else {
                FeatureGeometry::Undefined
            }
        }
        Some(_) | None => FeatureGeometry::Typed(&geo_property.value),
    }
}

/// The geometry of a multi-attribute: the instance a `datasetId` names, or the
/// default one when none was asked for.
///
/// The reified array is searched front to back; the simplified `dataset` map
/// keeps, for each key, the instance written last.
fn selected_instance<'a>(instances: &[&'a GeoProperty], layout: InstanceLayout, selectors: &[DatasetSelector]) -> FeatureGeometry<'a> {
    let named = |selector: &DatasetSelector| {
        let carries = |instance: &&&'a GeoProperty| selects(selector, instance.common.dataset_id.as_ref().map(Identifier::as_str));
        match layout {
            InstanceLayout::Reified => instances.iter().find(carries),
            InstanceLayout::Simplified => instances.iter().rev().find(carries),
        }
    };
    let selected = if selectors.is_empty() {
        named(&DatasetSelector::Default)
    } else {
        selectors.iter().find_map(named)
    };
    selected.map_or(FeatureGeometry::Undefined, |instance| FeatureGeometry::Typed(&instance.value))
}

/// The geometry of a member that is no plain `GeoProperty`: the member is
/// rendered on its own, as the Feature's `properties` hold it, and the
/// document rule selects from it.
fn rendered_member_geometry<'a>(
    entity: &TypedEntity<'a>,
    member: &'a AttributeValue,
    named: &str,
    selection: GeometrySelection<'_>,
) -> Result<FeatureGeometry<'a>, serde_json::Error> {
    let rendered = serde_json::to_value(entity.attribute_member(member))?;
    let mut alone = Map::with_capacity(1);
    alone.insert(named.to_owned(), rendered);
    Ok(selection
        .geometry(&alone)
        .and_then(|geometry| serde_json::from_value(geometry).ok())
        .map_or(FeatureGeometry::Undefined, FeatureGeometry::Written))
}
