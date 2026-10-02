//! Reading the expanded `coordinates` of a geometry straight into its
//! positions (clause 4.7, IETF RFC 7946 section 3.1).
//!
//! Expansion under the core context turns `coordinates` into nested `@list`
//! objects whose leaves are `{"@value": n}` ordinates. The readers here follow
//! the nesting the geometry type prescribes as the bytes stream past, building
//! each [`Position`] through [`Position::try_new`], each line through
//! [`LineStringCoordinates::try_new`] and each ring through
//! [`LinearRing::try_new`] — the rules every geometry path shares — so a
//! geometry of a million positions costs its positions and nothing more.

use crate::codec::{
    coordinate_list::{CoordinateRead, GeometryReading, ListStep, Path},
    object_key::ObjectKey,
    raw_scalar::RawScalar,
};
use andromeda_ngsi_ld_geometry::{
    form::{GeometryForm, PolygonRings},
    geojson::GeometryError,
    geometry_type::GeometryType,
    line_string_coordinates::LineStringCoordinates,
    linear_ring::LinearRing,
    position::{Position, PositionError},
};
use serde::{
    Deserialize,
    Deserializer,
    de::{DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Number;
use std::fmt;

/// One position: two or three numeric ordinates.
#[derive(Clone, Copy)]
pub(crate) struct PositionRead;

impl CoordinateRead for PositionRead {
    type Output = Position;

    fn read_items<'de, A: SeqAccess<'de>>(self, reading: &GeometryReading<'_>, mut items: A, path: Path<'_>) -> Result<Position, A::Error> {
        let mut ordinates = [0.0_f64; 3];
        let mut count = 0_usize;
        let mut first_non_numeric = None;
        let mut nests_a_list = false;
        while let Some(ordinate) = items.next_element::<Ordinate>()? {
            match ordinate {
                Ordinate::Number(value) => {
                    if let Some(slot) = ordinates.get_mut(count) {
                        *slot = value;
                    }
                }
                Ordinate::NotANumber => {
                    first_non_numeric.get_or_insert(count);
                }
                Ordinate::List => nests_a_list = true,
            }
            count += 1;
        }
        if nests_a_list {
            return Err(reading.misnested(path));
        }
        let position = match (first_non_numeric, count) {
            (Some(index), _) => Err(PositionError::NonNumericOrdinate { index }),
            (None, 2) => Position::try_new(ordinates[0], ordinates[1], None),
            (None, 3) => Position::try_new(ordinates[0], ordinates[1], Some(ordinates[2])),
            (None, ordinates) if ordinates < 2 => Err(PositionError::TooFewOrdinates { ordinates }),
            (None, ordinates) => Err(PositionError::TooManyOrdinates { ordinates }),
        };
        position.map_err(|source| {
            reading.refuse(GeometryError::Position {
                at: path.coordinate_path(),
                source,
            })
        })
    }
}

/// Every item of a list, each read by `R` one level deeper.
#[derive(Clone, Copy)]
pub(crate) struct EachRead<R>(pub(crate) R);

impl<R: CoordinateRead> CoordinateRead for EachRead<R> {
    type Output = Vec<R::Output>;

    fn read_items<'de, A: SeqAccess<'de>>(self, reading: &GeometryReading<'_>, mut items: A, path: Path<'_>) -> Result<Vec<R::Output>, A::Error> {
        let mut read = Vec::with_capacity(items.size_hint().unwrap_or(0));
        loop {
            let item_path = path.child(read.len());
            let step = ListStep {
                reading,
                path: item_path,
                read: self.0,
            };
            match items.next_element_seed(step)? {
                Some(item) => read.push(item),
                None => return Ok(read),
            }
        }
    }
}

/// The positions of one `LineString` (RFC 7946 section 3.1.4).
#[derive(Clone, Copy)]
pub(crate) struct LineStringRead;

impl CoordinateRead for LineStringRead {
    type Output = LineStringCoordinates;

    fn read_items<'de, A: SeqAccess<'de>>(self, reading: &GeometryReading<'_>, items: A, path: Path<'_>) -> Result<LineStringCoordinates, A::Error> {
        let positions = EachRead(PositionRead).read_items(reading, items, path)?;
        LineStringCoordinates::try_new(positions).map_err(|source| {
            reading.refuse(GeometryError::LineString {
                at: path.coordinate_path(),
                source,
            })
        })
    }
}

/// The positions of one Polygon ring (RFC 7946 section 3.1.6).
#[derive(Clone, Copy)]
pub(crate) struct RingRead;

impl CoordinateRead for RingRead {
    type Output = LinearRing;

    fn read_items<'de, A: SeqAccess<'de>>(self, reading: &GeometryReading<'_>, items: A, path: Path<'_>) -> Result<LinearRing, A::Error> {
        let positions = EachRead(PositionRead).read_items(reading, items, path)?;
        LinearRing::try_new(positions).map_err(|source| {
            reading.refuse(GeometryError::LinearRing {
                at: path.coordinate_path(),
                source,
            })
        })
    }
}

/// The rings of one Polygon, exterior ring first.
#[derive(Clone, Copy)]
pub(crate) struct RingsRead;

impl CoordinateRead for RingsRead {
    type Output = PolygonRings;

    fn read_items<'de, A: SeqAccess<'de>>(self, reading: &GeometryReading<'_>, items: A, path: Path<'_>) -> Result<PolygonRings, A::Error> {
        EachRead(RingRead).read_items(reading, items, path).map(PolygonRings::new)
    }
}

/// One item of a position list.
enum Ordinate {
    /// A number, as the nearest double.
    Number(f64),
    /// Anything that is not a number.
    NotANumber,
    /// A nested list, where a position holds none.
    List,
}

impl<'de> Deserialize<'de> for Ordinate {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Ordinate, D::Error> {
        deserializer.deserialize_any(OrdinateVisitor)
    }
}

/// Reads one item of a position list.
struct OrdinateVisitor;

impl<'de> Visitor<'de> for OrdinateVisitor {
    type Value = Ordinate;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded ordinate")
    }

    fn visit_bool<E>(self, _: bool) -> Result<Ordinate, E> {
        Ok(Ordinate::NotANumber)
    }

    fn visit_str<E>(self, _: &str) -> Result<Ordinate, E> {
        Ok(Ordinate::NotANumber)
    }

    fn visit_unit<E>(self) -> Result<Ordinate, E> {
        Ok(Ordinate::NotANumber)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Ordinate, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(Ordinate::List)
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Ordinate, A::Error> {
        let mut ordinate = Ordinate::NotANumber;
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Value => {
                    if let RawScalar::Number(number) = map.next_value::<RawScalar>()? {
                        ordinate = number.as_f64().filter(|value| value.is_finite()).map_or(Ordinate::NotANumber, Ordinate::Number);
                    }
                }
                ObjectKey::List => {
                    map.next_value::<IgnoredAny>()?;
                    ordinate = Ordinate::List;
                }
                ObjectKey::Id
                | ObjectKey::Type
                | ObjectKey::Language
                | ObjectKey::Direction
                | ObjectKey::OtherKeyword
                | ObjectKey::Name(_)
                | ObjectKey::InvalidName(_) => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        Ok(ordinate)
    }

    fn visit_u64<E>(self, value: u64) -> Result<Ordinate, E> {
        Ok(Number::from(value).as_f64().map_or(Ordinate::NotANumber, Ordinate::Number))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Ordinate, E> {
        Ok(Number::from(value).as_f64().map_or(Ordinate::NotANumber, Ordinate::Number))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Ordinate, E> {
        Ok(Ordinate::Number(value))
    }
}

/// Reads the `coordinates` member of a geometry node, `[{"@list": [...]}]`,
/// into the geometry its type nests.
pub(crate) struct CoordinatesSeed<'g> {
    /// The geometry being read.
    pub(crate) reading: &'g GeometryReading<'g>,
}

impl<'de> DeserializeSeed<'de> for CoordinatesSeed<'_> {
    type Value = GeometryForm;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<GeometryForm, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for CoordinatesSeed<'_> {
    type Value = GeometryForm;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded coordinates member")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<GeometryForm, A::Error> {
        let reading = self.reading;
        let root = Path::ROOT;
        let form = match reading.geometry_type {
            GeometryType::Point => seq.next_element_seed(step(reading, root, PositionRead))?.map(GeometryForm::Point),
            GeometryType::MultiPoint => seq
                .next_element_seed(step(reading, root, EachRead(PositionRead)))?
                .map(GeometryForm::MultiPoint),
            GeometryType::LineString => seq.next_element_seed(step(reading, root, LineStringRead))?.map(GeometryForm::LineString),
            GeometryType::MultiLineString => seq
                .next_element_seed(step(reading, root, EachRead(LineStringRead)))?
                .map(GeometryForm::MultiLineString),
            GeometryType::Polygon => seq.next_element_seed(step(reading, root, RingsRead))?.map(GeometryForm::Polygon),
            GeometryType::MultiPolygon => seq.next_element_seed(step(reading, root, EachRead(RingsRead)))?.map(GeometryForm::MultiPolygon),
        };
        let Some(form) = form else {
            return Err(reading.misnested(root));
        };
        if seq.next_element::<IgnoredAny>()?.is_some() {
            return Err(reading.misnested(root));
        }
        Ok(form)
    }
}

/// The step reading the root list with `read`.
const fn step<'g, R>(reading: &'g GeometryReading<'g>, path: Path<'static>, read: R) -> ListStep<'g, 'static, R> {
    ListStep { reading, path, read }
}
