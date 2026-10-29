//! A stub: the requests it answers, how it answers them, and the scenario state it requires and sets, in the JSON
//! shape of WireMock stub mappings.

use crate::sequence::stub_server::{
    pattern::{PatternError, RequestPattern},
    response_template::{TemplateError, render},
};
use serde::Deserialize;
use serde_json::{Map, Value as Json};
use std::{collections::BTreeMap, slice, time::Duration};
use wiremock::ResponseTemplate;

/// The scenario state every scenario starts in.
pub const STARTED: &str = "Started";

/// A stub mapping.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StubMapping {
    /// The requests the stub answers.
    pub request: RequestPattern,
    /// How it answers.
    pub response: StubResponse,
    /// The order among stubs matching one request: the lowest wins, and an unset priority counts as 5.
    pub priority: Option<i64>,
    /// The values a removal by metadata selects the stub by.
    #[serde(default)]
    pub metadata: Map<String, Json>,
    /// A label.
    pub name: Option<String>,
    /// The scenario the stub belongs to.
    pub scenario_name: Option<String>,
    /// The state the scenario must be in for the stub to answer.
    pub required_scenario_state: Option<String>,
    /// The state the scenario moves to when the stub answers.
    pub new_scenario_state: Option<String>,
}

/// How a stub answers.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StubResponse {
    /// The status code.
    status: u16,
    /// The header fields; a list sends the header once per value.
    #[serde(default)]
    headers: BTreeMap<String, HeaderValues>,
    /// A JSON body.
    json_body: Option<Json>,
    /// A text body.
    body: Option<String>,
    /// How long to wait before answering, in milliseconds.
    fixed_delay_milliseconds: Option<u64>,
    /// The transformers applied; a mapping naming any other is not a stub mapping.
    #[serde(default)]
    transformers: Vec<Transformer>,
}

/// The value or values of one stub response header.
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum HeaderValues {
    /// One value.
    One(String),
    /// Several values, each sent as a field of its own.
    Several(Vec<String>),
}

impl HeaderValues {
    /// Every value.
    fn values(&self) -> &[String] {
        match self {
            HeaderValues::One(value) => slice::from_ref(value),
            HeaderValues::Several(values) => values,
        }
    }
}

/// A transformer a stub response applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Transformer {
    /// Renders the response's templates from the request.
    ResponseTemplate,
}

/// Why a stub cannot answer.
#[derive(Debug, thiserror::Error)]
pub enum StubError {
    /// The request pattern is not valid.
    #[error(transparent)]
    Pattern(#[from] PatternError),
    /// The response template is not one the server renders.
    #[error(transparent)]
    Template(#[from] TemplateError),
    /// More than one transformer is named, where the server applies only `response-template`.
    #[error("{0} transformers are named, and the server applies only response-template")]
    Transformers(usize),
}

impl StubMapping {
    /// The priority, an unset one counting as 5.
    #[must_use]
    pub fn rank(&self) -> i64 {
        self.priority.unwrap_or(5)
    }

    /// Checks that every response template of the stub renders, so an unknown one fails when the test sets it up.
    ///
    /// # Errors
    ///
    /// Returns [`StubError`] when a transformer or template is unknown.
    pub fn validate(&self) -> Result<(), StubError> {
        self.response.answer().map(|_| ())
    }
}

impl StubResponse {
    /// The answer the stub gives.
    ///
    /// # Errors
    ///
    /// Returns [`StubError`] when a transformer or template is unknown.
    pub fn answer(&self) -> Result<ResponseTemplate, StubError> {
        let templated = match self.transformers.as_slice() {
            [] => false,
            [Transformer::ResponseTemplate] => true,
            several @ [_, _, ..] => return Err(StubError::Transformers(several.len())),
        };
        let fill = |text: &str| if templated { render(text) } else { Ok(text.to_owned()) };
        let mut answer = ResponseTemplate::new(self.status);
        for (name, values) in &self.headers {
            for value in values.values() {
                answer = answer.append_header(name.as_str(), fill(value)?.as_str());
            }
        }
        if let Some(json) = &self.json_body {
            answer = answer.set_body_bytes(fill(&json.to_string())?.into_bytes());
        }
        if let Some(body) = &self.body {
            answer = answer.set_body_bytes(fill(body)?.into_bytes());
        }
        if let Some(delay) = self.fixed_delay_milliseconds {
            answer = answer.set_delay(Duration::from_millis(delay));
        }
        Ok(answer)
    }
}
