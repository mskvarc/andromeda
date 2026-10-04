//! Compacting a geometry node of the `GeoJSON` vocabulary (clause 4.7).
//!
//! Expansion keeps a `GeoProperty` value as a node typed with its `GeoJSON`
//! geometry type, its `coordinates` a list of lists down to the ordinates.
//! Under the core context's `coordinates` term, whose container is `@list`,
//! compaction gives back the `GeoJSON` object: the type token and the nested
//! coordinate arrays. The walk writes that object from the positions directly.
//!
//! When the compact model reads the geometry as a typed `Geometry` anyway,
//! writing millions of positions only for them to be read back is waste: a
//! geometry whose compacted type is its own `GeoJSON` token is then kept as
//! the typed geometry it is (see [`Compacted::Geometry`]). Every other
//! geometry, and any geometry of a context that redefines `coordinates`, is
//! written in full: so is a geometry carrying a `bbox` (IETF RFC 7946 section
//! 5, written under the core context's `bbox` `@list` term) or foreign members
//! (section 6.1, compacted as the members of any node), which the compact
//! model then reads back with them (clause 4.6.4).

use crate::{
    compacted::{Compacted, CompactedObject},
    expanded_item::ExpandedItem,
    reserved_terms::reserved,
    undecidable::Undecidable,
    walk::{GeometryOutput, Walk, member_term},
};
use andromeda_ngsi_ld_entity::{geometry_value::GeometryValue, value_mode::Stored};
use andromeda_ngsi_ld_geometry::{
    bounding_box::BoundingBox,
    form::{GeometryForm, PolygonRings},
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::Position,
};
use andromeda_ngsi_ld_number::float_spelling::canonical_float_text;
use andromeda_ngsi_ld_vocabulary::compact::properties as compact_properties;
use jsonld::{
    ContainerKind,
    IriBuf,
    Value,
    compaction::{ValueHint, ValueKind},
    object::Literal,
    syntax::NumberBuf,
};
use std::slice::from_ref;

/// The `GeoJSON` member naming a geometry's type (IETF RFC 7946 section 3.1).
const GEOJSON_TYPE: &str = "type";

