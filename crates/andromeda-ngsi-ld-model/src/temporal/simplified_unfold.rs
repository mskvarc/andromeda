//! Unfolding the simplified temporal representation of a Temporal Evolution
//! (ETSI GS CIM 009 clause 4.5.9) into the instances it stands for, while the
//! evolution is read.
//!
//! An attribute object in that representation holds its family's plural member
//! (`values`, `objects`, `languageMaps`, `vocabs`, `jsons`, `valueLists`,
//! `objectLists`), whose every element is a `[value, instant]` pair. Clause
//! 4.5.9 has the object "only contain" that member; besides it the object
//! carries only its `type`, which names the family. Anything else, a
//! `datasetId` included, is refused.
//!
//! Each pair becomes one instance: the object's `type`, the
//! family's value member holding the pair's value (taken out of the
//! one-member object clause 4.5.9 wraps the value of a `LanguageProperty`, a
//! `JsonProperty` and a `VocabProperty` in), and the pair's instant as
//! `observedAt`. A system time is the broker's to set, so only the
//! `observedAt` axis of a pair is an instant a write carries. An instance is
//! read from the text of its own, never from a tree of the pairs.

use crate::{
    attribute::{instance::Attribute, member_value_reading::ReadError, structure_budget},
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
    temporal::plural_member_refusal::PluralMemberRefusal,
};
use andromeda_json_literal::{JsonLiteral, piece::JsonPiece};
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::{AttributeType, SimplifiedValueEncoding},
};
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeMembers, NodeShape};
use andromeda_ngsi_ld_vocabulary::{compact::properties as compact_properties, keyword};
use serde_json::value::RawValue;
use std::{borrow::Cow, convert::Infallible, str::FromStr};

/// What is wrong with an attribute object holding its plural member, before
/// the attribute is named.
#[derive(Clone, Copy, Debug)]
enum PairFault<'a> {
    /// It carries this member beside the pairs.
    MemberBesidePairs(&'a str),
    /// The plural member is not an array.
    NotAnArray,
    /// An element is not a two-element array.
    NotAPair,
    /// A wrapped family's value is not wrapped.
    UnwrappedValue,
    /// An `observedAt` is not a string.
    InstantNotAString,
}

/// The family of an attribute object that holds its family's plural member.
pub(crate) fn plural_family<'n, N: CompactNode<'n>>(members: &NodeMembers<'n, N>) -> Option<AttributeType> {
    let family = members.get(keyword::TYPE_ALIAS)?.text()?.parse::<AttributeType>().ok()?;
    members.contains(family.temporal_values_key()).then_some(family)
}

/// Push onto `out` the instances the attribute object `members` of the
/// attribute `attribute` and of `family` stands for, one per pair.
///
/// # Errors
///
/// Returns [`NodeRefusal::PluralMember`] when the object carries another
/// member, its plural member is not an array of pairs, or a pair's value is
/// not in its family's form or its instant not a string, and the refusal of an
/// instance a pair makes.
pub(crate) fn unfold<'n, N: CompactNode<'n>>(
    attribute: &str,
    family: AttributeType,
    members: NodeMembers<'n, N>,
    out: &mut Vec<Attribute>,
) -> Result<(), ReadError<'n, N>> {
    let refused = |fault| refusal::<N>(attribute, family, fault);
    let plural = family.temporal_values_key();
    let mut pairs = None;
    let mut beside: Option<Cow<'n, str>> = None;
    for (member, node) in members {
        match member.as_ref() {
            keyword::TYPE_ALIAS => {}
            key if key == plural => pairs = Some(node),
            _other => beside = beside.or(Some(member)),
        }
    }
    let NodeShape::Array(pair_nodes) = pairs.ok_or_else(|| refused(PairFault::NotAnArray))?.shape() else {
        return Err(refused(PairFault::NotAnArray));
    };
    if let Some(member) = &beside {
        return Err(refused(PairFault::MemberBesidePairs(member)));
    }
    for (index, pair) in pair_nodes.into_iter().enumerate() {
        if structure_budget::is_spent(pair) {
            break;
        }
        structure_budget::spend(pair);
        let within = |error: ReadError<'n, N>| error.within_member(plural).within_element(index);
        let text = pair_text(attribute, family, pair).map_err(within)?;
        let raw = RawValue::from_string(text).map_err(|source| {
            within(
                NodeRefusal::Member {
                    member: RefusedMember::Value(family),
                    source,
                }
                .into(),
            )
        })?;
        out.push(Attribute::from_node(attribute, JsonPiece::new(&raw)).map_err(|error| within(widened::<N>(error)))?);
    }
    Ok(())
}

