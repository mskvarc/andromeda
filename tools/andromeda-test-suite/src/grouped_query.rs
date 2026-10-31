//! Query Temporal Evolution of Entities with the grouping extension (`format=groupedValues`), whose answer is an array
//! of grouped aggregated temporal entities: the request is the temporal query path with the query string written as
//! given, and the answer is read entity by entity through [`GroupedEntity`].

use crate::{
    broker::BrokerClient,
    grouped_aggregation::{GroupedEntity, GroupedError},
    wire::{WireError, request_to},
};
use bytes::Bytes;
use http::{HeaderValue, Method, Response, header::ACCEPT};
use ngsi_ld::http::UncheckedRequest;
use serde_json::Value;

/// The request of Query Temporal Evolution of Entities with the query string `query` written as given, and
/// `Accept: application/json`.
///
/// # Errors
///
/// Returns [`WireError`] when the target is not a URI.
pub fn grouped_query_request(client: &BrokerClient, query: &str) -> Result<UncheckedRequest, WireError> {
    Ok(request_to(client, Method::GET, &format!("temporal/entities?{query}"))?.append_header(ACCEPT, HeaderValue::from_static("application/json")))
}

/// The grouped aggregated temporal entities the body of `response` holds, in the order listed.
///
/// # Errors
///
/// Returns [`GroupedError`] when the body is not an array of grouped aggregated temporal entities.
pub fn grouped_entities(response: &Response<Bytes>) -> Result<Vec<GroupedEntity>, GroupedError> {
    let document: Value = serde_json::from_slice(response.body()).map_err(GroupedError::NotJson)?;
    let Value::Array(items) = document else {
        return Err(GroupedError::NotArray);
    };
    items
        .iter()
        .map(|item| GroupedEntity::read(&Response::new(Bytes::from(item.to_string()))))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::grouped_query::grouped_entities;
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;

    #[gtest]
    fn an_array_of_grouped_entities_is_read_entity_by_entity() -> Result<()> {
        let answer = Response::new(Bytes::from_static(
            br#"[{"id":"urn:ngsi-ld:Vehicle:1","type":"Vehicle","speed":{"type":"Property","avg":{"2024-01-01T00:00:00Z":[[10,"a","b"]]}}}]"#,
        ));
        let entities = grouped_entities(&answer)?;
        expect_that!(entities.len(), eq(1));
        let series = entities
            .first()
            .and_then(|entity| entity.attribute("speed"))
            .and_then(|attribute| attribute.single())
            .or_fail()?;
        verify_that!(series.group_value("avg", "2024-01-01T00:00:00Z"), some(eq(10.0)))
    }

    #[gtest]
    fn a_body_that_is_no_array_is_refused() {
        expect_that!(grouped_entities(&Response::new(Bytes::from_static(b"{}"))).is_err(), eq(true));
    }
}
