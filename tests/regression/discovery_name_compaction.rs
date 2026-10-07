//! The names and identifiers a discovery answer renders (clauses 5.7.5 to
//! 5.7.10).
//!
//! Two rules meet on every one of these resources.
//!
//! Table 5.2.28-1 defines `attributeName` as the "Name of the attribute, short
//! name if contained in @context", and Tables 5.2.25-1 and 5.2.26-1 word
//! `typeName` the same way. Membership of the `@context` is the whole
//! condition. It holds on every surface, `GET /attributes?details=true`, the
//! `attributeList` of clause 5.2.27 and the `relationshipNames` of a Context
//! Source Registration alike, whatever the shape of the term's declaration:
//! neither table mentions a `"@type": "@id"` coercion.
//!
//! The identifiers beside those names take the opposite rule. Table 5.2.28-1
//! restricts the Attribute `id` to the "Full URI of attribute name" and Tables
//! 5.2.25-1 and 5.2.26-1 restrict the entity type `id` to the "Fully Qualified
//! Name (FQN) of the entity type being described". The core `location`
//! Attribute is therefore rendered as its full URI, not as the compact IRI
//! `ngsi-ld:location`, which resolves only against a `@context` declaring the
//! `ngsi-ld` prefix.
//!
//! Every test therefore asserts both halves of one answer: the name short and
//! the identifier full, in the same response.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use crate::hosted_context::{context_link, forget_cached, serve, start_origin};
use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_harness::environment::TestEnvironment;
use axum::http::HeaderMap;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;

/// The full URI of the core `location` Attribute, which Table 5.2.28-1 makes
/// the `id` of every Attribute resource describing it.
const LOCATION_IRI: &str = "https://uri.etsi.org/ngsi-ld/location";

/// The vocabulary one run of this module declares, and the names it declares
/// them under.
///
/// Both halves are unique per run: the suite shares a tenant, so a concurrent
/// run must not be able to contribute a name to the answers asserted here.
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

    /// The `@context` document declaring the run's terms.
    ///
    /// `isParked` carries the `"@type": "@id"` definition of annex C clause
    /// C.2.4's `commonTerms` vocabulary, which is the declaration shape the
    /// `attributeList` and `relationshipNames` surfaces must treat like any other.
    /// `status` is deliberately absent, as clauses C.5.10 and C.5.11 leave one
    /// Attribute undeclared so the answer shows both renderings at once.
    fn declaring_document(&self) -> String {
        json!({"@context": {
            self.term("Vehicle"): self.iri("Vehicle"),
            self.term("speed"): self.iri("speed"),
            self.term("isParked"): {"@type": "@id", "@id": self.iri("isParked")}
        }})
        .to_string()
    }

    /// A `@context` declaring nothing this run uses, which is clause C.7's
    /// "an @context with no matching terms" case.
    fn silent_document(&self) -> String {
        json!({"@context": {format!("unrelated{}", self.token): format!("http://example.org/unrelated/{}", self.token)}}).to_string()
    }

    /// One Vehicle carrying a declared Property, a declared Relationship whose
    /// term coerces an identifier, the core `location` `GeoProperty`, and one
    /// Attribute written as a Fully Qualified Name that no `@context` declares.
    fn vehicle(&self, id: &str) -> Value {
        json!({
            "id": id,
            "type": self.iri("Vehicle"),
            self.iri("speed"): {"type": "Property", "value": 55},
            self.iri("isParked"): {"type": "Relationship", "object": "urn:ngsi-ld:OffStreetParking:Downtown1"},
            self.iri("status"): {"type": "Property", "value": "free"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}}
        })
    }
}

/// Send one discovery GET the typed client does not model, under `context`.
async fn discover(client: &TestClient, path: &str, context: &str) -> Value {
    let response = client
        .raw(Method::GET, path)
        .header("accept", "application/json")
        .header("link", context_link(context).as_str())
        .send()
        .await
        .expect("discovery request");
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a discovery answer is JSON")
}

/// The body of a list answer the typed client models.
fn list_body(response: &TestResponse, path: &str) -> Value {
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a discovery answer is JSON")
}

/// The members of a named string array.
fn strings(answer: &Value, member: &str) -> Vec<String> {
    let items = answer[member].as_array();
    assert!(items.is_some(), "{member} is an array in {answer}");
    items.into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
}

