//! Which received requests a stub answers or a journal query selects, in the JSON shape of WireMock request
//! patterns: method, path or path pattern, query parameters, headers and body patterns.

use regex::Regex;
use serde::Deserialize;
use serde_json::Value as Json;
use serde_json_path::JsonPath;
use std::collections::BTreeMap;
use url::form_urlencoded;
use wiremock::Request;

/// A request pattern.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequestPattern {
    /// The method; `ANY` or none matches every method.
    method: Option<String>,
    /// The path, exactly.
    url_path: Option<String>,
    /// A regular expression the whole path matches.
    url_path_pattern: Option<String>,
    /// The query parameters, by name.
    #[serde(default)]
    query_parameters: BTreeMap<String, ValuePattern>,
    /// The headers, by name, matched without regard to case.
    #[serde(default)]
    headers: BTreeMap<String, ValuePattern>,
    /// What the body holds; every pattern must match.
    #[serde(default)]
    body_patterns: Vec<BodyPattern>,
}

/// What a header or query parameter value must be.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValuePattern {
    /// Equal to the text.
    equal_to: Option<String>,
    /// Holding the text.
    contains: Option<String>,
    /// Matching the whole regular expression.
    matches: Option<String>,
    /// Absent when true, present when false.
    absent: Option<bool>,
    /// Exactly these values, one pattern each.
    has_exactly: Option<Vec<ValuePattern>>,
}

/// What a body must hold.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BodyPattern {
    /// Holding the text.
    contains: Option<String>,
    /// Not holding the text.
    does_not_contain: Option<String>,
    /// A JSON body in which the JSON path selects at least one node.
    matches_json_path: Option<String>,
    /// A JSON body equal to this JSON text, numbers compared by value.
    equal_to_json: Option<String>,
    /// With `equal_to_json`: the body may hold members the text does not name, and arrays may hold items past the
    /// ones the text lists.
    #[serde(default)]
    ignore_extra_elements: bool,
}

/// Why a pattern is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PatternError {
    /// A regular expression is not valid.
    #[error("{0} is not a regular expression: {1}")]
    Regex(String, String),
    /// A JSON path is not valid.
    #[error("{0} is not a JSONPath: {1}")]
    JsonPath(String, String),
    /// The JSON an `equalToJson` pattern names is not valid.
    #[error("{0} is not JSON: {1}")]
    Json(String, String),
    /// A pattern states no requirement the server knows.
    #[error("a pattern states no known requirement")]
    Empty,
}

