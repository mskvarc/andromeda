//! The foreign members of a `GeoJSON` geometry (IETF RFC 7946 section 6.1).
//!
//! "Members not described by this specification ("foreign members") MAY be
//! used in a `GeoJSON` document", and RFC 7946 defines "no normative processing
//! model" for them. ETSI GS CIM 009 v1.9.1 clause 4.7.2 accepts a geometry
//! meeting RFC 7946, and clause 4.6.4 returns the content of a value as
//! provided, so a foreign member is kept as the JSON value it was written as:
//! like a `JsonProperty` value (clause 4.5.24), the JSON value is the domain
//! value, held as a [`JsonLiteral`]. The members are kept by name, as the
//! members of any JSON object are (IETF RFC 8259 section 4: "An object is an
//! unordered collection").

use andromeda_json_literal::JsonLiteral;
use derive_more::{AsRef, Display};
use std::collections::BTreeMap;
use thiserror::Error;

/// The members RFC 7946 describes for a geometry object, which no foreign
/// member may be named.
const GEOJSON_MEMBERS: [&str; 4] = ["type", "coordinates", "bbox", "geometries"];

/// The name of a foreign member: any member name but the ones RFC 7946
/// describes for a geometry, and no JSON-LD keyword, which is JSON-LD rather
/// than a member of the geometry.
#[derive(AsRef, Clone, Debug, Display, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[as_ref(str)]
pub struct ForeignMemberName(Box<str>);

/// Why a member name names no foreign member.
#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ForeignMemberNameError {
    /// RFC 7946 describes the member for a geometry object.
    #[error("the member is one RFC 7946 describes for a geometry object, not a foreign member")]
    GeoJsonMember,
    /// The name is a JSON-LD keyword.
    #[error("a JSON-LD keyword names no foreign member of a GeoJSON geometry")]
    Keyword,
}

impl TryFrom<&str> for ForeignMemberName {
    type Error = ForeignMemberNameError;

    fn try_from(name: &str) -> Result<ForeignMemberName, ForeignMemberNameError> {
        if GEOJSON_MEMBERS.contains(&name) {
            return Err(ForeignMemberNameError::GeoJsonMember);
        }
        if name.starts_with('@') {
            return Err(ForeignMemberNameError::Keyword);
        }
        Ok(ForeignMemberName(Box::from(name)))
    }
}

/// The foreign members of one geometry object, by name.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ForeignMembers(BTreeMap<ForeignMemberName, JsonLiteral>);

impl ForeignMembers {
    /// The members `members` holds.
    #[must_use]
    pub const fn new(members: BTreeMap<ForeignMemberName, JsonLiteral>) -> ForeignMembers {
        ForeignMembers(members)
    }

    /// Whether there is none.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Every member, by name.
    pub fn iter(&self) -> impl Iterator<Item = (&ForeignMemberName, &JsonLiteral)> {
        self.0.iter()
    }

    /// Keep `value` under `name`; a name written again keeps its last value,
    /// as a parsed tree keeps it.
    pub fn insert(&mut self, name: ForeignMemberName, value: JsonLiteral) {
        self.0.insert(name, value);
    }
}

#[cfg(test)]
mod tests {
    use crate::foreign_members::{ForeignMemberName, ForeignMemberNameError, ForeignMembers};
    use andromeda_json_literal::JsonLiteral;

    /// RFC 7946 section 6.1: any member the geometry object is not described
    /// with is foreign; a JSON-LD keyword is JSON-LD.
    #[test]
    fn a_foreign_member_name_is_any_name_but_the_geojson_members() {
        for name in ["title", "crs", "properties", "geometry", ""] {
            assert!(ForeignMemberName::try_from(name).is_ok(), "{name}");
        }
        for name in ["type", "coordinates", "bbox", "geometries"] {
            pretty_assertions::assert_eq!(ForeignMemberName::try_from(name), Err(ForeignMemberNameError::GeoJsonMember), "{name}");
        }
        pretty_assertions::assert_eq!(ForeignMemberName::try_from("@context"), Err(ForeignMemberNameError::Keyword));
    }

    /// A member written twice keeps its last value; members read by name.
    #[test]
    fn a_member_written_again_keeps_its_last_value() {
        let name = |name: &str| ForeignMemberName::try_from(name).expect("a foreign member name");
        let literal = |text: &str| JsonLiteral::admit(text).expect("a JSON literal");
        let mut members = ForeignMembers::default();
        members.insert(name("b"), literal("2"));
        members.insert(name("a"), literal("1"));
        members.insert(name("a"), literal("3"));
        let written: Vec<(String, &str)> = members.iter().map(|(name, value)| (name.to_string(), value.text())).collect();
        pretty_assertions::assert_eq!(written, vec![("a".to_owned(), "3"), ("b".to_owned(), "2")]);
    }
}
