//! What a trailing path reaches in the `GeoJSON` geometry object a
//! `GeoProperty`'s value is.
//!
//! Clause 4.9 evaluates a trailing path as a member expression against the
//! target value, and clause 4.7.1 makes a `GeoProperty`'s value a `GeoJSON`
//! geometry. IETF RFC 7946 describes a geometry's `type`, `coordinates`
//! (section 3.1), `bbox` (section 5) and `geometries` (section 3.1.8), named as
//! it spells them, and lets the object carry foreign members (section 6.1).
//! The value of a described member is a string or an array, neither of which
//! has a named member, so a path reaches one only when it names it alone. A
//! foreign member is expanded as a member of a structured value is, so a path
//! reaches it and the members within it by expanded name, and no foreign
//! member carries the expanded name of a described member, which expansion
//! keeps beside the foreign ones.

use crate::filter::trailing_group::MemberPath;
use andromeda_ngsi_ld_geometry::geometry_member::GeometryMember;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_ngsi_ld_vocabulary::expanded::properties::{BBOX, COORDINATES};

/// What a trailing path reaches in a geometry object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeometryPath<'p> {
    /// A member RFC 7946 describes, named alone by its own name.
    Described(GeometryMember),
    /// A foreign member and the members within it, by expanded name, the
    /// foreign member first.
    Foreign(&'p [AttributeName]),
    /// Nothing: a path through the value of a described member, or a foreign
    /// path naming a described member by its expanded name.
    Nothing,
}

impl GeometryPath<'_> {
    /// What `members` reaches in a geometry object: a described member by the
    /// name the client wrote, any other member by the name expansion gave it.
    #[must_use]
    pub fn of(members: &MemberPath) -> GeometryPath<'_> {
        let (Some(written), Some(addressed)) = (members.written.first(), members.addressed.first()) else {
            return GeometryPath::Nothing;
        };
        match written.as_str().parse::<GeometryMember>() {
            Ok(member) if members.written.len() == 1 => GeometryPath::Described(member),
            Ok(_) => GeometryPath::Nothing,
            Err(_) if [BBOX.as_str(), COORDINATES.as_str()].contains(&addressed.as_str()) => GeometryPath::Nothing,
            Err(_) => GeometryPath::Foreign(&members.addressed),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::{reading::geometry_path::GeometryPath, trailing_group::MemberPath};
    use andromeda_ngsi_ld_geometry::geometry_member::GeometryMember;
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use pretty_assertions::assert_eq;

    fn names(names: &[&str]) -> Vec<AttributeName> {
        names.iter().map(|name| name.parse().expect("an attribute name")).collect()
    }

    /// A path whose members expansion addressed as `addressed` after the
    /// client wrote them as `written`.
    fn path(written: &[&str], addressed: &[&str]) -> MemberPath {
        MemberPath {
            addressed: names(addressed),
            written: names(written),
        }
    }

    /// The core `@context` maps `type` to a keyword, which a path keeps as
    /// written, and `bbox` and `coordinates` to the `GeoJSON` vocabulary; each
    /// is read by the name the client wrote.
    #[test]
    fn a_described_member_named_alone_is_read_by_its_written_name() {
        for (written, addressed, member) in [
            ("type", "type", GeometryMember::Type),
            ("coordinates", "https://purl.org/geojson/vocab#coordinates", GeometryMember::Coordinates),
            ("bbox", "https://purl.org/geojson/vocab#bbox", GeometryMember::Bbox),
            ("geometries", "https://example.com/geometries", GeometryMember::Geometries),
        ] {
            assert_eq!(GeometryPath::of(&path(&[written], &[addressed])), GeometryPath::Described(member), "{written}");
        }
    }

    /// A described member holds a string or an array, which has no named
    /// member (clause 4.9 makes such an element non-existent).
    #[test]
    fn a_path_through_a_described_member_reaches_nothing() {
        let through = path(
            &["coordinates", "first"],
            &["https://purl.org/geojson/vocab#coordinates", "https://example.com/first"],
        );
        assert_eq!(GeometryPath::of(&through), GeometryPath::Nothing);
    }

    /// RFC 7946 section 6.1: any other member is foreign, and expansion keys
    /// it, and every member within it, by expanded name.
    #[test]
    fn any_other_member_is_a_foreign_one_read_by_its_expanded_names() {
        let foreign = path(&["meta", "source"], &["https://example.com/meta", "https://example.com/source"]);
        assert_eq!(GeometryPath::of(&foreign), GeometryPath::Foreign(&foreign.addressed));
    }

    /// The `bbox` and the `coordinates` are kept beside the foreign members,
    /// so no spelling of their expanded names reaches one.
    #[test]
    fn the_expanded_name_of_a_described_member_names_no_foreign_member() {
        for expanded in ["https://purl.org/geojson/vocab#bbox", "https://purl.org/geojson/vocab#coordinates"] {
            assert_eq!(GeometryPath::of(&path(&["box"], &[expanded])), GeometryPath::Nothing, "{expanded}");
        }
    }
}
