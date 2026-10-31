//! The temporal entities of a page read from an answer the typed client did not send for: the request followed a
//! link with a parameter added to it, so the answer is a body to read, not a page.

use bytes::Bytes;
use http::Response;
use ngsi_ld::model::{DocumentError, Lossless, Output, evolution::TemporalEntity};
use serde_json::Value;

/// Why an answer is not an array of temporal entities.
#[derive(Debug, thiserror::Error)]
pub enum TemporalPageError {
    /// The body is not JSON.
    #[error("the body is not JSON")]
    NotJson(#[source] serde_json::Error),
    /// The body is not an array.
    #[error("the body is not an array of temporal entities")]
    NotAnArray,
    /// An item of the body is not a temporal entity.
    #[error("an item of the page is not a temporal entity")]
    Entity(#[source] DocumentError),
}

/// The temporal entities of the body of `response`, an array in the normalized representation, in its order.
///
/// # Errors
///
/// Returns [`TemporalPageError`] when the body is not an array of temporal entities.
pub fn entities(response: &Response<Bytes>) -> Result<Vec<TemporalEntity<Output>>, TemporalPageError> {
    let document: Value = serde_json::from_slice(response.body()).map_err(TemporalPageError::NotJson)?;
    let Value::Array(items) = document else {
        return Err(TemporalPageError::NotAnArray);
    };
    items
        .iter()
        .map(|item| TemporalEntity::<Output>::parse(&item.to_string(), Lossless::Normalized).map_err(TemporalPageError::Entity))
        .collect()
}

#[cfg(test)]
mod tests {
    use crate::temporal_page_body::{TemporalPageError, entities};
    use bytes::Bytes;
    use googletest::prelude::*;
    use http::Response;

    fn answering(body: &'static str) -> Response<Bytes> {
        Response::new(Bytes::from_static(body.as_bytes()))
    }

    #[gtest]
    fn an_array_of_temporal_entities_is_read_in_order() -> Result<()> {
        let page = entities(&answering(
            r#"[{"id":"urn:ngsi-ld:Sensor:1","type":"Sensor","reading":[{"type":"Property","value":1,"observedAt":"2020-01-01T00:00:00Z"}]},
            {"id":"urn:ngsi-ld:Sensor:2","type":"Sensor"}]"#,
        ))?;
        expect_that!(page, len(eq(2)));
        verify_that!(page.first().map(|entity| entity.id.to_string()), some(eq("urn:ngsi-ld:Sensor:1")))
    }

    #[gtest]
    fn a_body_that_is_not_an_array_is_refused() {
        expect_that!(
            entities(&answering(r#"{"id":"urn:ngsi-ld:Sensor:1"}"#)),
            err(predicate(|error: &TemporalPageError| matches!(error, TemporalPageError::NotAnArray)))
        );
    }

    #[gtest]
    fn an_item_that_is_not_a_temporal_entity_is_refused() {
        expect_that!(
            entities(&answering(r#"[{"type":"Sensor"}]"#)),
            err(predicate(|error: &TemporalPageError| matches!(error, TemporalPageError::Entity(_))))
        );
    }
}
