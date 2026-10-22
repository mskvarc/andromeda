//! The members IETF RFC 7946 describes for a geometry object.
//!
//! Every reader and writer of a geometry object names its members here, and a
//! member by any other name is a foreign member (section 6.1).

use strum::{EnumIter, EnumString, IntoStaticStr};

/// A member RFC 7946 describes for a geometry object.
#[derive(Clone, Copy, Debug, PartialEq, Eq, EnumIter, EnumString, IntoStaticStr)]
#[strum(serialize_all = "camelCase")]
pub enum GeometryMember {
    /// `type`, the geometry type (section 3.1).
    Type,
    /// `coordinates`, the positions of every geometry but a collection
    /// (section 3.1.1).
    Coordinates,
    /// `bbox`, the bounding box (section 5).
    Bbox,
    /// `geometries`, the geometries of a `GeometryCollection` (section 3.1.8).
    Geometries,
}

impl GeometryMember {
    /// The member's name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        self.into()
    }
}

#[cfg(test)]
mod tests {
    use crate::geometry_member::GeometryMember;
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn every_member_is_named_as_rfc_7946_spells_it() {
        pretty_assertions::assert_eq!(
            GeometryMember::iter().map(GeometryMember::as_str).collect::<Vec<_>>(),
            ["type", "coordinates", "bbox", "geometries"]
        );
        for member in GeometryMember::iter() {
            pretty_assertions::assert_eq!(GeometryMember::from_str(member.as_str()), Ok(member));
        }
        assert!(GeometryMember::from_str("Coordinates").is_err());
    }
}
