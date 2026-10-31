//! The grouped aggregated temporal representation a broker with the temporal aggregation grouping extension answers
//! (`format=groupedValues`): per attribute, one aggregate map per method holding a list of periods for every group
//! key. The extension is Andromeda's own, so the typed model has no reader for it; the answer is read here, and the
//! request is the typed retrieve sent as written.

use crate::{
    broker::BrokerClient,
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{HeaderValue, Method, Response, header::ACCEPT};
use ngsi_ld::{
    http::UncheckedRequest,
    model::{EntityId, EntityTypeName, NameError, Uri, UriError},
};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

/// Why an answer is not a grouped aggregated temporal entity.
#[derive(Debug, thiserror::Error)]
pub enum GroupedError {
    /// The body is not JSON.
    #[error("the body is not JSON")]
    NotJson(#[source] serde_json::Error),
    /// The body of one entity is not an object.
    #[error("the body is not an object")]
    NotObject,
    /// The body of a list of entities is not an array.
    #[error("the body is not an array")]
    NotArray,
    /// The member `id` is missing or not a string.
    #[error("the member id is not a string")]
    IdNotString,
    /// The member `type` is missing or not a string.
    #[error("the member type is not a string")]
    TypeNotString,
    /// The member `id` is not an entity id.
    #[error("the id is not an entity id")]
    Id(#[source] UriError),
    /// The member `type` is not a type name.
    #[error("the type is not a type name")]
    Type(#[source] NameError),
    /// A `datasetId` is not a URI.
    #[error("datasetId {0} is not a URI")]
    DatasetId(Value),
    /// The periods of a group hold no first period whose first item is a number.
    #[error("the group periods {0} hold no period with a number")]
    GroupWithoutNumber(Value),
    /// An item of an attribute written as an array is not an object.
    #[error("the attribute item {0} is not an object")]
    ItemNotObject(Value),
    /// An attribute is neither an object nor an array.
    #[error("the attribute {0} is neither an object nor an array")]
    AttributeShape(Value),
}

/// The request of Retrieve Temporal Evolution of the entity `id` with the query string `query` written as given, and
/// `Accept: application/json`.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn grouped_request(client: &BrokerClient, id: &EntityId, query: &str) -> Result<UncheckedRequest, WireError> {
    Ok(request_to(client, Method::GET, &format!("temporal/entities/{id}?{query}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json")))
}

/// What one attribute holds for one dataset (or for the attribute as a whole): the aggregate of each method per group
/// key.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupedSeries {
    /// The dataset the series aggregates, when the answer names one.
    pub dataset_id: Option<Uri>,
    /// For each method, the first aggregate value of the first period of each group.
    aggregates: BTreeMap<String, BTreeMap<String, f64>>,
}

impl GroupedSeries {
    /// The first aggregate value of `method` for the group `group`.
    #[must_use]
    pub fn group_value(&self, method: &str, group: &str) -> Option<f64> {
        self.aggregates.get(method).and_then(|groups| groups.get(group)).copied()
    }

    /// The number of groups `method` holds, 0 when the series has no such method.
    #[must_use]
    pub fn group_count(&self, method: &str) -> usize {
        self.aggregates.get(method).map_or(0, BTreeMap::len)
    }

    /// The series of one attribute member.
    fn read(member: &Map<String, Value>) -> Result<GroupedSeries, GroupedError> {
        let dataset_id = member
            .get("datasetId")
            .map(|dataset| {
                dataset
                    .as_str()
                    .and_then(|text| text.parse::<Uri>().ok())
                    // The error owns the value it names; the answer keeps its own.
                    .ok_or_else(|| GroupedError::DatasetId(dataset.clone()))
            })
            .transpose()?;
        let aggregates = member
            .iter()
            .filter_map(|(method, groups)| groups.as_object().map(|groups| (method, groups)))
            .map(|(method, groups)| Ok((method.clone(), GroupedSeries::groups(groups)?)))
            .collect::<Result<BTreeMap<_, _>, GroupedError>>()?;
        Ok(GroupedSeries { dataset_id, aggregates })
    }

    /// The first aggregate value of each group of one method.
    fn groups(groups: &Map<String, Value>) -> Result<BTreeMap<String, f64>, GroupedError> {
        groups
            .iter()
            .map(|(group, periods)| {
                periods
                    .as_array()
                    .and_then(|periods| periods.first())
                    .and_then(Value::as_array)
                    .and_then(|period| period.first())
                    .and_then(Value::as_f64)
                    .map(|value| (group.clone(), value))
                    // The error owns the periods it names; the answer keeps its own.
                    .ok_or_else(|| GroupedError::GroupWithoutNumber(periods.clone()))
            })
            .collect()
    }
}

/// What an attribute holds in a grouped answer: one series, or one per dataset.
#[derive(Debug, Clone, PartialEq)]
pub enum GroupedAttribute {
    /// The attribute is one object.
    Single(GroupedSeries),
    /// The attribute is an array holding one series per dataset.
    PerDataset(Vec<GroupedSeries>),
}

impl GroupedAttribute {
    /// The one series of an attribute written as an object.
    #[must_use]
    pub const fn single(&self) -> Option<&GroupedSeries> {
        match self {
            GroupedAttribute::Single(series) => Some(series),
            GroupedAttribute::PerDataset(_) => None,
        }
    }

    /// The number of series of an attribute written as an array, `None` for an attribute written as an object.
    #[must_use]
    pub const fn series_count(&self) -> Option<usize> {
        match self {
            GroupedAttribute::Single(_) => None,
            GroupedAttribute::PerDataset(series) => Some(series.len()),
        }
    }

    /// The series of the dataset `dataset_id`, in an attribute written as an array.
    #[must_use]
    pub fn series_of(&self, dataset_id: &Uri) -> Option<&GroupedSeries> {
        match self {
            GroupedAttribute::Single(_) => None,
            GroupedAttribute::PerDataset(series) => series.iter().find(|candidate| candidate.dataset_id.as_ref() == Some(dataset_id)),
        }
    }
}

/// A grouped aggregated temporal entity.
#[derive(Debug, Clone, PartialEq)]
pub struct GroupedEntity {
    /// The entity's id.
    pub id: EntityId,
    /// The entity's type.
    pub entity_type: EntityTypeName,
    /// The aggregated attributes, by the name the answer gives them.
    attributes: BTreeMap<String, GroupedAttribute>,
}

impl GroupedEntity {
    /// Reads the body of `response`.
    ///
    /// # Errors
    ///
    /// Returns [`GroupedError`] when the body is not a grouped aggregated temporal entity.
    pub fn read(response: &Response<Bytes>) -> Result<GroupedEntity, GroupedError> {
        let document: Value = serde_json::from_slice(response.body()).map_err(GroupedError::NotJson)?;
        let Value::Object(members) = document else {
            return Err(GroupedError::NotObject);
        };
        let id = members
            .get("id")
            .and_then(Value::as_str)
            .ok_or(GroupedError::IdNotString)?
            .parse::<EntityId>()
            .map_err(GroupedError::Id)?;
        let entity_type = members
            .get("type")
            .and_then(Value::as_str)
            .ok_or(GroupedError::TypeNotString)?
            .parse::<EntityTypeName>()
            .map_err(GroupedError::Type)?;
        let attributes = members
            .iter()
            .filter(|(name, _)| !matches!(name.as_str(), "id" | "type" | "@context"))
            .map(|(name, member)| Ok((name.clone(), GroupedEntity::read_attribute(member)?)))
            .collect::<Result<BTreeMap<_, _>, GroupedError>>()?;
        Ok(GroupedEntity { id, entity_type, attributes })
    }

    /// The attribute named `name`, as the answer writes the name.
    #[must_use]
    pub fn attribute(&self, name: &str) -> Option<&GroupedAttribute> {
        self.attributes.get(name)
    }

    /// One attribute member: an object or an array of objects.
    fn read_attribute(member: &Value) -> Result<GroupedAttribute, GroupedError> {
        match member {
            Value::Object(object) => Ok(GroupedAttribute::Single(GroupedSeries::read(object)?)),
            Value::Array(items) => items
                .iter()
                .map(|item| {
                    item.as_object()
                        // The error owns the item it names; the answer keeps its own.
                        .ok_or_else(|| GroupedError::ItemNotObject(item.clone()))
                        .and_then(GroupedSeries::read)
                })
                .collect::<Result<Vec<_>, _>>()
                .map(GroupedAttribute::PerDataset),
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Err(GroupedError::AttributeShape(member.clone())),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::grouped_aggregation::{GroupedAttribute, GroupedEntity};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;
    use ngsi_ld::model::Uri;

    fn answer(text: &str) -> Response<Bytes> {
        Response::new(Bytes::from(text.to_owned()))
    }

    #[gtest]
    fn an_attribute_written_as_an_object_is_one_series_of_groups() -> Result<()> {
        let entity = GroupedEntity::read(&answer(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","avg":{"KMH":[[15,"2024-01-01T00:00:00Z","2024-03-01T00:00:00Z"]],"HM":[[35,"a","b"]]}}}"#,
        ))?;
        let series = entity.attribute("speed").and_then(GroupedAttribute::single).or_fail()?;
        expect_that!(series.group_value("avg", "KMH"), some(eq(15.0)));
        expect_that!(series.group_value("avg", "HM"), some(eq(35.0)));
        expect_that!(series.group_count("avg"), eq(2));
        verify_that!(series.group_count("sum"), eq(0))
    }

    #[gtest]
    fn an_attribute_written_as_an_array_is_one_series_per_dataset() -> Result<()> {
        let entity = GroupedEntity::read(&answer(
            r#"{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":[{"type":"Property","datasetId":"urn:ngsi-ld:Dataset:x-A","avg":{"urn:ngsi-ld:Dataset:x-A":[[15,"a","b"]]}},{"type":"Property","datasetId":"urn:ngsi-ld:Dataset:x-B","avg":{"urn:ngsi-ld:Dataset:x-B":[[35,"a","b"]]}}]}"#,
        ))?;
        let attribute = entity.attribute("speed").or_fail()?;
        expect_that!(attribute.series_count(), some(eq(2)));
        let dataset = "urn:ngsi-ld:Dataset:x-B".parse::<Uri>()?;
        let series = attribute.series_of(&dataset).or_fail()?;
        verify_that!(series.group_value("avg", "urn:ngsi-ld:Dataset:x-B"), some(eq(35.0)))
    }

    #[gtest]
    fn a_body_of_another_shape_is_refused() {
        expect_that!(GroupedEntity::read(&answer("[]")).is_err(), eq(true));
        expect_that!(GroupedEntity::read(&answer(r#"{"id":"urn:ngsi-ld:Vehicle:1"}"#)).is_err(), eq(true));
    }
}