/// The one record of a collection whose `id` is `identifier`.
fn record_with_id(answer: &Value, identifier: &str) -> Value {
    assert!(answer.is_array(), "the answer is a collection: {answer}");
    let record = answer.as_array().into_iter().flatten().find(|record| record["id"] == json!(identifier));
    assert!(record.is_some(), "no record for {identifier} in {answer}");
    record.expect("the assertion above holds").clone() // clone: the caller owns the record it asserts on
}

/// Create the run's Vehicle, and answer nothing: every assertion reads it back
/// through a discovery endpoint.
async fn seed(scope: &TestEnvironment, vocabulary: &RunVocabulary) {
    let entity = vocabulary.vehicle(&scope.id("Vehicle:discovery-names"));
    let created = scope
        .client()
        .post_entity(&entity)
        .with_json("https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld")
        .send()
        .await
        .expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "seeding the discovery fixture: {:?}", created.body);
}

/// Clause 5.2.27's `attributeList` renders a term declared with an identifier
/// coercion as its short name, exactly as every other surface does, and leaves
/// the one name no `@context` declares fully qualified.
#[tokio::test]
async fn regression_discovery_name_compaction_the_attribute_list_uses_an_identifier_coerced_term() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, declaring) = serve(&origin, "wave-j-declaring", &vocabulary.declaring_document(), HeaderMap::new()).await;
    seed(&scope, &vocabulary).await;

    let answer = list_body(
        &scope.client().list_attributes("", Some(&declaring)).await.expect("list attributes"),
        "/attributes",
    );
    let names = strings(&answer, "attributeList");

    assert_eq!(answer["type"], json!("AttributeList"), "{answer}");
    assert!(names.contains(&vocabulary.term("isParked")), "the @type: @id term compacts: {names:?}");
    assert!(names.contains(&vocabulary.term("speed")), "the plain term compacts: {names:?}");
    assert!(names.contains(&"location".to_owned()), "the core term compacts: {names:?}");
    assert!(names.contains(&vocabulary.iri("status")), "an undeclared name stays fully qualified: {names:?}");

    forget_cached(scope.database_pool(), &[declaring]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.14: `GET /attributes?details=true` renders the short name and,
/// beside it, the "Full URI of attribute name" Table 5.2.28-1 fixes the `id`
/// to — never the compact IRI for the core `location`.
#[tokio::test]
async fn regression_discovery_name_compaction_the_attribute_details_pair_a_short_name_with_a_full_uri() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, declaring) = serve(&origin, "wave-j-declaring", &vocabulary.declaring_document(), HeaderMap::new()).await;
    seed(&scope, &vocabulary).await;

    let answer = list_body(
        &scope
            .client()
            .list_attributes("details=true", Some(&declaring))
            .await
            .expect("list attribute details"),
        "/attributes?details=true",
    );

    let parked = record_with_id(&answer, &vocabulary.iri("isParked"));
    assert_eq!(parked["attributeName"], json!(vocabulary.term("isParked")), "{parked}");
    assert_eq!(parked["type"], json!("Attribute"), "{parked}");
    assert_eq!(
        strings(&parked, "typeNames"),
        vec![vocabulary.term("Vehicle")],
        "an entity type name is compacted the same way: {parked}"
    );

    let location = record_with_id(&answer, LOCATION_IRI);
    assert_eq!(location["attributeName"], json!("location"), "{location}");
    assert_eq!(location["id"], json!(LOCATION_IRI), "Table 5.2.28-1 asks for the full URI: {location}");

    let status = record_with_id(&answer, &vocabulary.iri("status"));
    assert_eq!(
        status["attributeName"],
        json!(vocabulary.iri("status")),
        "an undeclared name stays fully qualified: {status}"
    );

    forget_cached(scope.database_pool(), &[declaring]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.15: the single-attribute resource pairs the same short name with
/// the same full URI, for a declared term, for the core term whose IRI the
/// `ngsi-ld` prefix would shorten, and for the name no `@context` declares.
#[tokio::test]
async fn regression_discovery_name_compaction_the_attribute_information_pairs_a_short_name_with_a_full_uri() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, declaring) = serve(&origin, "wave-j-declaring", &vocabulary.declaring_document(), HeaderMap::new()).await;
    seed(&scope, &vocabulary).await;

    let parked = discover(scope.client(), &format!("/ngsi-ld/v1/attributes/{}", vocabulary.term("isParked")), &declaring).await;
    assert_eq!(parked["id"], json!(vocabulary.iri("isParked")), "{parked}");
    assert_eq!(parked["attributeName"], json!(vocabulary.term("isParked")), "{parked}");
    assert_eq!(strings(&parked, "attributeTypes"), vec!["Relationship".to_owned()], "{parked}");
    assert_eq!(parked["attributeCount"], json!(1), "{parked}");

    let location = discover(scope.client(), "/ngsi-ld/v1/attributes/location", &declaring).await;
    assert_eq!(location["id"], json!(LOCATION_IRI), "Table 5.2.28-1 asks for the full URI: {location}");
    assert_eq!(location["attributeName"], json!("location"), "{location}");

    let encoded = urlencoding::encode(&vocabulary.iri("status")).into_owned();
    let status = discover(scope.client(), &format!("/ngsi-ld/v1/attributes/{encoded}"), &declaring).await;
    assert_eq!(status["id"], json!(vocabulary.iri("status")), "{status}");
    assert_eq!(status["attributeName"], json!(vocabulary.iri("status")), "{status}");

    forget_cached(scope.database_pool(), &[declaring]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.5.10 and 4.5.11: `typeList` and the detailed list render the
/// declared short names, and the detailed list's `id` stays the FQN Table
/// 5.2.25-1 asks for while its `attributeNames` compact.
#[tokio::test]
async fn regression_discovery_name_compaction_the_entity_type_lists_use_the_declared_terms() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, declaring) = serve(&origin, "wave-j-declaring", &vocabulary.declaring_document(), HeaderMap::new()).await;
    seed(&scope, &vocabulary).await;

    let list = list_body(&scope.client().list_types("", Some(&declaring)).await.expect("list types"), "/types");
    assert_eq!(list["type"], json!("EntityTypeList"), "{list}");
    assert!(
        strings(&list, "typeList").contains(&vocabulary.term("Vehicle")),
        "the declared type compacts: {list}"
    );

    let details = list_body(
        &scope.client().list_types("details=true", Some(&declaring)).await.expect("list type details"),
        "/types?details=true",
    );
    let vehicle = record_with_id(&details, &vocabulary.iri("Vehicle"));
    assert_eq!(vehicle["typeName"], json!(vocabulary.term("Vehicle")), "{vehicle}");
    assert_eq!(vehicle["id"], json!(vocabulary.iri("Vehicle")), "Table 5.2.25-1 asks for the FQN: {vehicle}");
    let attribute_names = strings(&vehicle, "attributeNames");
    assert!(attribute_names.contains(&vocabulary.term("isParked")), "{attribute_names:?}");
    assert!(attribute_names.contains(&vocabulary.term("speed")), "{attribute_names:?}");
    assert!(attribute_names.contains(&"location".to_owned()), "{attribute_names:?}");
    assert!(attribute_names.contains(&vocabulary.iri("status")), "{attribute_names:?}");

    forget_cached(scope.database_pool(), &[declaring]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.12: the entity type information resource carries the short type
/// name beside the FQN identifier, and each of its `attributeDetails` carries
/// the short attribute name beside the full attribute URI.
#[tokio::test]
async fn regression_discovery_name_compaction_the_entity_type_information_pairs_short_names_with_full_uris() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, declaring) = serve(&origin, "wave-j-declaring", &vocabulary.declaring_document(), HeaderMap::new()).await;
    seed(&scope, &vocabulary).await;

    let answer = discover(scope.client(), &format!("/ngsi-ld/v1/types/{}", vocabulary.term("Vehicle")), &declaring).await;
    assert_eq!(answer["type"], json!("EntityTypeInfo"), "{answer}");
    assert_eq!(answer["id"], json!(vocabulary.iri("Vehicle")), "Table 5.2.26-1 asks for the FQN: {answer}");
    assert_eq!(answer["typeName"], json!(vocabulary.term("Vehicle")), "{answer}");
    assert_eq!(answer["entityCount"], json!(1), "{answer}");

    let details = answer["attributeDetails"].clone();
    let parked = record_with_id(&details, &vocabulary.iri("isParked"));
    assert_eq!(parked["attributeName"], json!(vocabulary.term("isParked")), "{parked}");
    assert_eq!(strings(&parked, "attributeTypes"), vec!["Relationship".to_owned()], "{parked}");

    let location = record_with_id(&details, LOCATION_IRI);
    assert_eq!(location["attributeName"], json!("location"), "{location}");
    assert_eq!(location["id"], json!(LOCATION_IRI), "Table 5.2.28-1 asks for the full URI: {location}");

    let status = record_with_id(&details, &vocabulary.iri("status"));
    assert_eq!(status["attributeName"], json!(vocabulary.iri("status")), "{status}");

    forget_cached(scope.database_pool(), &[declaring]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Clause C.7: under an `@context` with no matching terms, every user name is
/// rendered as a Fully Qualified Name while the core terms stay compacted, and
/// the identifiers are unchanged by the choice of `@context`.
#[tokio::test]
async fn regression_discovery_name_compaction_a_silent_context_renders_every_user_name_fully_qualified() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, silent) = serve(&origin, "wave-j-silent", &vocabulary.silent_document(), HeaderMap::new()).await;
    seed(&scope, &vocabulary).await;

    let types = list_body(&scope.client().list_types("", Some(&silent)).await.expect("list types"), "/types");
    assert!(strings(&types, "typeList").contains(&vocabulary.iri("Vehicle")), "{types}");

    let attributes = list_body(
        &scope.client().list_attributes("", Some(&silent)).await.expect("list attributes"),
        "/attributes",
    );
    let names = strings(&attributes, "attributeList");
    assert!(names.contains(&vocabulary.iri("isParked")), "{names:?}");
    assert!(names.contains(&vocabulary.iri("speed")), "{names:?}");
    assert!(
        names.contains(&"location".to_owned()),
        "the core @context is always implicit, so its terms still compact: {names:?}"
    );

    let details = list_body(
        &scope
            .client()
            .list_attributes("details=true", Some(&silent))
            .await
            .expect("list attribute details"),
        "/attributes?details=true",
    );
    let parked = record_with_id(&details, &vocabulary.iri("isParked"));
    assert_eq!(parked["attributeName"], json!(vocabulary.iri("isParked")), "{parked}");
    assert_eq!(parked["id"], json!(vocabulary.iri("isParked")), "{parked}");
    assert_eq!(strings(&parked, "typeNames"), vec![vocabulary.iri("Vehicle")], "{parked}");

    let location = record_with_id(&details, LOCATION_IRI);
    assert_eq!(
        location["id"],
        json!(LOCATION_IRI),
        "the identifier does not depend on the @context: {location}"
    );
    assert_eq!(location["attributeName"], json!("location"), "{location}");

    forget_cached(scope.database_pool(), &[silent]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}

