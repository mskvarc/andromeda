//! The bounding box and the foreign members of a `GeoJSON` geometry a broker answers (RFC 7946 sections 5 and 6.1),
//! read through the typed model: a broker keeps both as written, in every representation that carries a geometry.

use ngsi_ld::model::{
    Attribute,
    Entity,
    Output,
    SimplifiedEntity,
    SimplifiedValue,
    geometry::{Geometry, Position},
};

/// The ordinates of `position` in the order a bounding box writes them: longitude, latitude, altitude when it has one.
fn ordinates(position: &Position) -> impl Iterator<Item = Option<f64>> + '_ {
    [Some(position.longitude()), Some(position.latitude()), position.altitude()]
        .into_iter()
        .flatten()
        .map(|number| number.to_string().parse::<f64>().ok())
}

/// The members of the `bbox` of `geometry` in the order of RFC 7946 section 5 (the south-west corner, then the
/// north-east corner), or `None` when it has none or an ordinate does not read as a double.
#[must_use]
pub fn bbox(geometry: &Geometry) -> Option<Vec<f64>> {
    let bbox = geometry.bbox()?;
    ordinates(bbox.south_west()).chain(ordinates(bbox.north_east())).collect()
}

/// The foreign member `name` of `geometry` (RFC 7946 section 6.1) as a JSON tree, or `None` when it has none by that
/// name.
#[must_use]
pub fn foreign_member(geometry: &Geometry, name: &str) -> Option<serde_json::Value> {
    geometry
        .foreign_members()
        .iter()
        .find(|(candidate, _)| candidate.to_string() == name)
        .and_then(|(_, member)| serde_json::from_str(&member.to_string()).ok())
}

/// The geometry of the `GeoProperty` `name` of `entity`, when its one instance holds one.
#[must_use]
pub fn geo_property_geometry<'e>(entity: &'e Entity<Output>, name: &str) -> Option<&'e Geometry> {
    entity
        .attributes
        .iter()
        .find(|(candidate, _)| candidate.as_str() == name)
        .and_then(|(_, instances)| match instances.first() {
            Attribute::GeoProperty(property) => property.value().as_geometry(),
            Attribute::Property(_)
            | Attribute::LanguageProperty(_)
            | Attribute::VocabProperty(_)
            | Attribute::ListProperty(_)
            | Attribute::JsonProperty(_)
            | Attribute::Relationship(_)
            | Attribute::ListRelationship(_) => None,
        })
}

/// The geometry the attribute `name` of a simplified `entity` reduces to, when it reduces to one.
#[must_use]
pub fn simplified_geometry<'e>(entity: &'e SimplifiedEntity, name: &str) -> Option<&'e Geometry> {
    entity
        .attributes
        .iter()
        .find(|(candidate, _)| candidate.as_str() == name)
        .and_then(|(_, value)| match value {
            SimplifiedValue::Geometry(geometry) => Some(geometry),
            SimplifiedValue::Value(_)
            | SimplifiedValue::LanguageMap(_)
            | SimplifiedValue::Vocab(_)
            | SimplifiedValue::Json(_)
            | SimplifiedValue::Dataset(_) => None,
        })
}

#[cfg(test)]
mod tests {
    use crate::geometry_members::{bbox, foreign_member};
    use googletest::prelude::*;
    use ngsi_ld::model::{
        RawJson,
        geometry::{BoundingBox, ForeignMemberName, Geometry, Position, Shape},
    };

    fn point() -> Result<Geometry> {
        let corner = Position::try_new(1.0, 2.0)?;
        let geometry = Geometry::from(Shape::Point(Position::try_new(1.0, 2.0)?)).with_bbox(BoundingBox::try_new(corner.clone(), corner)?)?;
        let note: RawJson = r#"{"year": 2024}"#.parse()?;
        Ok(geometry.with_foreign_member("note".parse::<ForeignMemberName>()?, note))
    }

    #[gtest]
    fn the_bbox_lists_the_south_west_corner_then_the_north_east_one() -> Result<()> {
        verify_that!(bbox(&point()?), some(elements_are![eq(&1.0), eq(&2.0), eq(&1.0), eq(&2.0)]))
    }

    #[gtest]
    fn a_geometry_without_a_bbox_has_none() -> Result<()> {
        verify_that!(bbox(&Geometry::from(Shape::Point(Position::try_new(1.0, 2.0)?))), none())
    }

    #[gtest]
    fn a_foreign_member_reads_as_json_by_its_name() -> Result<()> {
        verify_that!(foreign_member(&point()?, "note"), some(eq(&serde_json::json!({"year": 2024}))))
    }

    #[gtest]
    fn a_missing_foreign_member_reads_as_none() -> Result<()> {
        verify_that!(foreign_member(&point()?, "other"), none())
    }
}
