//! How clause 4.9 reads the target value each attribute family carries.
//!
//! Clause 4.9 defines a target value for every attribute family of clause
//! 5.2.4, and the definitions differ in shape, not only in which member carries
//! them:
//!
//! * "If the target element is a Property, the target value is defined as the
//!   Value associated to such Property", one value, or, when the client wrote
//!   an array, the array JSON-LD expansion turned into several `hasValue`
//!   nodes;
//! * "If the target element is a `LanguageProperty`, and no target language is
//!   specified, the target value is defined as a value from any of the
//!   key-value pairs held within the languageMap";
//! * "If the target element is a `ListProperty`, the target value is defined as
//!   the valueList array associated to such a `ListProperty`";
//! * "If the target element is a `ListRelationship`, the target object is
//!   defined as the array of objects associated (represented as URIs) to such
//!   `ListRelationship`";
//! * "If the target element is a `VocabProperty`, the target value shall be
//!   expanded according to the @context";
//! * "If the target element is a Relationship, the target object is defined as
//!   the object associated (represented as a URI or array of URIs)".
//!
//! Every one of those that can hold more than one value resolves to an array, which is what makes the clause's array rules ("Is included in the target value, if the latter is an array", and under `unequal` "Is not included in the target value, if the latter is an array") apply to it.
//!
//! The two list families are ordered, so their target is the array even when
//! it holds a single element, which is what keeps the ordering and pattern
//! operators off it.
//!
//! Every execution reads a family through [`family_reading`], so what a
//! family's target is has one statement.

use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;

/// How a family's value member holds its values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueLayout {
    /// One value node per value the client wrote. A single value is a scalar
    /// target; several are an array target.
    Nodes,
    /// One ordered list, whose items are the target array whatever their count.
    OrderedList,
}

/// Which values of a member a target is read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalKeyword {
    /// Literals only, the values a JSON-LD value object carries under
    /// `@value`.
    Literal,
    /// Literals and the IRIs references and nodes are identified by, carried
    /// under `@id`: a Relationship's objects, a `VocabProperty`'s terms, and
    /// the end of a path that stops on a node.
    Identifier,
}

/// Which clause 4.9 definition a target satisfies.
///
/// Clause 4.9 forbids the ordering and pattern operators against a
/// Relationship, so a value's origin matters, not only its shape: a
/// Relationship object and a string Property value are both strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetOrigin {
    /// A target *value*: what a Property-family attribute carries.
    AttributeValue,
    /// A target *object*: the URI, or array of URIs, a Relationship or a
    /// `ListRelationship` names.
    RelationshipObject,
}

/// What clause 4.9 makes of an attribute that carries several values.
///
/// The two readings agree on the equality operators and part company on every
/// other one, so which of them applies is a property of the target rather than
/// of the operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiValueReading {
    /// The array of values is itself the target value.
    ///
    /// Clause 4.9 defines a `ListProperty`'s target value as "the valueList
    /// array associated to such a `ListProperty`" and a `ListRelationship`'s as
    /// "the array of objects", and a Property's is the Value the client wrote,
    /// an array when they wrote one. The equality operators test membership in
    /// it ("Is included in the target value, if the latter is an array"), while
    /// an operator defined over a scalar or a String target does not match it at
    /// all: "If the target value data type is different than String then it
    /// shall be considered as not matching."
    WholeArray,
    /// Any one of the values is the target value in its own right.
    ///
    /// Clause 4.9: "If the target element is a `LanguageProperty`, and no target
    /// language is specified, the target value is defined as a value from any of
    /// the key-value pairs held within the languageMap." Every operator is
    /// therefore answered by the values themselves, and the negated ones hold
    /// only when none of them satisfies the Query Term: "No matching value is
    /// found in any of the values of the key-value pairs of a languageMap".
    AnyOneValue,
}

/// What the keys of a family's value name, which decides what a one-member
/// trailing group spelled like a language tag addresses (`color[en]`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueKeys {
    /// The members of a compound value (`address[city]`).
    Members,
    /// The languages of a languageMap: clause 4.9 makes the one-member group of
    /// a `LanguageProperty` its target language (`color[en]=="red"`).
    Languages,
}

/// Where a trailing group reads the members of a family's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberSource {
    /// The value's own members: an expanded structured value's by expanded
    /// name, a JSON literal's by the names the client wrote.
    Value,
    /// The members of the `GeoJSON` geometry object the value is (IETF RFC
    /// 7946 sections 3.1, 5 and 6.1), as
    /// [`GeometryPath`](crate::filter::reading::geometry_path::GeometryPath)
    /// tells them apart: `type`, `coordinates` and `bbox` by the names the
    /// client wrote, and a foreign member by expanded name.
    Geometry,
}

