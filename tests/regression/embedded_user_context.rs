//! Where an NGSI-LD document may carry a user `@context`.
//!
//! ETSI GS CIM 009 v1.9.1 clause 5.5.7 states that the user `@context` shall
//! not "be embedded into NGSI-LD Attributes, i.e. there cannot be parts of the
//! user @context other than at the top level of the NGSI-LD document", and that
//! parts of a user `@context` breaking that "should result in an error of type
//! `BadRequestData`, because JSON-LD Scoped Contexts and nested embedded
//! @context could be used to modify terms defined in the Core @context or to
//! reshape NGSI-LD Elements during the expansion of terms". Clauses 4.4 and
//! 4.5.1 restate it.
//!
//! The broker accepted every one of them: an `@context` written inside an
//! attribute node, inside a sub-attribute, or inside a Property's value object
//! reached the JSON-LD processor and was applied, so a client could redefine
//! the terms of the very attribute it was writing.
//!
//! The one place a key spelled `@context` is data rather than a context is a
//! `JsonProperty`, which clause 5.5.7 closes on: it "is designed to hold native
//! JSON values which are by definition not available for expansion and
//! compaction via an @context. Therefore, the given short-hand name is always
//! used for accessing JSON keys within a `JsonProperty` json element".
//!
//! The rule holds for every NGSI-LD document a client sends, not only for
//! Entities: a Subscription or a Context Source Registration carrying an
//! `@context` inside `entities` or `information` was accepted. The user `@context` admission now refuses it on every operation, after
//! the context loaded, so an unreachable context still answers
//! `LdContextNotAvailable` first (clause 5.5.4).
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.7 gives a user `@context` written where it does
/// not belong.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// Assert one refusal is the `BadRequestData` clause 5.5.7 asks for, and that
/// its detail names the rule rather than some other admission the payload also
/// happens to break.
fn assert_embedded_context_refused(response: &TestResponse, scenario: &str) {
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
    let detail = problem["detail"].as_str().unwrap_or_default();
    assert!(detail.contains("@context is embedded"), "{scenario}: {problem}");
}

/// One `Device` carrying the single attribute under test, with the Core
/// `@context` at the top level where clause 5.5.7 admits one.
fn device_with(id: &str, attribute: &str, node: &Value) -> Value {
    json!({
        "id": id,
        "type": "Device",
        attribute: node,
        "@context": [CORE_CONTEXT]
    })
}

/// One `Device` carrying nothing but an ordinary Property, used to bring the
/// target of an update, a merge or a replacement into existence.
fn plain_device(id: &str) -> Value {
    device_with(id, "speed", &json!({"type": "Property", "value": 1}))
}

/// Create one entity and require the broker to accept it.
async fn create_device(client: &TestClient, entity: &Value, scenario: &str) {
    let response = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::CREATED, "{scenario}: {:?}", response.body);
}

/// Send one `application/ld+json` body to a path the typed client does not
/// model.
async fn send_ld_json(client: &TestClient, method: Method, path: &str, body: &Value) -> TestResponse {
    client
        .raw(method, path)
        .header("content-type", "application/ld+json")
        .json(body)
        .send()
        .await
        .expect("request")
}

/// The stored entity, in the normalized representation.
async fn stored_entity(client: &TestClient, id: &str) -> Value {
    let response = client.get_entity(id, "", "application/ld+json", None).await.expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieving {id}: {:?}", response.body);
    response.json_value().expect("an entity is JSON")
}

/// The four attribute slots a `@context` could hide in below an attribute
/// node, each of which the JSON-LD processor traverses and would apply.
///
/// The Property `value` object, the `GeoProperty` geometry and the
/// `LanguageProperty` language map are the three the clause 4.5.2.2 seal walk
/// deliberately does not enter -- it stops at a family's reserved value member
/// -- so they are exactly the placements a walk reusing that descent would
/// miss.
fn embedded_placements() -> Vec<(&'static str, &'static str, Value)> {
    vec![
        (
            "the attribute node itself",
            "speed",
            json!({"@context": {"speed": "https://example.org/speed"}, "type": "Property", "value": 1}),
        ),
        (
            "a sub-attribute",
            "speed",
            json!({"type": "Property", "value": 1, "accuracy": {"@context": {"accuracy": "https://example.org/accuracy"}, "type": "Property", "value": 0.5}}),
        ),
        (
            "a Property value object",
            "speed",
            json!({"type": "Property", "value": {"@context": {"reading": "https://example.org/reading"}, "reading": 1}}),
        ),
        (
            "a GeoProperty geometry",
            "location",
            json!({"type": "GeoProperty", "value": {"@context": {}, "type": "Point", "coordinates": [13.4, 52.5]}}),
        ),
        (
            "a LanguageProperty language map",
            "label",
            json!({"type": "LanguageProperty", "languageMap": {"@context": {}, "en": "hello"}}),
        ),
        (
            "a Relationship sub-attribute",
            "belongsTo",
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1", "providedBy": {"@context": {}, "type": "Relationship", "object": "urn:ngsi-ld:Device:1"}}),
        ),
    ]
}