/// Table 5.2.9-1's `relationshipNames`, which annex C clause C.3 prints as
/// `["isParked"]`, renders the declared term rather than the Fully Qualified
/// Name, whatever its `"@type": "@id"` definition says.
#[tokio::test]
async fn regression_discovery_name_compaction_a_registration_renders_its_relationship_names_short() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let vocabulary = RunVocabulary::allocate();
    let origin = start_origin().await;
    let (_, declaring) = serve(&origin, "wave-j-declaring", &vocabulary.declaring_document(), HeaderMap::new()).await;

    let registration_id = scope.id("ContextSourceRegistration:relationship-names");
    let registration = json!({
        "id": registration_id,
        "type": "ContextSourceRegistration",
        "information": [{
            "entities": [{"type": vocabulary.term("Vehicle")}],
            "propertyNames": [vocabulary.term("speed")],
            "relationshipNames": [vocabulary.term("isParked")]
        }],
        "endpoint": "http://example.org/wave-j/source",
        "@context": [declaring.as_str(), "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld"]
    });
    let created = scope.client().create_registration(&registration).await.expect("create registration");
    assert_eq!(created.status, StatusCode::CREATED, "creating the registration: {:?}", created.body);

    let retrieved = scope
        .client()
        .retrieve_registration(&registration_id, Some(&declaring))
        .await
        .expect("retrieve registration");
    let outcome = match retrieved.status {
        StatusCode::OK => retrieved.json_value().expect("a registration is JSON"),
        other => {
            scope.client().delete_registration(&registration_id).await.expect("delete registration");
            panic!("retrieving the registration answered {other}: {:?}", retrieved.body);
        }
    };

    let information = &outcome["information"][0];
    assert_eq!(
        strings(information, "relationshipNames"),
        vec![vocabulary.term("isParked")],
        "clause C.3 prints the short name: {outcome}"
    );
    assert_eq!(strings(information, "propertyNames"), vec![vocabulary.term("speed")], "{outcome}");
    assert_eq!(information["entities"][0]["type"], json!(vocabulary.term("Vehicle")), "{outcome}");

    scope.client().delete_registration(&registration_id).await.expect("delete registration");
    forget_cached(scope.database_pool(), &[declaring]).await;
    origin.shutdown().await.expect("context server shuts down");
    scope.finish().await.expect("cleanup failed");
}
