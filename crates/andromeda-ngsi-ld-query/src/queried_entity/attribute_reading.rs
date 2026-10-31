//! What clause 4.9 reads of an entity's attributes.
//!
//! Three traits, one per level of the entity the `q` evaluator walks: the
//! entity, one attribute instance, and one node of a structured value. They are
//! implemented for cheap `Copy` handles (typically shared references), so the
//! evaluator passes them by value and every value it reads borrows from the
//! entity for `'a`.

use crate::{
    filter::{language_matching::LanguageMatching, target_language::LanguageSelector},
    queried_entity::{instance_metadata::InstanceMetadata, value_item::ValueItem},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, attribute_type::AttributeType, entity_type_name::EntityTypeName};
use std::borrow::Cow;

/// An entity as the `q` evaluator reads it (clause 4.9).
pub trait QueriedEntity<'a>: Copy {
    /// One attribute instance of the entity.
    type Instance: QueriedInstance<'a>;

    /// Every instance of the attribute keyed by the expanded name `attribute`,
    /// default and `datasetId` instances alike, in the order the entity holds
    /// them; none when the entity does not carry the attribute.
    ///
    /// Clause 4.9 makes the target of a multi-instance attribute (clause
    /// 4.5.5) "any Value of such instances", so each instance is read.
    fn attribute_instances(self, attribute: &AttributeName) -> impl Iterator<Item = Self::Instance>;

    /// The entity's types, expanded (clause 4.5.1), which the type guard of a
    /// linked-entity hop is checked against.
    fn types(self) -> impl Iterator<Item = &'a EntityTypeName>;
}

/// One attribute instance as the `q` evaluator reads it: a default instance, a
/// `datasetId` instance, or the first instance of a sub-attribute.
pub trait QueriedInstance<'a>: Copy {
    /// One node of a structured value this instance holds.
    type Node: QueriedValueNode<'a>;

    /// The attribute family the instance belongs to (clause 5.2.4), which
    /// decides how clause 4.9 reads its values.
    fn family(self) -> AttributeType;

    /// The values the family's value member holds, in order: the `value` of a
    /// Property or a `GeoProperty` (several when the client wrote an array),
    /// the `object` of a Relationship (several for a multi-target one), the
    /// `vocab` terms of a `VocabProperty`, the strings of a `LanguageProperty`'s
    /// map, the `json` of a `JsonProperty`, and the items of the list a
    /// `ListProperty` or a `ListRelationship` holds.
    fn values(self) -> impl Iterator<Item = ValueItem<'a, Self::Node>>;

    /// The strings a `LanguageProperty` instance holds under every language
    /// tag `wanted` admits under `matching`, in order: the target value clause
    /// 4.9 reads when a target language is specified. None when no key is
    /// selected, and none for every other family.
    fn language_values(self, wanted: LanguageSelector<'_>, matching: LanguageMatching) -> impl Iterator<Item = ValueItem<'a, Self::Node>>;

    /// Every instance of the sub-attribute keyed by the expanded name `name`,
    /// default and `datasetId` instances alike, in the order the instance holds
    /// them, and none when the instance carries no such sub-attribute. Clause
    /// 4.9 reads "any Value of such instances" at every level of the `.` axis,
    /// so each instance is read.
    fn sub_attribute_instances(self, name: &AttributeName) -> impl Iterator<Item = Self>;

    /// The lexical form of what the instance holds as `member`, or `None` when
    /// it holds nothing there.
    fn metadata(self, member: InstanceMetadata) -> Option<Cow<'a, str>>;
}

/// One node of a structured Property value, whose members a `q` compound path
/// (`address[city]`) or a sub-attribute chain addresses by expanded IRI.
pub trait QueriedValueNode<'a>: Copy {
    /// The IRI the node is identified by, when it has one.
    fn id(self) -> Option<&'a str>;

    /// The values of the member keyed by the expanded name `member`, in the
    /// order they are held; none when the node has no such member.
    fn member_values(self, member: &AttributeName) -> impl Iterator<Item = ValueItem<'a, Self>>;
}
