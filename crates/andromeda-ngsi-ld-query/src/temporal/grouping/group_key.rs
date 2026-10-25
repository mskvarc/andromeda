//! What a single `aggrGroupBy` term groups on (extension grammar §4.2).
//!
//! `GroupKey = ValuePath | EntityMember`. The `ValuePath` arm reuses the clause
//! 4.9 production every path-addressing parameter shares, so grouping introduces
//! no new path syntax; the `EntityMember` arm is the fixed structural vocabulary.
//! Classification follows the `orderBy` keyword rule: a bare name matching a
//! member token is that member, and anything carrying a path is a value key.

use crate::{
    error::QueryError,
    filter::{path::AttrPath, value_path::parse_attribute_path},
    temporal::grouping::entity_member::EntityMember,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use std::str::FromStr;

/// The thing one grouping term keys on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKey {
    /// A structural entity or instance member (`id`, `type`, `scope`,
    /// `datasetId`, `unitCode`).
    Member(EntityMember),
    /// One attribute's value in effect at each instant, addressed by a clause
    /// 4.9 `ValuePath`.
    Path(AttrPath),
}

impl GroupKey {
    /// Parse and classify one key name.
    ///
    /// A path-free name that matches an [`EntityMember`] token classifies as
    /// that member; every other name — including a member token that carries a
    /// sub-attribute or bracketed path — is a [`GroupKey::Path`] value key.
    ///
    /// # Errors
    ///
    /// Returns the [`parse_attribute_path`] failures when the name is not a
    /// well-formed clause 4.9 `ValuePath`.
    pub fn parse(name: &str) -> Result<GroupKey, QueryError> {
        let path = parse_attribute_path(name)?;
        if let Some(member) = path.is_simple().then(|| EntityMember::classify(path.attr.as_str())).flatten() {
            return Ok(GroupKey::Member(member));
        }
        Ok(GroupKey::Path(path))
    }

    /// The temporal axis this key names, if any.
    ///
    /// A bucket modifier is only meaningful on a temporal key. The clause 4.8
    /// temporal properties (`observedAt`, `createdAt`, `modifiedAt`,
    /// `deletedAt`) are the statically-known `DateTime` members, so a simple
    /// [`GroupKey::Path`] naming one of them is a temporal axis and any other
    /// key is not.
    #[must_use]
    pub fn temporal_property(&self) -> Option<TimeProp> {
        match self {
            GroupKey::Path(path) if path.is_simple() => TimeProp::from_str(path.attr.as_str()).ok(),
            GroupKey::Member(_) | GroupKey::Path(_) => None,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::temporal::grouping::{entity_member::EntityMember, group_key::GroupKey};

    #[test]
    fn a_member_token_classifies_as_a_member() {
        assert_eq!(GroupKey::parse("datasetId").expect("parses"), GroupKey::Member(EntityMember::DatasetId));
        assert_eq!(GroupKey::parse("unitCode").expect("parses"), GroupKey::Member(EntityMember::UnitCode));
    }

    #[test]
    fn a_plain_attribute_name_classifies_as_a_value_path() {
        let key = GroupKey::parse("speed").expect("parses");
        assert!(matches!(key, GroupKey::Path(path) if path.attr.as_str() == "speed"));
    }

    /// Clause 4.9 forms: a bracketed and a dotted value path.
    #[test]
    fn compound_and_dotted_value_paths_parse_as_paths() {
        for text in ["address[city]", "refDevice.category"] {
            assert!(matches!(GroupKey::parse(text).expect("parses"), GroupKey::Path(_)), "{text}");
        }
    }

    /// A member token that carries a path descends into an attribute of that
    /// name rather than naming the structural member.
    #[test]
    fn a_member_token_with_a_path_is_a_value_path() {
        assert!(matches!(GroupKey::parse("datasetId.foo").expect("parses"), GroupKey::Path(_)));
    }

    #[test]
    fn a_malformed_path_is_rejected() {
        for text in ["a[", "a[]", "a.", "rel{attr}"] {
            assert!(GroupKey::parse(text).is_err(), "{text}");
        }
    }
}
