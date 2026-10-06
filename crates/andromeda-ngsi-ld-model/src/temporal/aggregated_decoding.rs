//! Reading the aggregated representation of a Temporal Evolution a peer
//! answered with (ETSI GS CIM 009 clauses 4.5.19 and 5.7.4.4).
//!
//! An aggregated attribute names its family in `type`, its `datasetId`, and for
//! every requested method one array of `[value, start, end]` triples, nested
//! one map key per grouping term under the grouped extension. The aggregates
//! are read as the peer computed them: two aggregates of one period are no
//! aggregate of their union, so nothing here combines them.

use crate::{
    attribute::instance_member::ReadError,
    compact_member::{decoded, decoded_optional},
    entity_type::EntityTypeValue,
    node_refusal::{NodeReadError, NodeRefusal, RefusedMember},
    temporal::{
        GroupKeyValue,
        aggregated_entity::{AggregateValue, AggregatedAttribute, AggregatedTemporalEntity, AggregatedTriple, AggregatedValues, MethodAggregate},
    },
};
use andromeda_ngsi_ld_lexicon::{aggr_method::AggrMethod, attribute_name::AttributeName, attribute_type::AttributeType, identifier::Identifier};
use andromeda_ngsi_ld_number::JsonNumber;
use andromeda_ngsi_ld_representation::compact_node::{CompactNode, NodeShape};
use andromeda_ngsi_ld_vocabulary::NULL_URI;
use chrono::{DateTime, Utc};
use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use std::str::FromStr;
use strum::ParseError;
use thiserror::Error;

/// The member an entity's identifier is written under.
const ID_MEMBER: &str = "id";

/// The member an entity's types are written under.
const TYPE_MEMBER: &str = "type";

/// The member a JSON-LD document names its context in.
const CONTEXT_MEMBER: &str = "@context";

/// The member an aggregated attribute names its family in.
const FAMILY_MEMBER: &str = "type";

/// The member an aggregated attribute names its dataset in.
const DATASET_MEMBER: &str = "datasetId";

/// What is wrong with an aggregated attribute.
#[derive(Debug, Error)]
pub enum AggregateRefusal {
    /// A member beside `type` and `datasetId` names no aggregation method; the
    /// refusal locates the member.
    #[error("the member names no aggregation method: {0}")]
    UnknownMethod(#[source] ParseError),
    /// An aggregate is neither an array of triples nor a map of them.
    #[error("an aggregate is an array of [value, start, end] triples, or a map of them")]
    NotAnAggregate,
    /// A triple is not an array of three elements.
    #[error("an aggregate triple is an array of the value, the start and the end of its period")]
    NotATriple,
    /// The value of a triple is no JSON null, number, string or boolean.
    #[error("the value of an aggregate is a JSON null, number, string or boolean")]
    ValueShape,
    /// The start or the end of a period is no RFC 3339 instant.
    #[error("the bounds of a period are RFC 3339 instants or null")]
    Instant,
    /// An aggregated attribute names no family.
    #[error("an aggregated attribute names its family in `type`")]
    MissingFamily,
    /// An aggregated attribute holds no aggregate: it is no more an aggregate
    /// than the instance of an attribute that lacks its value.
    #[error("an aggregated attribute holds the aggregates of at least one method")]
    NoMethod,
    /// An aggregated attribute is no JSON object.
    #[error("an aggregated attribute is a JSON object")]
    NotAnObject,
}

impl AggregatedTemporalEntity {
    /// The aggregated entity a compact node is.
    ///
    /// # Errors
    ///
    /// Returns [`NodeReadError::Refused`] for a node that is no entity, one
    /// without `id` or `type`, or an attribute that holds no aggregate the
    /// representation defines, and [`NodeReadError::Unreadable`] when a node of
    /// it has no JSON form.
    pub fn from_node<'n, N: CompactNode<'n>>(node: N) -> Result<AggregatedTemporalEntity, ReadError<'n, N>> {
        let NodeShape::Object(members) = node.shape() else {
            return Err(NodeRefusal::NotAnObject.into());
        };
        let mut id: Option<Identifier> = None;
        let mut type_: Option<EntityTypeValue> = None;
        let mut attrs: IndexMap<AttributeName, Vec<AggregatedAttribute>> = IndexMap::new();
        for (name, member) in members {
            let at = |error: ReadError<'n, N>| error.within_member(&name);
            match name.as_ref() {
                CONTEXT_MEMBER => {}
                ID_MEMBER => id = Some(required(member, RefusedMember::Id).map_err(at)?),
                TYPE_MEMBER => type_ = Some(required(member, RefusedMember::Type).map_err(at)?),
                attribute => {
                    let attribute_name = AttributeName::from_str(attribute).map_err(|source| at(NodeRefusal::AttributeName(source).into()))?;
                    attrs.insert(attribute_name, attribute_instances(member).map_err(at)?);
                }
            }
        }
        Ok(AggregatedTemporalEntity {
            id: id.ok_or(NodeRefusal::Missing { member: RefusedMember::Id })?,
            type_: type_.ok_or(NodeRefusal::Missing { member: RefusedMember::Type })?,
            attrs,
        })
    }
}