impl RequestPattern {
    /// Whether `request` matches the pattern.
    ///
    /// # Errors
    ///
    /// Returns [`PatternError`] when a regular expression or JSON path of the pattern is not valid.
    pub fn matches(&self, request: &Request) -> Result<bool, PatternError> {
        let method = self
            .method
            .as_deref()
            .is_none_or(|method| method == "ANY" || method.eq_ignore_ascii_case(request.method.as_str()));
        let path = request.url.path();
        let exact = self.url_path.as_deref().is_none_or(|expected| expected == path);
        let pattern = match &self.url_path_pattern {
            Some(pattern) => whole(pattern)?.is_match(path),
            None => true,
        };
        if !(method && exact && pattern) {
            return Ok(false);
        }
        for (name, expected) in &self.query_parameters {
            let values = form_urlencoded::parse(request.url.query().unwrap_or_default().as_bytes())
                .filter(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .collect::<Vec<_>>();
            if !expected.matches(&values)? {
                return Ok(false);
            }
        }
        for (name, expected) in &self.headers {
            let values = request
                .headers
                .get_all(name.as_str())
                .iter()
                .map(|value| String::from_utf8_lossy(value.as_bytes()).into_owned())
                .collect::<Vec<_>>();
            if !expected.matches(&values)? {
                return Ok(false);
            }
        }
        let body = String::from_utf8_lossy(&request.body);
        for expected in &self.body_patterns {
            if !expected.matches(&body)? {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl ValuePattern {
    /// Whether the values sent under one name match.
    fn matches(&self, values: &[String]) -> Result<bool, PatternError> {
        if let Some(absent) = self.absent {
            return Ok(absent == values.is_empty());
        }
        if let Some(patterns) = &self.has_exactly {
            if patterns.len() != values.len() {
                return Ok(false);
            }
            for value in values {
                let mut any = false;
                for pattern in patterns {
                    any |= pattern.single(value)?;
                }
                if !any {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        for value in values {
            if self.single(value)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Whether one value matches.
    fn single(&self, value: &str) -> Result<bool, PatternError> {
        if let Some(expected) = &self.equal_to {
            return Ok(value == expected);
        }
        if let Some(part) = &self.contains {
            return Ok(value.contains(part.as_str()));
        }
        if let Some(pattern) = &self.matches {
            return Ok(whole(pattern)?.is_match(value));
        }
        Err(PatternError::Empty)
    }
}

impl BodyPattern {
    /// Whether a body matches.
    fn matches(&self, body: &str) -> Result<bool, PatternError> {
        if let Some(part) = &self.contains {
            return Ok(body.contains(part.as_str()));
        }
        if let Some(part) = &self.does_not_contain {
            return Ok(!body.contains(part.as_str()));
        }
        if let Some(expression) = &self.matches_json_path {
            return match serde_json::from_str::<Json>(body) {
                Ok(document) => selects(expression, &document),
                Err(_) => Ok(false),
            };
        }
        if let Some(text) = &self.equal_to_json {
            let expected = serde_json::from_str::<Json>(text).map_err(|error| PatternError::Json(text.clone(), error.to_string()))?;
            let extra = if self.ignore_extra_elements {
                ExtraElements::Ignored
            } else {
                ExtraElements::Refused
            };
            return Ok(serde_json::from_str::<Json>(body).is_ok_and(|document| json_equal(&expected, &document, extra)));
        }
        Err(PatternError::Empty)
    }
}

/// Whether `expression` selects at least one node of `document`, read as the WireMock JSON path implementation
/// reads it: a filter selector applied to an object tests the object itself, where RFC 9535 tests the object's
/// member values. Every other selector reads as RFC 9535 defines it.
fn selects(expression: &str, document: &Json) -> Result<bool, PatternError> {
    let parse = |text: &str| JsonPath::parse(text).map_err(|error| PatternError::JsonPath(expression.to_owned(), error.to_string()));
    let Some((start, end)) = first_filter(expression) else {
        return Ok(!parse(expression)?.query(document).all().is_empty());
    };
    let filter = parse(&format!("${}", &expression[start..end]))?;
    let rest = format!("${}", &expression[end..]);
    for node in parse(&expression[..start])?.query(document).all() {
        let wrapped;
        let subject = match node {
            // The filter's RFC 9535 reading tests array items, so the object is tested as the one item of an array
            // it is copied into.
            Json::Object(_) => {
                wrapped = Json::Array(vec![node.clone()]);
                &wrapped
            }
            Json::Array(_) => node,
            Json::Null | Json::Bool(_) | Json::Number(_) | Json::String(_) => continue,
        };
        for item in filter.query(subject).all() {
            if selects(&rest, item)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// The byte range of the first filter selector (`[?...]`) of `expression` outside its string literals.
fn first_filter(expression: &str) -> Option<(usize, usize)> {
    let bytes = expression.as_bytes();
    let mut quote = None;
    let mut escaped = false;
    let mut start = None;
    let mut depth = 0_usize;
    for (index, &byte) in bytes.iter().enumerate() {
        if let Some(open) = quote {
            match byte {
                _ if escaped => escaped = false,
                b'\\' => escaped = true,
                _ if byte == open => quote = None,
                _ => {}
            }
            continue;
        }
        match byte {
            b'\'' | b'"' => quote = Some(byte),
            b'[' if start.is_none() && bytes.get(index + 1) == Some(&b'?') => {
                start = Some(index);
                depth = 1;
            }
            b'[' if start.is_some() => depth += 1,
            b']' if start.is_some() => {
                depth -= 1;
                if depth == 0 {
                    return start.map(|start| (start, index + 1));
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether a JSON body may hold more than the expected JSON names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtraElements {
    /// Members and array items past the expected ones fail the match.
    Refused,
    /// Members the expected JSON does not name, and array items past its last, are ignored.
    Ignored,
}

/// Whether `actual` equals `expected`, numbers compared by value, as WireMock's `equalToJson` compares.
fn json_equal(expected: &Json, actual: &Json, extra: ExtraElements) -> bool {
    match (expected, actual) {
        (Json::Object(expected), Json::Object(actual)) => {
            (extra == ExtraElements::Ignored || expected.len() == actual.len())
                && expected
                    .iter()
                    .all(|(name, value)| actual.get(name).is_some_and(|member| json_equal(value, member, extra)))
        }
        (Json::Array(expected), Json::Array(actual)) => {
            (if extra == ExtraElements::Ignored {
                expected.len() <= actual.len()
            } else {
                expected.len() == actual.len()
            }) && expected.iter().zip(actual).all(|(item, member)| json_equal(item, member, extra))
        }
        (Json::Number(expected), Json::Number(actual)) => expected.as_f64() == actual.as_f64(),
        (expected, actual) => expected == actual,
    }
}

/// The regular expression `pattern`, anchored to match the whole subject.
fn whole(pattern: &str) -> Result<Regex, PatternError> {
    Regex::new(&format!("^(?:{pattern})$")).map_err(|error| PatternError::Regex(pattern.to_owned(), error.to_string()))
}

#[cfg(test)]
mod tests {
    use crate::sequence::stub_server::pattern::BodyPattern;
    use googletest::prelude::*;

    fn equal_to_json(json: &str) -> serde_json::Result<BodyPattern> {
        serde_json::from_str(json)
    }

    #[gtest]
    fn a_filter_on_a_member_object_tests_the_object_itself() -> Result<()> {
        let pattern = equal_to_json(r#"{"matchesJsonPath": "$.label[?(@.type == 'Property')].value"}"#)?;
        expect_that!(pattern.matches(r#"{"label": {"type": "Property", "value": {"en": "car"}}}"#)?, eq(true));
        verify_that!(pattern.matches(r#"{"label": {"type": "Relationship", "object": "urn:a"}}"#)?, eq(false))
    }

    #[gtest]
    fn a_filter_at_the_root_tests_an_object_body_and_the_items_of_an_array_body() -> Result<()> {
        let pattern = equal_to_json(r#"{"matchesJsonPath": "$[?(@.id == 'urn:a')]"}"#)?;
        expect_that!(pattern.matches(r#"{"id": "urn:a"}"#)?, eq(true));
        expect_that!(pattern.matches(r#"[{"id": "urn:b"}, {"id": "urn:a"}]"#)?, eq(true));
        verify_that!(pattern.matches(r#"{"entity": {"id": "urn:a"}}"#)?, eq(false))
    }

    #[gtest]
    fn a_filter_after_an_index_tests_the_indexed_object() -> Result<()> {
        let pattern = equal_to_json(r#"{"matchesJsonPath": "$.entities[0][?(@.type == 'T' && @.id == 'urn:a')]"}"#)?;
        verify_that!(pattern.matches(r#"{"entities": [{"type": "T", "id": "urn:a"}]}"#)?, eq(true))
    }

    #[gtest]
    fn equal_to_json_ignoring_extra_elements_matches_a_body_holding_more_members() -> Result<()> {
        let pattern = equal_to_json(r#"{"equalToJson": "{\"expiresAt\": \"urn:ngsi-ld:null\"}", "ignoreExtraElements": true}"#)?;
        verify_that!(pattern.matches(r#"{"expiresAt": "urn:ngsi-ld:null", "q": "speed>1"}"#)?, eq(true))
    }

    #[gtest]
    fn equal_to_json_refuses_a_body_holding_more_members_unless_told_to_ignore_them() -> Result<()> {
        let pattern = equal_to_json(r#"{"equalToJson": "{\"expiresAt\": \"urn:ngsi-ld:null\"}"}"#)?;
        verify_that!(pattern.matches(r#"{"expiresAt": "urn:ngsi-ld:null", "q": "speed>1"}"#)?, eq(false))
    }

    #[gtest]
    fn equal_to_json_compares_numbers_by_value_and_refuses_another_value() -> Result<()> {
        let pattern = equal_to_json(r#"{"equalToJson": "{\"speed\": [1, 2]}", "ignoreExtraElements": true}"#)?;
        expect_that!(pattern.matches(r#"{"speed": [1.0, 2, 3]}"#)?, eq(true));
        verify_that!(pattern.matches(r#"{"speed": [2, 1]}"#)?, eq(false))
    }
}
