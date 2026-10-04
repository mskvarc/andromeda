//! Reading the elements of an expanded array: value objects, list objects and
//! nodes, together with the NGSI-LD Null wherever it stands.
//!
//! JSON-LD orders nothing inside an object, so an element's members are
//! gathered first and the element is decided once the object is closed. The
//! Null is recognised in the spellings clause 4.5.0 gives it in expanded form:
//! a value object whose `@value` is the Null (whatever its `@type` or
//! `@language`), and a node that is nothing but an `@id` naming it. Whether
//! one is a deletion or an error depends on where it stands, which the caller
//! decides; inside a structured value a member that is exactly the Null is the
//! clause 5.5.12 deletion of that member.

use crate::{
    codec::{decode_context::DecodeContext, object_key::ObjectKey, raw_scalar::RawScalar},
    expanded_value::{ExpandedValue, ValueNode},
    expanded_value_name::ExpandedValueName,
    json_literal::JsonLiteral,
    literal::{Direction, Literal, LiteralValue},
    member_shape::MemberShape,
    ngsi_ld_null::is_ngsi_ld_null,
    node_id::NodeId,
    refusal::EntityRefusal,
    value_mode::{Fragment, FragmentMember},
};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, reference::NgsiLdReference};
use andromeda_ngsi_ld_number::JsonNumber;
use compact_str::CompactString;
use indexmap::IndexMap;
use langtag::LangTagBuf;
use serde::{
    Deserialize,
    Deserializer,
    de::{DeserializeSeed, MapAccess, SeqAccess, Visitor},
};
use std::fmt;

/// The JSON-LD keyword marking a JSON literal's datatype.
pub(crate) const JSON_DATATYPE: &str = "@json";

/// One element of an expanded array.
#[derive(Debug)]
pub(crate) enum Decoded {
    /// A value.
    Value(ExpandedValue<Fragment>),
    /// The NGSI-LD Null.
    Null,
    /// The NGSI-LD Null as the text of one language: a merge deletes that
    /// language (clause 5.5.12).
    TaggedNull(LangTagBuf),
    /// A list object holding the NGSI-LD Null alone: the deletion encoding of
    /// the two list families (clauses 4.5.21.2, 4.5.22.2).
    NullList,
}

/// Reads one expanded array into its elements.
#[derive(Clone, Copy)]
pub(crate) struct ElementsSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The attribute the array belongs to, named in a refusal.
    pub(crate) owner: Option<&'c AttributeName>,
}

impl<'de> DeserializeSeed<'de> for ElementsSeed<'_> {
    type Value = Vec<Decoded>;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Vec<Decoded>, D::Error> {
        deserializer.deserialize_seq(self)
    }
}

impl<'de> Visitor<'de> for ElementsSeed<'_> {
    type Value = Vec<Decoded>;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an array of expanded elements")
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Vec<Decoded>, A::Error> {
        let mut elements = Vec::with_capacity(seq.size_hint().unwrap_or(1));
        while let Some(element) = seq.next_element_seed(ElementSeed {
            context: self.context,
            owner: self.owner,
        })? {
            elements.push(element);
        }
        Ok(elements)
    }
}

/// Reads one element of an expanded array.
#[derive(Clone, Copy)]
pub(crate) struct ElementSeed<'c> {
    /// Where a refusal is recorded.
    pub(crate) context: &'c DecodeContext,
    /// The attribute the element belongs to, named in a refusal.
    pub(crate) owner: Option<&'c AttributeName>,
}

impl ElementSeed<'_> {
    /// The refusal of an element not shaped as the expanded form shapes it.
    fn refusal(&self) -> EntityRefusal {
        malformed(self.owner)
    }
}

