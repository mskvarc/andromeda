//! A condition on the body of a request a test's server received, the way a stub mapping's `bodyPatterns` states
//! one: text the body holds or does not hold, or a `JSONPath` query that selects something from it as JSON. A test
//! reads what its server received through these to pick out the requests it is about.

use serde_json_path::JsonPath;
use wiremock::{Match, Request};

/// One condition on a received body.
#[derive(Debug, Clone)]
pub enum BodyPattern {
    /// The body holds the text.
    Contains(String),
    /// The body does not hold the text.
    DoesNotContain(String),
    /// The body is JSON and the query selects at least one node of it.
    Selects(JsonPath),
    /// The body is JSON and the filter query, starting with a filter, selects a node from one of the nodes the first
    /// query selects. A stub mapping's `JSONPath` applies a filter to an object as to an array holding it, at any
    /// depth, which RFC 9535 does not: `$.label[?(@.type == 'Property')]` selects the `label` object itself.
    SelectsBelow(JsonPath, JsonPath),
}

impl BodyPattern {
    /// Whether `body` meets the condition.
    #[must_use]
    pub fn holds(&self, body: &[u8]) -> bool {
        match self {
            BodyPattern::Contains(text) => String::from_utf8_lossy(body).contains(text.as_str()),
            BodyPattern::DoesNotContain(text) => !String::from_utf8_lossy(body).contains(text.as_str()),
            BodyPattern::Selects(query) => serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|json| selects(query, json)),
            BodyPattern::SelectsBelow(prefix, filter) => {
                serde_json::from_slice::<serde_json::Value>(body).is_ok_and(|json| prefix.query(&json).into_iter().any(|node| filters(filter, node)))
            }
        }
    }
}

/// Whether `query` selects a node of `json`. A stub mapping's `JSONPath` applies a filter at the root to a root
/// object as to an array holding it (`$[?(@.id == 'x')]` selects an entity posted alone), which RFC 9535 does not, so
/// a root object is also queried as that array.
fn selects(query: &JsonPath, json: serde_json::Value) -> bool {
    if !query.query(&json).is_empty() {
        return true;
    }
    json.is_object() && !query.query(&serde_json::Value::Array(vec![json])).is_empty()
}

/// Whether `filter` selects a node of `node`: of the array itself, or of an object as the array holding it alone.
fn filters(filter: &JsonPath, node: &serde_json::Value) -> bool {
    match node {
        serde_json::Value::Array(_) => !filter.query(node).is_empty(),
        // The filter reads the object as one item; cloning hands that array the object it holds.
        serde_json::Value::Object(_) => !filter.query(&serde_json::Value::Array(vec![node.clone()])).is_empty(),
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) | serde_json::Value::String(_) => false,
    }
}

impl Match for BodyPattern {
    fn matches(&self, request: &Request) -> bool {
        self.holds(&request.body)
    }
}

#[cfg(test)]
mod tests {
    use crate::body_pattern::BodyPattern;
    use googletest::prelude::*;

    #[gtest]
    fn text_conditions_read_the_body_as_text() -> Result<()> {
        let body = br#"{"data":[{"id":"urn:ngsi-ld:V:1"}]}"#;
        expect_that!(BodyPattern::Contains("V:1".to_owned()).holds(body), eq(true));
        verify_that!(BodyPattern::DoesNotContain("V:1".to_owned()).holds(body), eq(false))
    }

    #[gtest]
    fn a_query_selects_from_a_json_body_only() -> Result<()> {
        let selects = BodyPattern::Selects("$.data[?(@.id == 'urn:ngsi-ld:V:1')]".parse()?);
        expect_that!(selects.holds(br#"{"data":[{"id":"urn:ngsi-ld:V:1"}]}"#), eq(true));
        expect_that!(selects.holds(br#"{"data":[{"id":"urn:ngsi-ld:V:2"}]}"#), eq(false));
        verify_that!(selects.holds(b"not json"), eq(false))
    }

    #[gtest]
    fn a_root_filter_selects_an_object_posted_alone() -> Result<()> {
        let selects = BodyPattern::Selects("$[?(@.id == 'urn:ngsi-ld:V:1')]".parse()?);
        expect_that!(selects.holds(br#"{"id":"urn:ngsi-ld:V:1"}"#), eq(true));
        expect_that!(selects.holds(br#"[{"id":"urn:ngsi-ld:V:1"}]"#), eq(true));
        verify_that!(selects.holds(br#"{"id":"urn:ngsi-ld:V:2"}"#), eq(false))
    }

    #[gtest]
    fn a_filter_below_the_root_reads_an_object_as_one_item() -> Result<()> {
        let selects = BodyPattern::SelectsBelow("$.label".parse()?, "$[?(@.type == 'Property')].value".parse()?);
        expect_that!(selects.holds(br#"{"label":{"type":"Property","value":"a"}}"#), eq(true));
        expect_that!(
            selects.holds(br#"{"label":[{"type":"Property","value":"a"},{"type":"Relationship"}]}"#),
            eq(true)
        );
        expect_that!(selects.holds(br#"{"label":{"type":"Relationship","object":"urn:x"}}"#), eq(false));
        verify_that!(selects.holds(br#"{"label":{"inner":{"type":"Property","value":"a"}}}"#), eq(false))
    }

    #[gtest]
    fn a_filter_below_the_root_selects_from_an_array_as_written() -> Result<()> {
        let selects = BodyPattern::SelectsBelow("$.data".parse()?, "$[?(@.id == 'urn:ngsi-ld:V:1')]".parse()?);
        expect_that!(selects.holds(br#"{"data":[{"id":"urn:ngsi-ld:V:2"},{"id":"urn:ngsi-ld:V:1"}]}"#), eq(true));
        expect_that!(selects.holds(br#"{"data":[{"id":"urn:ngsi-ld:V:2"}]}"#), eq(false));
        verify_that!(selects.holds(br#"{"data":"urn:ngsi-ld:V:1"}"#), eq(false))
    }
}
