//! A `@context` term that coerces a datatype, across the clause 4.5.1
//! representations.
//!
//! ETSI GS CIM 009 v1.9.1 clause C.6 gives three ways to carry a Date, Time or
//! `DateTime` on a reified Property, the third being JSON-LD type coercion:
//!
//! ```text
//! "testedAt": {
//!   "@type": "https://uri.etsi.org/ngsi-ld/DateTime",
//!   "@id": "http://example.org/test/testedAt"
//! }
//! ```
//!
//! and then states exactly where it does and does not work. "The above does not
//! work, when using the @context to perform compaction, in the normalized and
//! compact representation of NGSI-LD, due to reification of the Property,
//! because in this case testedAt is a complex JSON object, which cannot be
//! compacted to a `DateTime` type as the @context specifies. Thus, the full URI
//! <http://example.org/test/testedAt> is kept, instead of the short name
//! testedAt." And: "However, in the simplified (keyValue) representation case,
//! such an @context with the specification of testedAt could be used, as there
//! is no reification."
//!
//! The broker names the Attribute by its short name in the simplified
//! representation, where no reified object stands in the way, and keeps the full
//! URI in the reified ones. It is the same rule Table 5.2.28-1 states for a
//! discovery answer: the short name is conditioned on membership of the
//! `@context`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::hosted_context::{forget_cached, serve, start_origin};
use andromeda_test_client::test_client::TestClient;
use andromeda_test_harness::environment::TestEnvironment;
use axum::http::HeaderMap;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

/// The instant the Property under test carries, as clause C.6 prints it.
const TESTED_AT: &str = "2018-12-04T12:00:00Z";

/// The vocabulary one run declares, unique per run so a shared tenant cannot
/// let two runs contribute to each other's answers.
struct RunVocabulary {
    /// The run-unique token every name and IRI is built from.
    token: String,
}

impl RunVocabulary {
    /// Allocate a vocabulary for one run.
    fn allocate() -> RunVocabulary {
        RunVocabulary {
            token: Uuid::new_v4().simple().to_string(),
        }
    }

    /// The name a term is declared under.
    fn term(&self, base: &str) -> String {
        format!("{base}{}", self.token)
    }

    /// The IRI a term expands to.
    fn iri(&self, base: &str) -> String {
        format!("http://example.org/wave-j/{}/{base}", self.token)
    }

    /// The coercing `@context` of clause C.6, beside a plain declaration that
    /// serves as the control: the plain term compacts everywhere, so any
    /// difference between the two representations belongs to the coercion.
    fn coercing_document(&self) -> String {
        json!({"@context": {
            self.term("Vehicle"): self.iri("Vehicle"),
            self.term("brandName"): self.iri("brandName"),
            self.term("testedAt"): {"@type": "https://uri.etsi.org/ngsi-ld/DateTime", "@id": self.iri("testedAt")}
        }})
        .to_string()
    }

    /// A `@context` declaring nothing this run uses.
    fn silent_document(&self) -> String {
        json!({"@context": {format!("unrelated{}", self.token): format!("http://example.org/unrelated/{}", self.token)}}).to_string()
    }
}

/// Create the Vehicle clause C.6 describes, written under the coercing
/// `@context` so its Attribute is stored under the coerced term's IRI.
async fn seed(scope: &TestEnvironment, vocabulary: &RunVocabulary, context: &str) -> String {
    let id = scope.id("Vehicle:coerced-term");
    let entity = json!({
        "id": id,
        "type": vocabulary.term("Vehicle"),
        vocabulary.term("testedAt"): {"type": "Property", "value": TESTED_AT},
        vocabulary.term("brandName"): {"type": "Property", "value": "Mercedes"}
    });
    let created = scope
        .client()
        .post_entity(&entity)
        .with_json(context)
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "seeding the coerced-term fixture: {:?}", created.body);
    id
}

/// One retrieval of the seeded Entity in the named representation.
async fn retrieve(client: &TestClient, id: &str, query: &str, context: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/json", Some(context))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieval with {query}: {:?}", response.body);
    response.json_value().expect("an entity is JSON")
}