/// How clause 4.9 reads one attribute family's values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FamilyReading {
    /// How the family's value member holds its values.
    pub layout: ValueLayout,
    /// Which values of the member make the target.
    pub terminal: TerminalKeyword,
    /// Which clause 4.9 definition the values satisfy.
    pub origin: TargetOrigin,
    /// What clause 4.9 makes of several values.
    pub reading: MultiValueReading,
    /// What the keys of the family's value name.
    pub keys: ValueKeys,
    /// Where a trailing group reads the members of the value.
    pub members: MemberSource,
}

/// The families a clause 4.9 hop crosses into the Entities their objects
/// name: those whose target is a Relationship's object, a Relationship and a
/// `ListRelationship`, whose objects the Linked Entity retrieval of clause
/// 4.5.23 follows alike.
pub fn hop_families() -> impl Iterator<Item = AttributeType> {
    AttributeType::ALL
        .into_iter()
        .filter(|family| matches!(family_reading(*family).origin, TargetOrigin::RelationshipObject))
}

/// How clause 4.9 reads the values of `family`, covering all eight attribute
/// families of clause 5.2.4 so a `q` over any of them resolves.
#[must_use]
pub const fn family_reading(family: AttributeType) -> FamilyReading {
    let (layout, terminal, origin, reading, keys, members) = match family {
        AttributeType::Property | AttributeType::JsonProperty => (
            ValueLayout::Nodes,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
            ValueKeys::Members,
            MemberSource::Value,
        ),
        AttributeType::GeoProperty => (
            ValueLayout::Nodes,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
            ValueKeys::Members,
            MemberSource::Geometry,
        ),
        AttributeType::Relationship => (
            ValueLayout::Nodes,
            TerminalKeyword::Identifier,
            TargetOrigin::RelationshipObject,
            MultiValueReading::WholeArray,
            ValueKeys::Members,
            MemberSource::Value,
        ),
        AttributeType::VocabProperty => (
            ValueLayout::Nodes,
            TerminalKeyword::Identifier,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
            ValueKeys::Members,
            MemberSource::Value,
        ),
        AttributeType::LanguageProperty => (
            ValueLayout::Nodes,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::AnyOneValue,
            ValueKeys::Languages,
            MemberSource::Value,
        ),
        AttributeType::ListProperty => (
            ValueLayout::OrderedList,
            TerminalKeyword::Literal,
            TargetOrigin::AttributeValue,
            MultiValueReading::WholeArray,
            ValueKeys::Members,
            MemberSource::Value,
        ),
        AttributeType::ListRelationship => (
            ValueLayout::OrderedList,
            TerminalKeyword::Identifier,
            TargetOrigin::RelationshipObject,
            MultiValueReading::WholeArray,
            ValueKeys::Members,
            MemberSource::Value,
        ),
    };
    FamilyReading {
        layout,
        terminal,
        origin,
        reading,
        keys,
        members,
    }
}

#[cfg(test)]
mod tests {
    use crate::filter::reading::family_reading::{MemberSource, MultiValueReading, TargetOrigin, ValueKeys, ValueLayout, family_reading};
    use andromeda_ngsi_ld_lexicon::attribute_type::AttributeType;
    use pretty_assertions::assert_eq;

    /// The families whose reading `holds` picks out, in declaration order.
    fn families_where(holds: impl Fn(AttributeType) -> bool) -> Vec<AttributeType> {
        AttributeType::ALL.into_iter().filter(|family| holds(*family)).collect()
    }

    /// Clause 4.9 names a target *object* for a Relationship and a
    /// `ListRelationship` alone.
    #[test]
    fn only_the_two_relationship_families_hold_target_objects() {
        assert_eq!(
            families_where(|family| family_reading(family).origin == TargetOrigin::RelationshipObject),
            [AttributeType::Relationship, AttributeType::ListRelationship]
        );
    }

    /// Clause 4.9 reads the languageMap of a `LanguageProperty` as any one of
    /// its values, and its one-member trailing group as the target language.
    #[test]
    fn only_a_language_property_is_read_as_any_one_value_and_keyed_by_language() {
        assert_eq!(
            families_where(|family| family_reading(family).reading == MultiValueReading::AnyOneValue),
            [AttributeType::LanguageProperty]
        );
        assert_eq!(
            families_where(|family| family_reading(family).keys == ValueKeys::Languages),
            [AttributeType::LanguageProperty]
        );
    }

    /// Clause 4.9 makes the `valueList` and the array of objects the target
    /// whatever their length.
    #[test]
    fn only_the_two_list_families_hold_one_ordered_list() {
        assert_eq!(
            families_where(|family| family_reading(family).layout == ValueLayout::OrderedList),
            [AttributeType::ListProperty, AttributeType::ListRelationship]
        );
    }

    /// Clause 4.9 reads a trailing group against the seed target value, and a
    /// `GeoProperty`'s is the `GeoJSON` geometry it holds as positions.
    #[test]
    fn only_a_geo_property_reads_its_members_from_its_geometry() {
        assert_eq!(
            families_where(|family| family_reading(family).members == MemberSource::Geometry),
            [AttributeType::GeoProperty]
        );
    }
}
