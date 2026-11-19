//! What a single `aggrGroupBy` term groups on (extension grammar §4.2).
//!
//! `GroupKey = ValuePath | EntityMember`. The `ValuePath` arm reuses the clause
//! 4.9 production every path-addressing parameter shares, so grouping introduces
//! no new path syntax; the `EntityMember` arm is the fixed structural vocabulary.
//! Classification follows the `orderBy` keyword rule: a bare name matching a
//! member token is that member, and anything carrying a path is a value key.

use crate::{
    filter::{filter_error::FilterError, value_path::ValuePath},
    name_stage::{NameStage, Written},
    temporal::grouping::entity_member::EntityMember,
};
use andromeda_ngsi_ld_lexicon::time_prop::TimeProp;
use std::str::FromStr;

/// The thing one grouping term keys on, its attribute names in the stage `S`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GroupKey<S: NameStage> {
    /// A structural entity or instance member (`id`, `type`, `scope`,
    /// `datasetId`, `unitCode`).
    Member(EntityMember),
    /// One attribute's value in effect at each instant, addressed by a clause
    /// 4.9 `ValuePath`.
    Path(ValuePath<S>),
}

impl<S: NameStage> GroupKey<S> {
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
            GroupKey::Path(path) if path.is_simple() => TimeProp::from_str(path.attribute.as_ref()).ok(),
            GroupKey::Member(_) | GroupKey::Path(_) => None,
        }
    }
}

impl FromStr for GroupKey<Written> {
    type Err = FilterError;

    /// Parse and classify one key name.
    ///
    /// A path-free name that matches an [`EntityMember`] token classifies as
    /// that member; every other name (including a member token that carries a
    /// sub-attribute or bracketed path) is a [`GroupKey::Path`] value key.
    ///
    /// # Errors
    ///
    /// Returns the [`ValuePath`] `FromStr` failures when the name is not a
    /// well-formed clause 4.9 `ValuePath`.
    fn from_str(name: &str) -> Result<GroupKey<Written>, FilterError> {
        let path = name.parse::<ValuePath<Written>>()?;
        if let Some(member) = path.is_simple().then(|| EntityMember::classify(path.attribute.as_ref())).flatten() {
            return Ok(GroupKey::Member(member));
        }
        Ok(GroupKey::Path(path))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        filter::filter_error::FilterError,
        name_stage::Written,
        temporal::grouping::{entity_member::EntityMember, group_key::GroupKey},
    };

    #[test]
    fn a_member_token_classifies_as_a_member() {
        assert_eq!(
            "datasetId".parse::<GroupKey<Written>>().expect("parses"),
            GroupKey::Member(EntityMember::DatasetId)
        );
        assert_eq!(
            "unitCode".parse::<GroupKey<Written>>().expect("parses"),
            GroupKey::Member(EntityMember::UnitCode)
        );
    }

    #[test]
    fn a_plain_attribute_name_classifies_as_a_value_path() {
        let key = "speed".parse::<GroupKey<Written>>().expect("parses");
        assert!(matches!(key, GroupKey::Path(path) if path.attribute.as_str() == "speed"));
    }

    /// Clause 4.9 forms: a bracketed and a dotted value path.
    #[test]
    fn compound_and_dotted_value_paths_parse_as_paths() {
        for text in ["address[city]", "refDevice.category"] {
            assert!(matches!(text.parse::<GroupKey<Written>>().expect("parses"), GroupKey::Path(_)), "{text}");
        }
    }

    /// A member token that carries a path descends into an attribute of that
    /// name rather than naming the structural member.
    #[test]
    fn a_member_token_with_a_path_is_a_value_path() {
        assert!(matches!("datasetId.foo".parse::<GroupKey<Written>>().expect("parses"), GroupKey::Path(_)));
    }

    /// The key is read as a value path, so input after the name is refused at its first character.
    #[test]
    fn input_after_a_member_name_is_refused_at_its_first_character() {
        let error = "datasetI\"".parse::<GroupKey<Written>>().expect_err("a quotation mark continues no name");
        assert!(
            matches!(error, FilterError::UnexpectedCharacter { character: '"', position: 8 }),
            "got {error:?}"
        );
    }

    #[test]
    fn a_malformed_path_is_rejected() {
        for text in ["a[", "a[]", "a.", "rel{attr}"] {
            assert!(text.parse::<GroupKey<Written>>().is_err(), "{text}");
        }
    }
}
