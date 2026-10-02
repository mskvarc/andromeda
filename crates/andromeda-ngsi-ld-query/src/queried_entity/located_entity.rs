//! What a geo-query reads of an entity (clause 4.10).

use andromeda_ngsi_ld_geometry::form::GeometryForm;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::borrow::Cow;

/// An entity as a geo-query reads it.
pub trait LocatedEntity<'a>: Copy {
    /// The geometry of every instance of the `GeoProperty` keyed by the
    /// expanded name `attribute`, default and `datasetId` instances alike, in
    /// the order the entity holds them.
    ///
    /// Borrowed where the entity holds the geometry as one, owned where it
    /// holds the clause 4.7.2 string form and has to parse it. An instance
    /// holding no geometry contributes none, and an entity not carrying the
    /// attribute yields nothing.
    fn geo_property_geometries(self, attribute: &AttributeName) -> impl Iterator<Item = Cow<'a, GeometryForm>>;
}