impl<'de> DeserializeSeed<'de> for ElementSeed<'_> {
    type Value = Decoded;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Decoded, D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for ElementSeed<'_> {
    type Value = Decoded;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an expanded element")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Decoded, A::Error> {
        let mut parts = ElementParts::default();
        let nested = ElementsSeed {
            context: self.context,
            owner: self.owner,
        };
        while let Some(key) = map.next_key::<ObjectKey>()? {
            match key {
                ObjectKey::Value => parts.value = Some(map.next_value::<RawScalar>()?),
                ObjectKey::Type => parts.types = Some(map.next_value::<TypeEntries>()?),
                ObjectKey::Language => parts.language = Some(map.next_value::<String>()?),
                ObjectKey::Direction => parts.direction = Some(map.next_value::<String>()?),
                ObjectKey::List => parts.list = Some(map.next_value_seed(nested)?),
                ObjectKey::Id => parts.id = Some(map.next_value::<String>()?),
                ObjectKey::Name(name) => {
                    let values = map.next_value_seed(nested)?;
                    parts.members.insert(name, values);
                }
                ObjectKey::OtherKeyword => return Err(self.context.refuse(self.refusal())),
                ObjectKey::InvalidName(source) => return Err(self.context.refuse(EntityRefusal::ExpandedMemberName(source))),
            }
        }
        parts.decide(self.owner).map_err(|refusal| self.context.refuse(refusal))
    }
}

/// The `@type` of an element: one string on a value object, an array on a node.
#[derive(Debug)]
pub(crate) enum TypeEntries {
    /// A single string.
    Single(String),
    /// An array of strings.
    Array(Vec<String>),
}

impl<'de> Deserialize<'de> for TypeEntries {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<TypeEntries, D::Error> {
        deserializer.deserialize_any(TypeEntriesVisitor)
    }
}

/// Reads the `@type` of an element.
struct TypeEntriesVisitor;

