//! The Entities the Core, Temporal and subscription scenarios read, and how an
//! answer names them.
//!
//! One Entity per spelling of the Slovenian `čebela` (bee): four case forms a
//! case-insensitive pattern must reach, the same word without its caron, which
//! case folding must not reach, and an unrelated control name.

use andromeda_test_client::response::TestResponse;
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use std::collections::BTreeSet;

/// Every spelling stored, one per Entity, in the order their identifiers are
/// numbered.
pub(crate) const SPELLINGS: [&str; 6] = ["čebela", "Čebela", "ČEBELA", "ČEbela", "cebela", "Škofja Loka"];

/// The four case forms of `čebela`, which `(?i)^čebela$` selects.
pub(crate) const CASE_FORMS: [&str; 4] = ["čebela", "Čebela", "ČEBELA", "ČEbela"];

/// When every stored `name` was observed.
pub(crate) const OBSERVED_AT: &str = "2024-05-01T08:00:00Z";

/// The start of a temporal window holding [`OBSERVED_AT`].
pub(crate) const WINDOW_OPENS: &str = "2024-01-01T00:00:00Z";

/// The Entities of one scenario: `<prefix>:Bee:<n>` holding `SPELLINGS[n]`.
pub(crate) struct Spellings {
    /// The Entity type unique to the scenario.
    pub(crate) entity_type: String,
    /// Each spelling beside the identifier of the Entity holding it.
    entities: Vec<(&'static str, String)>,
}

impl Spellings {
    /// Create one Entity per spelling, of a type unique to this scenario.
    pub(crate) async fn create(scope: &TestEnvironment, base_type: &str) -> Spellings {
        let entity_type = scope.entity_type(base_type);
        let mut entities = Vec::new();
        for (index, spelling) in SPELLINGS.into_iter().enumerate() {
            let id = scope.id(&format!("Bee:{index}"));
            let entity = json!({
                "id": id,
                "type": entity_type,
                "name": {"type": "Property", "value": spelling, "observedAt": OBSERVED_AT},
                "@context": [TEST_SUITE_CONTEXT]
            });
            let created = scope.client().post_entity(&entity).send().await.expect("create entity request");
            assert_eq!(created.status, StatusCode::CREATED, "create {spelling}: {:?}", created.body);
            entities.push((spelling, id));
        }
        Spellings { entity_type, entities }
    }

    /// The identifiers of the Entities holding `spellings`.
    pub(crate) fn ids_of(&self, spellings: &[&str]) -> BTreeSet<String> {
        self.entities
            .iter()
            .filter(|(spelling, _)| spellings.contains(spelling))
            .map(|(_, id)| id.clone()) // clone: the fixture keeps its identifiers for the next assertion.
            .collect()
    }
}

/// A `q` filter over one attribute, percent-encoded for the query string.
pub(crate) fn encoded_filter(filter: &str) -> String {
    urlencoding::encode(filter).into_owned()
}

/// The identifiers a query answered with, which must be a `200` array.
pub(crate) fn identifiers(response: &TestResponse) -> BTreeSet<String> {
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    response
        .json_value()
        .expect("a query answers with JSON")
        .as_array()
        .expect("a query answers with an array")
        .iter()
        .map(|item| item["id"].as_str().expect("every item carries an id").to_owned())
        .collect()
}

/// The identifiers of the Entities one notification carries.
pub(crate) fn notified_identifiers(notification: &Value) -> impl Iterator<Item = String> + '_ {
    notification["data"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|entity| entity["id"].as_str().expect("a notified Entity carries an id").to_owned())
}