/// The instances the member written under the attribute name `name` stands
/// for: its array, or its one object, each simplified attribute object
/// unfolded and each other object read as the instance it is.
///
/// # Errors
///
/// Returns the refusal of the first element that is neither an instance nor a
/// simplified attribute object, located at the element.
pub(crate) fn instances_of<'n, N: CompactNode<'n>>(name: &str, node: N) -> Result<Vec<Attribute>, ReadError<'n, N>> {
    let items = match node.shape() {
        NodeShape::Array(items) => items,
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => vec![node],
    };
    let mut instances = Vec::new();
    for (index, item) in items.into_iter().enumerate() {
        if structure_budget::is_spent(item) {
            break;
        }
        structure_budget::spend(item);
        let within = |error: ReadError<'n, N>| error.within_element(index);
        match item.shape() {
            NodeShape::Object(members) => match plural_family(&members) {
                Some(family) => unfold(name, family, members, &mut instances).map_err(within)?,
                None => instances.push(Attribute::from_node(name, item).map_err(within)?),
            },
            NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Array(_) | NodeShape::Geometry(_) => {
                instances.push(Attribute::from_node(name, item).map_err(within)?);
            }
        }
    }
    Ok(instances)
}

/// The JSON text of the instance one `[value, instant]` pair of a `family`
/// object stands for.
fn pair_text<'n, N: CompactNode<'n>>(attribute: &str, family: AttributeType, pair: N) -> Result<String, ReadError<'n, N>> {
    let refused = |fault| refusal::<N>(attribute, family, fault);
    let NodeShape::Array(elements) = pair.shape() else {
        return Err(refused(PairFault::NotAPair));
    };
    let [value, instant] = elements.as_slice() else {
        return Err(refused(PairFault::NotAPair));
    };
    let value_member = family.compact_value_key();
    let value = match family.simplified_value_encoding() {
        SimplifiedValueEncoding::Wrapped => {
            let wrapped = match value.shape() {
                NodeShape::Object(members) if members.len() == 1 => members.get(value_member),
                NodeShape::Null
                | NodeShape::Boolean(_)
                | NodeShape::Number(_)
                | NodeShape::String(_)
                | NodeShape::Array(_)
                | NodeShape::Object(_)
                | NodeShape::Geometry(_) => None,
            };
            literal_text(wrapped.ok_or_else(|| refused(PairFault::UnwrappedValue))?, RefusedMember::Value(family))?
        }
        SimplifiedValueEncoding::Bare => literal_text(*value, RefusedMember::Value(family))?,
    };
    let NodeShape::String(_) = instant.shape() else {
        return Err(refused(PairFault::InstantNotAString));
    };
    let observed_at = literal_text(*instant, RefusedMember::ObservedAt)?;
    Ok(format!(
        r#"{{"{}":"{}","{value_member}":{value},"{}":{observed_at}}}"#,
        keyword::TYPE_ALIAS,
        family.short_name(),
        compact_properties::OBSERVED_AT
    ))
}

/// The JSON text of a node.
fn literal_text<'n, N: CompactNode<'n>>(node: N, member: RefusedMember) -> Result<String, ReadError<'n, N>> {
    let literal: JsonLiteral = node
        .literal()
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| ReadError::<N>::from(NodeRefusal::Literal { member, source }))?;
    Ok(literal.to_string())
}

/// The failure of reading a text of the broker's own writing, which no
/// node can leave unreadable, as a failure of the node being read.
fn widened<'n, N: CompactNode<'n>>(error: NodeReadError<Infallible>) -> ReadError<'n, N> {
    match error {
        NodeReadError::Refused(refused) => NodeReadError::Refused(refused),
        NodeReadError::Unreadable(never) => match never {},
    }
}

/// The refusal of the attribute `name` of `family` for `fault`.
fn refusal<'n, N: CompactNode<'n>>(name: &str, family: AttributeType, fault: PairFault<'_>) -> ReadError<'n, N> {
    let named = |raw: &str| AttributeName::from_str(raw).map_err(|source| ReadError::<N>::from(NodeRefusal::AttributeName(source)));
    let attribute = match named(name) {
        Ok(attribute) => attribute,
        Err(refused) => return refused,
    };
    let refused = match fault {
        PairFault::MemberBesidePairs(member) => match named(member) {
            Ok(member) => PluralMemberRefusal::MemberBesidePairs { attribute, family, member },
            Err(refused) => return refused,
        },
        PairFault::NotAnArray => PluralMemberRefusal::NotAnArray { attribute, family },
        PairFault::NotAPair => PluralMemberRefusal::NotAPair { attribute, family },
        PairFault::UnwrappedValue => PluralMemberRefusal::UnwrappedValue { attribute, family },
        PairFault::InstantNotAString => PluralMemberRefusal::InstantNotAString { attribute, family },
    };
    NodeRefusal::PluralMember(Box::new(refused)).into()
}