/// Read a member the entity requires.
fn required<'n, T: DeserializeOwned, N: CompactNode<'n>>(node: N, member: RefusedMember) -> Result<T, ReadError<'n, N>> {
    decoded(node)
        .map_err(NodeReadError::Unreadable)?
        .map_err(|source| NodeRefusal::Member { member, source }.into())
}

/// The aggregated instances of one attribute: one object, or an array of them.
fn attribute_instances<'n, N: CompactNode<'n>>(node: N) -> Result<Vec<AggregatedAttribute>, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Array(items) => items
            .into_iter()
            .enumerate()
            .map(|(index, item)| aggregated_attribute(item).map_err(|error| error.within_element(index)))
            .collect(),
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            Ok(vec![aggregated_attribute(node)?])
        }
    }
}

/// One aggregated attribute instance.
fn aggregated_attribute<'n, N: CompactNode<'n>>(node: N) -> Result<AggregatedAttribute, ReadError<'n, N>> {
    let NodeShape::Object(members) = node.shape() else {
        return Err(NodeRefusal::Aggregate(AggregateRefusal::NotAnObject).into());
    };
    let mut family: Option<AttributeType> = None;
    let mut dataset_id: Option<Identifier> = None;
    let mut aggregates = Vec::new();
    for (name, member) in members {
        let at = |error: ReadError<'n, N>| error.within_member(&name);
        match name.as_ref() {
            FAMILY_MEMBER => family = Some(required(member, RefusedMember::Type).map_err(at)?),
            DATASET_MEMBER => {
                dataset_id = decoded_optional(member).map_err(NodeReadError::Unreadable).map_err(at)?.map_err(|source| {
                    at(NodeRefusal::Member {
                        member: RefusedMember::DatasetId,
                        source,
                    }
                    .into())
                })?;
            }
            method => {
                let method = AggrMethod::from_str(method).map_err(|source| at(NodeRefusal::Aggregate(AggregateRefusal::UnknownMethod(source)).into()))?;
                aggregates.push(MethodAggregate {
                    method,
                    values: aggregated_values(member).map_err(at)?,
                });
            }
        }
    }
    if aggregates.is_empty() {
        return Err(NodeRefusal::Aggregate(AggregateRefusal::NoMethod).into());
    }
    Ok(AggregatedAttribute {
        family: family.ok_or(NodeRefusal::Aggregate(AggregateRefusal::MissingFamily))?,
        dataset_id,
        aggregates,
    })
}

/// What one method aggregates to: the triples, or the triples nested by group
/// key.
fn aggregated_values<'n, N: CompactNode<'n>>(node: N) -> Result<AggregatedValues, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Array(triples) => triples
            .into_iter()
            .enumerate()
            .map(|(index, triple)| aggregated_triple(triple).map_err(|error| error.within_element(index)))
            .collect::<Result<Vec<_>, _>>()
            .map(AggregatedValues::Triples),
        NodeShape::Object(groups) => {
            let mut grouped = IndexMap::with_capacity(groups.len());
            for (key, group) in groups {
                let nested = aggregated_values(group).map_err(|error| error.within_member(&key))?;
                grouped.insert(group_key(&key), nested);
            }
            Ok(AggregatedValues::Grouped(grouped))
        }
        NodeShape::Null | NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::String(_) | NodeShape::Geometry(_) => {
            Err(NodeRefusal::Aggregate(AggregateRefusal::NotAnAggregate).into())
        }
    }
}

/// The group a map key names: the reserved keys of an absent and a Null value,
/// or the group value itself.
fn group_key(key: &str) -> GroupKeyValue {
    if key == GroupKeyValue::NONE_KEY {
        GroupKeyValue::Absent
    } else if key == NULL_URI.as_str() {
        GroupKeyValue::Null
    } else {
        GroupKeyValue::Value(key.to_owned())
    }
}

/// One `[value, start, end]` triple.
fn aggregated_triple<'n, N: CompactNode<'n>>(node: N) -> Result<AggregatedTriple, ReadError<'n, N>> {
    let NodeShape::Array(elements) = node.shape() else {
        return Err(NodeRefusal::Aggregate(AggregateRefusal::NotATriple).into());
    };
    let [value, start, end] = <[N; 3]>::try_from(elements).map_err(|_| NodeRefusal::Aggregate(AggregateRefusal::NotATriple))?;
    Ok(AggregatedTriple {
        value: aggregate_value(value)?,
        start: instant(start)?,
        end: instant(end)?,
    })
}