/// Clause 5.5.7: an `@context` written into an attribute node, a sub-attribute
/// or a value object is a part of the user `@context` outside the document's
/// top level, and Create Entity (clause 5.6.1) must refuse it.
#[tokio::test]
async fn regression_embedded_user_context_create_entity_refuses_every_placement() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (label, attribute, node) in embedded_placements() {
        let id = scope.id(&format!("Device:create-{}", label.replace(' ', "-")));
        let entity = device_with(&id, attribute, &node);
        let response = client.post_entity(&entity).with_ld_json().send().await.expect("create entity request");
        assert_embedded_context_refused(&response, label);

        let missing = client.get_entity(&id, "", "application/ld+json", None).await.expect("retrieve entity request");
        assert_eq!(missing.status, StatusCode::NOT_FOUND, "{label}: a refused create stores nothing");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.3.4 (Append Attributes) and clause 5.6.2.4 (Update Attributes)
/// each open with the clause 5.5.4 JSON-LD validation, and clause 5.5.7 is part
/// of what a JSON-LD document must satisfy.
#[tokio::test]
async fn regression_embedded_user_context_entity_attribute_writes_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:attribute-writes");
    create_device(client, &plain_device(&id), "seeding the target").await;

    let fragment = json!({
        "gear": {"type": "Property", "value": 3, "@context": {"gear": "https://example.org/gear"}},
        "@context": [CORE_CONTEXT]
    });
    let appended = client.append_entity_attrs(&id, &fragment).await.expect("append attributes request");
    assert_embedded_context_refused(&appended, "clause 5.6.3 append attributes");

    let updated = client.update_entity_attrs(&id, &fragment).await.expect("update attributes request");
    assert_embedded_context_refused(&updated, "clause 5.6.2 update attributes");

    let stored = stored_entity(client, &id).await;
    assert!(stored.get("gear").is_none(), "a refused write stores nothing: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.4 makes an Attribute Fragment the attribute node itself, so its
/// own top level is where an `application/ld+json` request legitimately carries
/// its `@context` -- and anything under one of its members is embedded. Clause
/// 5.6.19 replaces the same node.
#[tokio::test]
async fn regression_embedded_user_context_attribute_fragment_writes_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:attribute-fragment");
    create_device(client, &plain_device(&id), "seeding the target").await;

    let fragment = json!({
        "value": 2,
        "accuracy": {"type": "Property", "value": 0.5, "@context": {"accuracy": "https://example.org/accuracy"}},
        "@context": [CORE_CONTEXT]
    });
    let partial = client
        .partial_update_attr(&id, "speed", &fragment)
        .await
        .expect("partial update attribute request");
    assert_embedded_context_refused(&partial, "clause 5.6.4 partial update attribute");

    let replacement = json!({
        "type": "Property",
        "value": 3,
        "accuracy": {"type": "Property", "value": 0.5, "@context": {}},
        "@context": [CORE_CONTEXT]
    });
    let replaced = send_ld_json(client, Method::PUT, &format!("/ngsi-ld/v1/entities/{id}/attrs/speed"), &replacement).await;
    assert_embedded_context_refused(&replaced, "clause 5.6.19 replace attribute");

    let stored = stored_entity(client, &id).await;
    assert_eq!(stored["speed"]["value"], json!(1), "a refused write leaves the attribute alone: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.18 (Replace Entity) and clause 5.6.17 (Merge Entity) run the same
/// clause 5.5.4 validation first, and both decode their payload into the entity
/// model before it is screened -- so this covers the model round trip too.
#[tokio::test]
async fn regression_embedded_user_context_replace_and_merge_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:replace-and-merge");
    create_device(client, &plain_device(&id), "seeding the target").await;

    let payload = json!({
        "id": id,
        "type": "Device",
        "speed": {"type": "Property", "value": {"@context": {"reading": "https://example.org/reading"}, "reading": 9}},
        "@context": [CORE_CONTEXT]
    });
    let replaced = client
        .replace_entity(&id, &payload, "application/ld+json", None)
        .await
        .expect("replace entity request");
    assert_embedded_context_refused(&replaced, "clause 5.6.18 replace entity");

    let fragment = json!({
        "speed": {"type": "Property", "value": 4, "@context": {}},
        "@context": [CORE_CONTEXT]
    });
    let merged = send_ld_json(client, Method::PATCH, &format!("/ngsi-ld/v1/entities/{id}"), &fragment).await;
    assert_embedded_context_refused(&merged, "clause 5.6.17 merge entity");

    let stored = stored_entity(client, &id).await;
    assert_eq!(stored["speed"]["value"], json!(1), "a refused write leaves the entity alone: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.7.4 to 5.6.9.4 and 5.6.20.4 run the single-entity behaviour "for
/// each of the NGSI-LD Entities included in the input Array" and add "a new
/// `BatchEntityError` containing the failed Entity ID and the related
/// `ProblemDetails`" for each one that fails: an embedded `@context` is that one
/// entity's failure and leaves the rest of the batch to be applied.
#[tokio::test]
async fn regression_embedded_user_context_batch_refuses_only_the_offending_entity() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    // Clauses 5.6.9.4 and 5.6.20.4 update or merge an Entity that must already
    // be there; clauses 5.6.7.4 and 5.6.8.4 bring it into being themselves, and
    // seeding it first would answer `AlreadyExists` instead.
    for (label, path, targets_must_exist) in [
        ("clause 5.6.7 batch create", "/ngsi-ld/v1/entityOperations/create", false),
        ("clause 5.6.8 batch upsert", "/ngsi-ld/v1/entityOperations/upsert", false),
        ("clause 5.6.9 batch update", "/ngsi-ld/v1/entityOperations/update", true),
        ("clause 5.6.20 batch merge", "/ngsi-ld/v1/entityOperations/merge", true),
    ] {
        let slug = label.replace(' ', "-");
        let admitted = scope.id(&format!("Device:batch-ok-{slug}"));
        let refused = scope.id(&format!("Device:batch-bad-{slug}"));
        if targets_must_exist {
            create_device(client, &plain_device(&admitted), &format!("{label}: seeding the admitted target")).await;
            create_device(client, &plain_device(&refused), &format!("{label}: seeding the refused target")).await;
        }

        let body = json!([
            plain_device(&admitted),
            device_with(&refused, "speed", &json!({"type": "Property", "value": 2, "@context": {}}))
        ]);
        let response = send_ld_json(client, Method::POST, path, &body).await;
        assert_eq!(response.status, StatusCode::MULTI_STATUS, "{label}: {:?}", response.body);

        let result = response.json_value().expect("a batch answers with a clause 5.2.16 result");
        let errors = result["errors"].as_array().expect("the clause 5.2.16 result lists its errors");
        let reported = errors.iter().find(|error| error["entityId"] == json!(refused)).unwrap_or(&Value::Null);
        assert_eq!(reported["error"]["type"], json!(BAD_REQUEST_DATA), "{label}: {result}");
        assert!(
            reported["error"]["detail"].as_str().unwrap_or_default().contains("@context is embedded"),
            "{label}: {result}"
        );
        assert!(
            !errors.iter().any(|error| error["entityId"] == json!(admitted)),
            "{label}: the rest of the batch is applied: {result}"
        );
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 5.6.11.4 and 5.6.12.4 open with the same clause 5.5.4 JSON-LD
/// validation, and clause 5.6.14 modifies one instance of a Temporal
/// Evolution. None of the three may take a payload carrying an embedded
/// `@context`.
#[tokio::test]
async fn regression_embedded_user_context_temporal_writes_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:temporal-embedded");

    let seeded = json!({
        "id": id,
        "type": "Device",
        "speed": [{"type": "Property", "value": 1, "observedAt": "2024-01-01T00:00:00Z"}],
        "@context": [CORE_CONTEXT]
    });
    let created = client.create_temporal_entity(&seeded).await.expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "seeding the evolution: {:?}", created.body);

    let embedded = json!({
        "id": id,
        "type": "Device",
        "speed": [{"type": "Property", "value": 2, "observedAt": "2024-01-02T00:00:00Z", "@context": {"speed": "https://example.org/speed"}}],
        "@context": [CORE_CONTEXT]
    });
    let refused = client.create_temporal_entity(&embedded).await.expect("create temporal entity request");
    assert_embedded_context_refused(&refused, "clause 5.6.11 create or update temporal entity");

    let appended = send_ld_json(
        client,
        Method::POST,
        &format!("/ngsi-ld/v1/temporal/entities/{id}/attrs"),
        &json!({
            "gear": [{"type": "Property", "value": 3, "observedAt": "2024-01-02T00:00:00Z", "@context": {}}],
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    assert_embedded_context_refused(&appended, "clause 5.6.12 add temporal attributes");

    let instance = send_ld_json(
        client,
        Method::PATCH,
        &format!("/ngsi-ld/v1/temporal/entities/{id}/attrs/speed/urn:ngsi-ld:AttributeInstance:probe"),
        &json!({
            "speed": [{
                "type": "Property",
                "value": 5,
                "accuracy": {"type": "Property", "value": 0.5, "@context": {}}
            }],
            "@context": [CORE_CONTEXT]
        }),
    )
    .await;
    assert_embedded_context_refused(&instance, "clause 5.6.14 modify attribute instance");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.7's closing note: a `JsonProperty` "is designed to hold native
/// JSON values which are by definition not available for expansion and
/// compaction via an @context. Therefore, the given short-hand name is always
/// used for accessing JSON keys within a `JsonProperty` json element." A key
/// spelled `@context` inside one is data, and the broker must store and serve it
/// unchanged.
#[tokio::test]
async fn regression_embedded_user_context_a_json_property_keeps_a_context_key_verbatim() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:json-property-context-key");

    let native = json!({
        "@context": "https://example.org/this-is-data-not-a-context",
        "nested": {"@context": {"shipped": "as-is"}, "count": 3},
        "list": [{"@context": []}, 7]
    });
    let entity = json!({
        "id": id,
        "type": "Device",
        "config": {"type": "JsonProperty", "json": native},
        "@context": [CORE_CONTEXT]
    });
    create_device(client, &entity, "a JsonProperty carrying an @context key is admitted").await;

    let stored = stored_entity(client, &id).await;
    assert_eq!(stored["config"]["type"], json!("JsonProperty"), "{stored}");
    assert_eq!(stored["config"]["json"], native, "the native JSON round-trips unchanged: {stored}");

    scope.finish().await.expect("cleanup failed");
}

/// The control: the `@context` clause 5.5.7 does admit is the one at the top
/// level of the document, and every write path must keep taking it.
#[tokio::test]
async fn regression_embedded_user_context_a_top_level_context_is_still_admitted() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:top-level-context");

    let entity = json!({
        "id": id,
        "type": "Device",
        "speed": {"type": "Property", "value": 1, "accuracy": {"type": "Property", "value": 0.5}},
        "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}},
        "label": {"type": "LanguageProperty", "languageMap": {"en": "hello"}},
        "@context": [CORE_CONTEXT, {"speed": "https://example.org/speed"}]
    });
    create_device(client, &entity, "a top-level @context is the document's own").await;

    let appended = client
        .append_entity_attrs(&id, &json!({"gear": {"type": "Property", "value": 3}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("append attributes request");
    assert_eq!(
        appended.status,
        StatusCode::NO_CONTENT,
        "appending under a top-level @context: {:?}",
        appended.body
    );

    let stored = stored_entity(client, &id).await;
    assert_eq!(stored["gear"]["value"], json!(3), "{stored}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.2 defines the `ngsildproof` Property, whose two seals the Core
/// `@context` declares inside a scoped `@context` of its own -- the one scoped
/// context in the specification. Clause 5.5.7 calls the Core `@context`
/// protected and addresses its prohibition to the *user* `@context`, so the
/// core's own scoped term has to keep working: the seals must still expand and
/// come back.
#[tokio::test]
async fn regression_embedded_user_context_the_core_context_scoped_proof_term_still_expands() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:core-scoped-proof");

    let entity = json!({
        "id": id,
        "type": "Device",
        "speed": {
            "type": "Property",
            "value": 1,
            "ngsildproof": {
                "type": "Property",
                "value": {"type": "DataIntegrityProof", "cryptosuite": "eddsa-jcs-2022"},
                "entityIdSealed": id,
                "entityTypeSealed": "Device"
            }
        },
        "@context": [CORE_CONTEXT]
    });
    create_device(client, &entity, "the core context's own scoped term").await;

    let stored = stored_entity(client, &id).await;
    let proof = &stored["speed"]["ngsildproof"];
    // Clause 4.5.2.2 calls the two seals "non-reified subproperties", so the
    // one that comes back is the bare value and not a Property wrapping it.
    assert_eq!(
        proof["entityIdSealed"],
        json!(id),
        "the seal declared in the core's scoped @context survives the round trip: {stored}"
    );
    assert_eq!(
        proof["value"]["cryptosuite"],
        json!("eddsa-jcs-2022"),
        "the proof value is stored verbatim: {stored}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// A remote `@context` at an address nothing answers on, so its resolution
/// fails with a connection refusal rather than a slow timeout (clause 5.5.6).
const UNREACHABLE_CONTEXT: &str = "http://127.0.0.1:9/unreachable.jsonld";

/// The problem type clause 5.5.6 gives a `@context` that cannot be retrieved.
const LD_CONTEXT_NOT_AVAILABLE: &str = "https://uri.etsi.org/ngsi-ld/errors/LdContextNotAvailable";

/// The status a resource at `path` answers a plain GET with.
async fn status_of(client: &TestClient, path: &str) -> StatusCode {
    client.raw(Method::GET, path).send().await.expect("retrieve request").status
}

/// The resource at `path`, read back as JSON.
async fn resource(client: &TestClient, path: &str) -> Value {
    let response = client.raw(Method::GET, path).send().await.expect("retrieve request");
    assert_eq!(response.status, StatusCode::OK, "{path}: {:?}", response.body);
    response.json_value().expect("a resource is JSON")
}

/// Create the resource `body` at `collection` and require the broker to accept
/// it.
async fn create_resource(client: &TestClient, collection: &str, body: &Value, scenario: &str) {
    let response = send_ld_json(client, Method::POST, collection, body).await;
    assert_eq!(response.status, StatusCode::CREATED, "{scenario}: {:?}", response.body);
}

/// A Subscription to `Device`s, its `entities` selector carrying `selector`'s
/// members.
fn subscription(id: &str, selector: &Value) -> Value {
    let mut entity_selector = json!({"type": "Device"});
    if let (Some(target), Some(members)) = (entity_selector.as_object_mut(), selector.as_object()) {
        target.extend(members.clone()); // clone: the fixture keeps its selector members
    }
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [entity_selector],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:9/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// A Context Source Registration for `Device`s, its first entity selector
/// carrying `selector`'s members.
fn registration(id: &str, selector: &Value) -> Value {
    let mut entity_selector = json!({"type": "Device"});
    if let (Some(target), Some(members)) = (entity_selector.as_object_mut(), selector.as_object()) {
        target.extend(members.clone()); // clone: the fixture keeps its selector members
    }
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [entity_selector]}],
        "endpoint": "http://context-source.invalid:1026/",
        "@context": [CORE_CONTEXT]
    })
}

/// Clause 5.5.7: a Subscription or a Context Source Registration carrying
/// an `@context` below its top level is `BadRequestData` on create and on
/// update, and nothing is created or changed.
#[tokio::test]
async fn regression_embedded_user_context_subscriptions_and_registrations_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let embedded = json!({"@context": {"Device": "https://example.org/Device"}});

    for (label, collection, refused_id, kept_id, build) in [
        (
            "subscription",
            "/ngsi-ld/v1/subscriptions",
            scope.id("Subscription:embedded-refused"),
            scope.id("Subscription:embedded-kept"),
            subscription as fn(&str, &Value) -> Value,
        ),
        (
            "context source subscription",
            "/ngsi-ld/v1/csourceSubscriptions",
            scope.id("Subscription:csource-embedded-refused"),
            scope.id("Subscription:csource-embedded-kept"),
            subscription,
        ),
        (
            "context source registration",
            "/ngsi-ld/v1/csourceRegistrations",
            scope.id("ContextSourceRegistration:embedded-refused"),
            scope.id("ContextSourceRegistration:embedded-kept"),
            registration,
        ),
    ] {
        let created = send_ld_json(client, Method::POST, collection, &build(&refused_id, &embedded)).await;
        assert_embedded_context_refused(&created, &format!("{label} create"));
        assert_eq!(
            status_of(client, &format!("{collection}/{refused_id}")).await,
            StatusCode::NOT_FOUND,
            "{label}: a refused create stores nothing"
        );

        create_resource(client, collection, &build(&kept_id, &json!({})), &format!("{label}: seeding")).await;
        let mut fragment = build(&kept_id, &json!({"@context": {}}));
        if let Some(members) = fragment.as_object_mut() {
            members.remove("id");
            members.insert("description".to_owned(), json!("patched"));
        }
        let updated = send_ld_json(client, Method::PATCH, &format!("{collection}/{kept_id}"), &fragment).await;
        assert_embedded_context_refused(&updated, &format!("{label} update"));
        let kept = resource(client, &format!("{collection}/{kept_id}")).await;
        assert!(kept.get("description").is_none(), "{label}: a refused update changes nothing: {kept}");

        let deleted = client
            .raw(Method::DELETE, &format!("{collection}/{kept_id}"))
            .send()
            .await
            .expect("delete request");
        assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{label}: cleanup");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.7 holds for every NGSI-LD document a client sends: a posted
/// query, an Entity Map creation from one and an Entity Map update carrying an
/// `@context` below the top level are `BadRequestData`. A query's entity
/// selector admits no member but `id`, `idPattern` and `type`, yet the body is
/// judged only once its context was admitted, so the refusal names clause
/// 5.5.7 rather than the member the decoding would refuse.
#[tokio::test]
async fn regression_embedded_user_context_queries_and_entity_maps_are_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let query = json!({
        "type": "Query",
        "entities": [{"type": "Device", "@context": {"Device": "https://example.org/Device"}}],
        "@context": [CORE_CONTEXT]
    });
    for path in [
        "/ngsi-ld/v1/entityOperations/query",
        "/ngsi-ld/v1/temporal/entityOperations/query",
        "/ngsi-ld/v1/entityMaps",
        "/ngsi-ld/v1/temporal/entityMaps",
    ] {
        let response = send_ld_json(client, Method::POST, path, &query).await;
        assert_embedded_context_refused(&response, path);
    }

    let device = scope.id("Device:entity-map-target");
    create_device(client, &plain_device(&device), "seeding the map's entity").await;
    let created = client
        .raw(Method::GET, &format!("/ngsi-ld/v1/entityMaps?type=Device&id={}", urlencoding::encode(&device)))
        .send()
        .await
        .expect("create entity map request");
    assert_eq!(created.status, StatusCode::CREATED, "{:?}", created.body);
    let map = created.json_value().expect("a created map is JSON");
    let map_id = map["id"].as_str().expect("the map names its id").to_owned();
    let path = format!("/ngsi-ld/v1/entityMaps/{map_id}");
    let map_update = json!({
        "expiresAt": "2099-01-01T00:00:00Z",
        "linkedMaps": {"@context": {}},
        "@context": [CORE_CONTEXT]
    });
    let updated = send_ld_json(client, Method::PATCH, &path, &map_update).await;
    assert_embedded_context_refused(&updated, "entity map update");
    assert_eq!(resource(client, &path).await["expiresAt"], map["expiresAt"], "a refused update changes nothing");

    let deleted = client.raw(Method::DELETE, &path).send().await.expect("delete request");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "cleanup");
    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.4 judges data "according to the @context": a request whose
/// `@context` cannot be retrieved is `LdContextNotAvailable` whatever it
/// embedded, on a single write, a subscription and each entity of a batch.
#[tokio::test]
async fn regression_embedded_user_context_an_unreachable_context_outranks_an_embedded_one() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let link = format!(r#"<{UNREACHABLE_CONTEXT}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#);

    let entity = json!({
        "id": scope.id("Device:unreachable-embedded"),
        "type": "Device",
        "speed": {"type": "Property", "value": 1, "@context": {}}
    });
    let mut subscribed = subscription(&scope.id("Subscription:unreachable-embedded"), &json!({"@context": {}}));
    if let Some(members) = subscribed.as_object_mut() {
        members.remove("@context");
    }
    for (label, path, body) in [
        ("entity create", "/ngsi-ld/v1/entities", entity),
        ("subscription create", "/ngsi-ld/v1/subscriptions", subscribed),
    ] {
        let response = client
            .raw(Method::POST, path)
            .header("content-type", "application/json")
            .header("link", link.as_str())
            .json(&body)
            .send()
            .await
            .expect("request");
        assert_eq!(response.status, StatusCode::GATEWAY_TIMEOUT, "{label}: {:?}", response.body);
        let problem = response.json_value().expect("a refusal carries a problem details body");
        assert_eq!(problem["type"], json!(LD_CONTEXT_NOT_AVAILABLE), "{label}: {problem}");
    }

    let unreachable = scope.id("Device:batch-unreachable-embedded");
    let mut item = device_with(&unreachable, "speed", &json!({"type": "Property", "value": 1, "@context": {}}));
    item["@context"] = json!([UNREACHABLE_CONTEXT]);
    let response = send_ld_json(client, Method::POST, "/ngsi-ld/v1/entityOperations/create", &json!([item])).await;
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "{:?}", response.body);
    let result = response.json_value().expect("a batch answers with a clause 5.2.16 result");
    let reported = result["errors"]
        .as_array()
        .and_then(|errors| errors.iter().find(|error| error["entityId"] == json!(unreachable)))
        .unwrap_or(&Value::Null);
    assert_eq!(reported["error"]["type"], json!(LD_CONTEXT_NOT_AVAILABLE), "{result}");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.5.4 judges a payload "syntactically correct, according to the
/// @context": a body that does not decode into what the operation takes is
/// refused only once its context loaded, so an unreachable context answers
/// `LdContextNotAvailable` first on every typed body, and on each entity of a
/// batch.
#[tokio::test]
async fn regression_embedded_user_context_an_unreachable_context_outranks_an_undecodable_body() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let link = format!(r#"<{UNREACHABLE_CONTEXT}>; rel="http://www.w3.org/ns/json-ld#context"; type="application/ld+json""#);

    for (label, method, path, body) in [
        (
            "entity create",
            Method::POST,
            "/ngsi-ld/v1/entities".to_owned(),
            json!({"id": scope.id("Device:unreachable-undecodable"), "type": 5}),
        ),
        (
            "entity query",
            Method::POST,
            "/ngsi-ld/v1/entityOperations/query".to_owned(),
            json!({"type": "Query", "entities": [{"type": "Device", "bogus": 1}]}),
        ),
        (
            "temporal entity query",
            Method::POST,
            "/ngsi-ld/v1/temporal/entityOperations/query".to_owned(),
            json!({"type": "Query", "entities": [{"type": "Device", "bogus": 1}]}),
        ),
        (
            "entity map creation",
            Method::POST,
            "/ngsi-ld/v1/entityMaps".to_owned(),
            json!({"type": "Query", "entities": [{"type": "Device", "bogus": 1}]}),
        ),
        (
            "batch delete",
            Method::POST,
            "/ngsi-ld/v1/entityOperations/delete".to_owned(),
            json!(["urn:ngsi-ld:Device:a", 42]),
        ),
    ] {
        let response = client
            .raw(method, &path)
            .header("content-type", "application/json")
            .header("link", link.as_str())
            .json(&body)
            .send()
            .await
            .expect("request");
        assert_eq!(response.status, StatusCode::GATEWAY_TIMEOUT, "{label}: {:?}", response.body);
        let problem = response.json_value().expect("a refusal carries a problem details body");
        assert_eq!(problem["type"], json!(LD_CONTEXT_NOT_AVAILABLE), "{label}: {problem}");
    }

    let unreachable = scope.id("Device:batch-unreachable-undecodable");
    let decodable = scope.id("Device:batch-reachable-undecodable");
    let body = json!([
        {"id": unreachable, "type": 5, "@context": [UNREACHABLE_CONTEXT]},
        {"id": decodable, "type": 5, "@context": [CORE_CONTEXT]}
    ]);
    let response = send_ld_json(client, Method::POST, "/ngsi-ld/v1/entityOperations/create", &body).await;
    assert_eq!(response.status, StatusCode::MULTI_STATUS, "{:?}", response.body);
    let result = response.json_value().expect("a batch answers with a clause 5.2.16 result");
    let reported = |id: &str| {
        result["errors"]
            .as_array()
            .and_then(|errors| errors.iter().find(|error| error["entityId"] == json!(id)))
            .map_or(Value::Null, |error| error["error"]["type"].clone()) // clone: the type outlives the borrowed result
    };
    assert_eq!(reported(&unreachable), json!(LD_CONTEXT_NOT_AVAILABLE), "{result}");
    assert_eq!(reported(&decodable), json!(BAD_REQUEST_DATA), "{result}");

    scope.finish().await.expect("cleanup failed");
}