/// Clause C.6: with no reification in the way, the coerced term names the
/// Attribute in the simplified representation.
#[tokio::test]
async fn regression_coerced_term_representation_the_simplified_representation_uses_the_declared_term() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, coercing) = serve(&origin, "wave-j-coercing", &vocabulary.coercing_document(), HeaderMap::new()).await;
    let id = seed(&scope, &vocabulary, &coercing).await;

    for query in ["format=simplified", "format=keyValues", "options=keyValues"] {
        let answer = retrieve(scope.client(), &id, query, &coercing).await;
        assert_eq!(answer[vocabulary.term("testedAt")], json!(TESTED_AT), "clause C.6 with {query}: {answer}");
        assert_eq!(
            answer[vocabulary.iri("testedAt")],
            Value::Null,
            "the full URI is not carried beside the term: {answer}"
        );
        assert_eq!(
            answer[vocabulary.term("brandName")],
            json!("Mercedes"),
            "the plain term is unaffected: {answer}"
        );
    }

    forget_cached(scope.database_pool(), &[coercing]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause C.6: the reified representations keep the full URI.
#[tokio::test]
async fn regression_coerced_term_representation_the_reified_representations_keep_the_full_uri() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, coercing) = serve(&origin, "wave-j-coercing", &vocabulary.coercing_document(), HeaderMap::new()).await;
    let id = seed(&scope, &vocabulary, &coercing).await;

    // The two representations spell the reified Property differently -- clause
    // 4.5.3's concise form drops the `type` and `value` members of a Property
    // that carries nothing else -- so only the name is asserted across both,
    // which is what clause C.6 is about.
    for query in ["", "format=normalized", "format=concise"] {
        let answer = retrieve(scope.client(), &id, query, &coercing).await;
        assert!(
            answer.get(vocabulary.iri("testedAt")).is_some(),
            "clause C.6 keeps the full URI with '{query}': {answer}"
        );
        assert_eq!(
            answer[vocabulary.term("testedAt")],
            Value::Null,
            "the short name is not used where the Property is reified: {answer}"
        );
        assert!(
            answer.get(vocabulary.term("brandName")).is_some(),
            "the plain term compacts even where the coerced one cannot: {answer}"
        );
    }

    // The normalized spelling of that same Attribute, so the test says what the
    // Property carries and not only what it is called.
    let normalized = retrieve(scope.client(), &id, "format=normalized", &coercing).await;
    assert_eq!(
        normalized[vocabulary.iri("testedAt")],
        json!({"type": "Property", "value": TESTED_AT}),
        "{normalized}"
    );

    forget_cached(scope.database_pool(), &[coercing]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// The rule belongs to the representation rather than to the endpoint, so a
/// query answers the same names a retrieval does.
#[tokio::test]
async fn regression_coerced_term_representation_a_query_answers_the_same_names_as_a_retrieval() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, coercing) = serve(&origin, "wave-j-coercing", &vocabulary.coercing_document(), HeaderMap::new()).await;
    let id = seed(&scope, &vocabulary, &coercing).await;

    let response = scope
        .client()
        .query_entities(
            &format!("type={}&id={id}&format=simplified", vocabulary.term("Vehicle")),
            "application/json",
            Some(&coercing),
        )
        .await
        .expect("query entities request");
    assert_eq!(response.status, StatusCode::OK, "{:?}", response.body);
    let answer = response.json_value().expect("a query answers JSON");
    assert!(answer.is_array(), "a query answers a collection: {answer}");
    let matches = answer.as_array().expect("the assertion above holds");
    assert_eq!(matches.len(), 1, "the query answers the one Entity it selected: {answer}");
    assert_eq!(matches[0][vocabulary.term("testedAt")], json!(TESTED_AT), "{answer}");

    let normalized = scope
        .client()
        .query_entities(&format!("type={}&id={id}", vocabulary.term("Vehicle")), "application/json", Some(&coercing))
        .await
        .expect("query entities request");
    assert_eq!(normalized.status, StatusCode::OK, "{:?}", normalized.body);
    let normalized = normalized.json_value().expect("a query answers JSON");
    assert_eq!(normalized[0][vocabulary.iri("testedAt")]["value"], json!(TESTED_AT), "{normalized}");

    forget_cached(scope.database_pool(), &[coercing]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause C.7: an `@context` with no matching terms leaves the name fully
/// qualified in the simplified representation too, so the short name comes from
/// a declaration and never from the representation alone.
#[tokio::test]
async fn regression_coerced_term_representation_a_silent_context_leaves_the_name_fully_qualified() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, coercing) = serve(&origin, "wave-j-coercing", &vocabulary.coercing_document(), HeaderMap::new()).await;
    let (_, silent) = serve(&origin, "wave-j-silent", &vocabulary.silent_document(), HeaderMap::new()).await;
    let id = seed(&scope, &vocabulary, &coercing).await;

    let answer = retrieve(scope.client(), &id, "format=simplified", &silent).await;
    assert_eq!(answer[vocabulary.iri("testedAt")], json!(TESTED_AT), "{answer}");
    assert_eq!(answer[vocabulary.term("testedAt")], Value::Null, "{answer}");
    assert_eq!(answer[vocabulary.iri("brandName")], json!("Mercedes"), "{answer}");

    forget_cached(scope.database_pool(), &[coercing, silent]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}