/// The value of one period.
fn aggregate_value<'n, N: CompactNode<'n>>(node: N) -> Result<AggregateValue, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Null => Ok(AggregateValue::Absent),
        NodeShape::Boolean(flag) => Ok(AggregateValue::Boolean(flag)),
        NodeShape::Number(text) => JsonNumber::from_str(&text)
            .map(AggregateValue::Number)
            .map_err(|_| NodeRefusal::Aggregate(AggregateRefusal::ValueShape).into()),
        NodeShape::String(text) => Ok(AggregateValue::Text(text.into_owned())),
        NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => Err(NodeRefusal::Aggregate(AggregateRefusal::ValueShape).into()),
    }
}

/// The bound of a period: an instant, or null.
fn instant<'n, N: CompactNode<'n>>(node: N) -> Result<Option<DateTime<Utc>>, ReadError<'n, N>> {
    match node.shape() {
        NodeShape::Null => Ok(None),
        NodeShape::String(text) => DateTime::parse_from_rfc3339(&text)
            .map(|parsed| Some(parsed.with_timezone(&Utc)))
            .map_err(|_| NodeRefusal::Aggregate(AggregateRefusal::Instant).into()),
        NodeShape::Boolean(_) | NodeShape::Number(_) | NodeShape::Array(_) | NodeShape::Object(_) | NodeShape::Geometry(_) => {
            Err(NodeRefusal::Aggregate(AggregateRefusal::Instant).into())
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::temporal::aggregated_entity::{AggregateValue, AggregatedTemporalEntity, AggregatedValues};
    use andromeda_json_literal::piece::JsonPiece;
    use pretty_assertions::assert_eq;
    use serde_json::{Value, json, value::RawValue};

    fn read(document: &Value) -> AggregatedTemporalEntity {
        let raw = RawValue::from_string(document.to_string()).expect("raw JSON");
        AggregatedTemporalEntity::from_node(JsonPiece::new(&raw)).expect("an aggregated entity")
    }

    /// The aggregated entity a peer wrote is written back as it was read.
    #[test]
    fn an_aggregated_answer_is_read_triple_by_triple() {
        let entity = read(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "weight": {"type": "Property", "datasetId": "urn:ngsi-ld:Dataset:a", "avg": [[2.5, "2026-01-01T00:00:00Z", "2026-01-01T01:00:00Z"], [null, null, null]]}
        }));
        let attribute = &entity.attrs["weight"][0];
        assert_eq!(attribute.dataset_id.as_ref().map(ToString::to_string), Some("urn:ngsi-ld:Dataset:a".to_owned()));
        let AggregatedValues::Triples(triples) = &attribute.aggregates[0].values else {
            panic!("the answer is not grouped");
        };
        assert_eq!(triples.len(), 2);
        assert!(matches!(triples[0].value, AggregateValue::Number(_)));
        assert!(matches!(triples[1].value, AggregateValue::Absent));
        assert!(triples[0].start.is_some() && triples[1].start.is_none());
    }

    /// The grouped extension nests the triples under one map key per term.
    #[test]
    fn a_grouped_answer_is_read_by_its_group_keys() {
        let entity = read(&json!({
            "id": "urn:ngsi-ld:Vehicle:1",
            "type": "Vehicle",
            "weight": {"type": "Property", "totalCount": {"KGM": [[2, null, null]], "@none": [[1, null, null]]}}
        }));
        let AggregatedValues::Grouped(groups) = &entity.attrs["weight"][0].aggregates[0].values else {
            panic!("the answer is grouped");
        };
        assert_eq!(groups.len(), 2);
    }

    /// An attribute holding no aggregate is no aggregate.
    #[test]
    fn an_attribute_without_a_method_is_refused() {
        let raw = RawValue::from_string(json!({"id": "urn:a", "type": "T", "w": {"type": "Property"}}).to_string()).expect("raw JSON");
        assert!(AggregatedTemporalEntity::from_node(JsonPiece::new(&raw)).is_err());
    }

    /// A normalized instance is no aggregate: it names no method, so a peer's
    /// instance answer is never mistaken for one.
    #[test]
    fn a_member_naming_no_method_is_refused() {
        let raw = RawValue::from_string(json!({"id": "urn:a", "type": "T", "w": {"type": "Property", "colour": "red"}}).to_string()).expect("raw JSON");
        assert!(AggregatedTemporalEntity::from_node(JsonPiece::new(&raw)).is_err());
    }
}
