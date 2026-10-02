//! Reading one expanded coordinate list, `{"@list": [...]}`, and naming where
//! it sits when it is refused.
//!
//! Every level of a geometry's `coordinates` is one such list; what its items
//! are is the business of the [`CoordinateRead`] reading it. A refusal names
//! the geometry type and the index path of the list, built only when
//! something is refused.

use crate::{
    codec::{decode_context::DecodeContext, object_key::ObjectKey},
    refusal::EntityRefusal,
};
use andromeda_ngsi_ld_geometry::{coordinate_path::CoordinatePath, geojson::GeometryError, geometry_type::GeometryType};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use serde::{
    Deserializer,
    de::{DeserializeSeed, Error, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

/// The geometry being read, and where its refusal goes.
pub(crate) struct GeometryReading<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The `GeoProperty` the geometry belongs to.
    pub(crate) attribute: &'c AttributeName,
    /// The type whose nesting is read.
    pub(crate) geometry_type: GeometryType,
}

impl GeometryReading<'_> {
    /// Record a geometry refusal and turn it into the deserializer's error.
    pub(crate) fn refuse<E: Error>(&self, source: GeometryError) -> E {
        self.context.refuse(EntityRefusal::GeoProperty {
            attribute: self.attribute.clone(),
            source,
        })
    }

    /// The refusal of a member found where the type nests something else.
    pub(crate) fn misnested<E: Error>(&self, path: Path<'_>) -> E {
        self.refuse(GeometryError::Misnested {
            geometry_type: self.geometry_type,
            at: path.coordinate_path(),
        })
    }
}

/// Where one list sits in the coordinates: its index at each level, as a chain
/// of borrowed frames so no path is allocated unless one is refused.
#[derive(Clone, Copy)]
pub(crate) struct Path<'p> {
    /// The enclosing list and this list's index in it; `None` at the root.
    step: Option<(&'p Path<'p>, usize)>,
}

impl<'p> Path<'p> {
    /// The `coordinates` member itself.
    pub(crate) const ROOT: Path<'static> = Path { step: None };

    /// The list at `index` inside this one.
    pub(crate) const fn child(&'p self, index: usize) -> Path<'p> {
        Path { step: Some((self, index)) }
    }

    /// The indices, outermost first.
    pub(crate) fn coordinate_path(self) -> CoordinatePath {
        let mut indices = Vec::new();
        let mut cursor = self;
        while let Some((parent, index)) = cursor.step {
            indices.push(index);
            cursor = *parent;
        }
        indices.reverse();
        CoordinatePath::new(indices)
    }
}

/// How the items of one list are read into what that level of the geometry
/// holds.
pub(crate) trait CoordinateRead: Copy {
    /// What the list is read into.
    type Output;

    /// Read every item of the list at `path`.
    fn read_items<'de, A: SeqAccess<'de>>(self, reading: &GeometryReading<'_>, items: A, path: Path<'_>) -> Result<Self::Output, A::Error>;
}

/// Reads one `{"@list": [...]}` object and hands its items to `read`.
pub(crate) struct ListStep<'g, 'p, R> {
    /// The geometry being read.
    pub(crate) reading: &'g GeometryReading<'g>,
    /// Where the list sits.
    pub(crate) path: Path<'p>,
    /// How its items are read.
    pub(crate) read: R,
}

impl<'de, R: CoordinateRead> DeserializeSeed<'de> for ListStep<'_, '_, R> {
    type Value = R::Output;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<R::Output, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de, R: CoordinateRead> Visitor<'de> for ListStep<'_, '_, R> {
    type Value = R::Output;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded coordinate list")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<R::Output, A::Error> {
        let mut output = None;
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::List => {
                    output = Some(map.next_value_seed(ItemsStep {
                        reading: self.reading,
                        path: self.path,
                        read: self.read,
                    })?);
                }
                ObjectKey::Id
                | ObjectKey::Type
                | ObjectKey::Value
                | ObjectKey::Language
                | ObjectKey::Direction
                | ObjectKey::OtherKeyword
                | ObjectKey::Name(_)
                | ObjectKey::InvalidName(_) => return Err(self.reading.misnested(self.path)),
            }
        }
        output.ok_or_else(|| self.reading.misnested(self.path))
    }
}

/// Reads the item array of one list.
struct ItemsStep<'g, 'p, R> {
    /// The geometry being read.
    reading: &'g GeometryReading<'g>,
    /// Where the list sits.
    path: Path<'p>,
    /// How its items are read.
    read: R,
}

impl<'de, R: CoordinateRead> DeserializeSeed<'de> for ItemsStep<'_, '_, R> {
    type Value = R::Output;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<R::Output, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de, R: CoordinateRead> Visitor<'de> for ItemsStep<'_, '_, R> {
    type Value = R::Output;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("the items of an expanded coordinate list")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, items: A) -> Result<R::Output, A::Error> {
        self.read.read_items(self.reading, items, self.path)
    }
}