impl Walk<'_> {
    /// Compact one geometry node.
    pub(crate) fn compact_geometry<'a>(&mut self, value: &'a GeometryValue) -> Result<Compacted<'a>, Undecidable> {
        let form = value.form();
        self.admit_coordinates_term(form)?;
        let mut result = CompactedObject::new();
        self.write_identity(&mut result, None, from_ref(reserved().geometry_type(form.geometry_type())))?;
        let Some(members) = value.members() else {
            if self.geometries == GeometryOutput::Typed && reads_as_geometry(&result, form) {
                return Ok(Compacted::Geometry(form));
            }
            result.insert(compact_properties::COORDINATES, self.coordinates(form)?);
            return Ok(Compacted::Object(result));
        };
        result.insert(compact_properties::COORDINATES, self.coordinates(form)?);
        if let Some(bbox) = members.bbox() {
            result.insert(compact_properties::BBOX, self.bounding_box(bbox)?);
        }
        for (name, values) in members.foreign() {
            let property = member_term(name);
            let items: Vec<ExpandedItem<'a, Stored>> = ExpandedItem::of_values(values)?;
            self.compact_property(&mut result, &property, &items)?;
        }
        Ok(Compacted::Object(result))
    }

    /// The ordinates of a `bbox`, each compacted under `bbox`, refusing a
    /// context in which `bbox` does not compact to the core context's `@list`
    /// term the walk writes the array for.
    fn bounding_box<'a>(&self, bbox: &BoundingBox) -> Result<Compacted<'a>, Undecidable> {
        let items = [ValueKind::Literal { datatype: None }];
        let key = self.terms.property_key(&reserved().bbox, ValueHint::new(ValueKind::List(&items)))?;
        match key {
            Some(key) if &*key == compact_properties::BBOX && self.terms.container(&key).contains(ContainerKind::List) => {}
            Some(_) | None => return Err(Undecidable::GeometryTerms),
        }
        bbox.ordinates()
            .map(|ordinate| self.ordinate(ordinate, compact_properties::BBOX))
            .collect::<Result<_, _>>()
            .map(Compacted::Array)
    }

    /// Refuse a context in which `coordinates` does not compact to the core
    /// context's `@list` term, whose arrays the walk writes.
    fn admit_coordinates_term(&self, form: &GeometryForm) -> Result<(), Undecidable> {
        // The list hint reads only whether the items are value objects, which
        // the first item tells for all: ordinates for a Point, lists below.
        let item = match form {
            GeometryForm::Point(_) => ValueKind::Literal { datatype: None },
            GeometryForm::MultiPoint(_)
            | GeometryForm::LineString(_)
            | GeometryForm::MultiLineString(_)
            | GeometryForm::Polygon(_)
            | GeometryForm::MultiPolygon(_) => ValueKind::List(&[]),
        };
        let items = [item];
        let key = self.terms.property_key(&reserved().coordinates, ValueHint::new(ValueKind::List(&items)))?;
        match key {
            Some(key) if &*key == compact_properties::COORDINATES && self.terms.container(&key).contains(ContainerKind::List) => Ok(()),
            Some(_) | None => Err(Undecidable::GeometryTerms),
        }
    }

    /// The coordinate arrays of a geometry, each ordinate compacted under
    /// `coordinates`.
    fn coordinates<'a>(&self, form: &GeometryForm) -> Result<Compacted<'a>, Undecidable> {
        match form {
            GeometryForm::Point(position) => self.position(position),
            GeometryForm::MultiPoint(positions) => self.positions(positions),
            GeometryForm::LineString(line) => self.line(line),
            GeometryForm::MultiLineString(lines) => lines.iter().map(|line| self.line(line)).collect::<Result<_, _>>().map(Compacted::Array),
            GeometryForm::Polygon(rings) => self.rings(rings),
            GeometryForm::MultiPolygon(polygons) => polygons.iter().map(|rings| self.rings(rings)).collect::<Result<_, _>>().map(Compacted::Array),
        }
    }

    /// The rings of one Polygon.
    fn rings<'a>(&self, rings: &PolygonRings) -> Result<Compacted<'a>, Undecidable> {
        rings.rings().iter().map(|ring| self.ring(ring)).collect::<Result<_, _>>().map(Compacted::Array)
    }

    /// One ring.
    fn ring<'a>(&self, ring: &LinearRing) -> Result<Compacted<'a>, Undecidable> {
        self.positions(ring.positions())
    }

    /// One line.
    fn line<'a>(&self, line: &LineStringCoordinates) -> Result<Compacted<'a>, Undecidable> {
        self.positions(line.positions())
    }

    /// A run of positions.
    fn positions<'a>(&self, positions: &[Position]) -> Result<Compacted<'a>, Undecidable> {
        positions
            .iter()
            .map(|position| self.position(position))
            .collect::<Result<_, _>>()
            .map(Compacted::Array)
    }

    /// One position: its two or three ordinates.
    fn position<'a>(&self, position: &Position) -> Result<Compacted<'a>, Undecidable> {
        [Some(position.longitude()), Some(position.latitude()), position.altitude()]
            .into_iter()
            .flatten()
            .map(|ordinate| self.ordinate(ordinate, compact_properties::COORDINATES))
            .collect::<Result<_, _>>()
            .map(Compacted::Array)
    }

    /// One ordinate, in the canonical spelling the codec stores it with,
    /// compacted under `key` as the number value object it is stored as.
    fn ordinate<'a>(&self, ordinate: f64, key: &str) -> Result<Compacted<'a>, Undecidable> {
        let text = canonical_float_text(ordinate).map_err(|_| Undecidable::UnrepresentableValue)?;
        let number = NumberBuf::from(serde_json::Number::from_string_unchecked(text));
        self.terms.value(&Value::Literal(Literal::Number(number), None::<IriBuf>), key)
    }
}

/// Whether the compact model reads the geometry node whose type `result`
/// holds as a typed geometry: its type written under `type` as the `GeoJSON`
/// token of the geometry.
fn reads_as_geometry(result: &CompactedObject<'_>, form: &GeometryForm) -> bool {
    let token = form.geometry_type().to_string();
    result.len() == 1 && result.get(GEOJSON_TYPE).and_then(Compacted::as_str) == Some(token.as_str())
}