impl<'de> Visitor<'de> for TypeEntriesVisitor {
    type Value = TypeEntries;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a type string or an array of them")
    }

    fn visit_str<E>(self, text: &str) -> Result<TypeEntries, E> {
        Ok(TypeEntries::Single(text.to_owned()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<TypeEntries, A::Error> {
        let mut entries = Vec::new();
        while let Some(entry) = seq.next_element::<String>()? {
            entries.push(entry);
        }
        Ok(TypeEntries::Array(entries))
    }
}

/// The members of one element object, gathered before it is decided.
#[derive(Debug, Default)]
struct ElementParts {
    /// `@value`.
    value: Option<RawScalar>,
    /// `@type`.
    types: Option<TypeEntries>,
    /// `@language`.
    language: Option<String>,
    /// `@direction`.
    direction: Option<String>,
    /// `@list`.
    list: Option<Vec<Decoded>>,
    /// `@id`.
    id: Option<String>,
    /// The expanded members of a node.
    members: IndexMap<ExpandedValueName, Vec<Decoded>>,
}

impl ElementParts {
    /// Decide what the gathered members make.
    fn decide(self, owner: Option<&AttributeName>) -> Result<Decoded, EntityRefusal> {
        if self.value.is_some() {
            return self.value_object(owner);
        }
        if self.list.is_some() {
            return self.list_object(owner);
        }
        self.node(owner)
    }

    /// A value object: a literal, a JSON literal or the Null.
    fn value_object(self, owner: Option<&AttributeName>) -> Result<Decoded, EntityRefusal> {
        let ElementParts {
            value,
            types,
            language,
            direction,
            list,
            id,
            members,
        } = self;
        if list.is_some() || id.is_some() || !members.is_empty() {
            return Err(malformed(owner));
        }
        let Some(value) = value else {
            return Err(malformed(owner));
        };
        if matches!(&value, RawScalar::String(text) if is_ngsi_ld_null(text)) {
            return match language {
                Some(tag) => LangTagBuf::new(tag).map(Decoded::TaggedNull).map_err(|_| malformed(owner)),
                None => Ok(Decoded::Null),
            };
        }
        let datatype = match types {
            None => None,
            Some(TypeEntries::Single(datatype)) if datatype == JSON_DATATYPE => {
                if language.is_some() || direction.is_some() {
                    return Err(malformed(owner));
                }
                return Ok(Decoded::Value(ExpandedValue::Json(JsonLiteral::new(value.into_json()))));
            }
            Some(TypeEntries::Single(datatype)) => Some(datatype.parse::<NgsiLdReference>().map_err(|_| malformed(owner))?),
            Some(TypeEntries::Array(_)) => return Err(malformed(owner)),
        };
        let scalar = match value {
            RawScalar::Number(number) => LiteralValue::Number(JsonNumber::try_from(&number).map_err(|_| malformed(owner))?),
            RawScalar::String(text) => LiteralValue::String(CompactString::from(text)),
            RawScalar::Boolean(flag) => LiteralValue::Boolean(flag),
            RawScalar::Json(_) => return Err(malformed(owner)),
        };
        let language = language.map(|tag| LangTagBuf::new(tag).map_err(|_| malformed(owner))).transpose()?;
        let direction = direction.map(|text| text.parse::<Direction>().map_err(|_| malformed(owner))).transpose()?;
        Literal::try_new(scalar, datatype, language, direction)
            .map(|literal| Decoded::Value(ExpandedValue::Literal(literal)))
            .map_err(|_| malformed(owner))
    }

    /// A list object, or the list families' deletion encoding.
    fn list_object(self, owner: Option<&AttributeName>) -> Result<Decoded, EntityRefusal> {
        if self.types.is_some() || self.id.is_some() || !self.members.is_empty() || self.language.is_some() || self.direction.is_some() {
            return Err(malformed(owner));
        }
        let items = self.list.unwrap_or_default();
        if matches!(items.as_slice(), [Decoded::Null | Decoded::TaggedNull(_)]) {
            return Ok(Decoded::NullList);
        }
        values_without_null(items, owner).map(|items| Decoded::Value(ExpandedValue::List(items)))
    }

    /// A node object or a node reference, or the Null as a bare `@id`.
    fn node(self, owner: Option<&AttributeName>) -> Result<Decoded, EntityRefusal> {
        if self.language.is_some() || self.direction.is_some() {
            return Err(malformed(owner));
        }
        let types = match self.types {
            None => Vec::new(),
            Some(TypeEntries::Array(types)) => types
                .into_iter()
                .map(|entry| entry.parse::<ExpandedValueName>().map_err(EntityRefusal::ExpandedMemberName))
                .collect::<Result<Vec<_>, _>>()?,
            Some(TypeEntries::Single(_)) => return Err(malformed(owner)),
        };
        let id = match self.id {
            Some(id) if is_ngsi_ld_null(&id) => {
                return if types.is_empty() && self.members.is_empty() {
                    Ok(Decoded::Null)
                } else {
                    Err(EntityRefusal::NullBesideData { attribute: owner.cloned() })
                };
            }
            Some(id) => Some(NodeId::from(id.as_str())),
            None => None,
        };
        let members = self
            .members
            .into_iter()
            .map(|(name, values)| fragment_member(values, owner).map(|member| (name, member)))
            .collect::<Result<IndexMap<_, _>, _>>()?;
        Ok(Decoded::Value(ExpandedValue::Node(ValueNode::new(id, types, members))))
    }
}

/// One member of a structured value: its values, or the clause 5.5.12
/// deletion when it is exactly the Null.
fn fragment_member(values: Vec<Decoded>, owner: Option<&AttributeName>) -> Result<FragmentMember, EntityRefusal> {
    if matches!(values.as_slice(), [Decoded::Null | Decoded::TaggedNull(_)]) {
        return Ok(FragmentMember::Remove);
    }
    values_without_null(values, owner).map(FragmentMember::Values)
}

/// The values of `elements`, refusing a Null among them.
pub(crate) fn values_without_null(elements: Vec<Decoded>, owner: Option<&AttributeName>) -> Result<Vec<ExpandedValue<Fragment>>, EntityRefusal> {
    elements
        .into_iter()
        .map(|element| match element {
            Decoded::Value(value) => Ok(value),
            Decoded::Null | Decoded::TaggedNull(_) | Decoded::NullList => Err(EntityRefusal::NullBesideData { attribute: owner.cloned() }),
        })
        .collect()
}

/// The refusal of a nested value not shaped as the expanded form shapes it.
fn malformed(owner: Option<&AttributeName>) -> EntityRefusal {
    EntityRefusal::Malformed {
        attribute: owner.cloned(),
        member: MemberShape::NestedValue,
    }
}
