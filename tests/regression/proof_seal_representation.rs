//! How the two data-integrity seals of clause 4.5.2.2 are represented.
//!
//! ETSI GS CIM 009 v1.9.1 clause 4.5.2.2 lists `ngsildproof` among a Property's
//! optional members as "a Property with the non-reified subproperties
//! `entityIdSealed` and `entityTypeSealed` as specified in [35]", whose `value`
//! "shall be an object containing the W3C Data integrity `proof` structure".
//! Clauses 4.5.2.3, 4.5.18, 4.5.20, 4.5.21, 4.5.22 and 4.5.24 repeat the same
//! member on every other attribute family, and clause C.11 prints the shape.
//!
//! *Non-reified* is the whole of the requirement the broker got wrong. The
//! concise-to-normalized rewrite of clause 4.5.2.3 wraps every member of an
//! instance that is not one of the family's own into `{"type": "Property",
//! "value": …}`, and it wrapped the two seals as well: a proof written as
//! clause C.11 prints it read back with each seal reified as a Property of its
//! own, and `entityTypeSealed` — declared `"@type": "@vocab"` inside
//! `ngsildproof`'s scoped `@context` — compacted to the prefixed IRI
//! `ngsi-ld:entityTypeSealed` rather than to its term, because a reified node
//! object does not match that term definition.
//!
//! Nothing here verifies a proof. Verification appears nowhere in CIM 009: the
//! seal semantics are delegated to ETSI GS CIM 019, and the one crypto suite
//! the document names sits in the informative Annex C. The broker's obligation
//! is to accept, store and hand back the member exactly as written, which is
//! what these tests assert.
//!
//! Naming convention: `regression_<finding>_<scenario>`.

#![allow(clippy::expect_used, clippy::unwrap_used, reason = "integration test setup uses explicit assertions")]

use andromeda_test_client::{response::TestResponse, test_client::TestClient};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use http::Method;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};

/// The problem type clause 5.5.4 gives data that is not syntactically correct
/// according to the API data type definitions.
const BAD_REQUEST_DATA: &str = "https://uri.etsi.org/ngsi-ld/errors/BadRequestData";

/// The entity type the seals in these fixtures claim to seal.
const SEALED_TYPE: &str = "Building";

/// The eight attribute families of clause 5.2.4, each with the value member
/// its own representation clause gives it.
///
/// Every one of them carries `ngsildproof` in its "Optional" list, so the
/// requirement is the same eight times over and is asserted eight times over.
fn attribute_families() -> [(&'static str, Value); 8] {
    [
        ("Property", json!({"type": "Property", "value": 21.5})),
        ("Relationship", json!({"type": "Relationship", "object": "urn:ngsi-ld:Site:1"})),
        (
            "GeoProperty",
            json!({"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.4, 52.5]}}),
        ),
        (
            "LanguageProperty",
            json!({"type": "LanguageProperty", "languageMap": {"en": "Door", "de": "Tuer"}}),
        ),
        ("VocabProperty", json!({"type": "VocabProperty", "vocab": "sensor"})),
        ("ListProperty", json!({"type": "ListProperty", "valueList": [1, 2, 3]})),
        ("ListRelationship", json!({"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Room:1"]})),
        ("JsonProperty", json!({"type": "JsonProperty", "json": {"floors": 3}})),
    ]
}

/// The W3C Data Integrity `proof` structure clause C.11 prints, member for
/// member.
fn data_integrity_proof() -> Value {
    json!({
        "type": "DataIntegrityProof",
        "created": "2025-01-27T21:02:24Z",
        "verificationMethod": "https://example.edu/issuers/565049#z6MkwXG2WjeQnN....Hc6SaVWoT",
        "cryptosuite": "eddsa-rdfc-2022",
        "proofPurpose": "assertionMethod",
        "proofValue": "z3XrH3diVCqpVHXkE7WbnictqyQCkJBGTx....NRTzmuoWU1Y2FyqGfSV9eS"
    })
}

/// The `ngsildproof` Property clause 4.5.2.2 defines, sealing `entity_id`.
fn ngsildproof(entity_id: &str) -> Value {
    json!({
        "type": "Property",
        "entityIdSealed": entity_id,
        "entityTypeSealed": SEALED_TYPE,
        "value": data_integrity_proof()
    })
}

/// One `Building` whose single attribute is `node` under the name `attribute`.
fn building_with(id: &str, attribute: &str, node: &Value) -> Value {
    json!({
        "id": id,
        "type": SEALED_TYPE,
        attribute: node,
        "@context": [TEST_SUITE_CONTEXT]
    })
}

/// One `Building` whose `sealed` attribute is of the named family and carries
/// the `ngsildproof` sub-Property.
fn sealed_building(id: &str, family_node: &Value) -> Value {
    let mut node = family_node.clone();
    node.as_object_mut()
        .expect("a family fixture is an attribute instance")
        .insert("ngsildproof".to_owned(), ngsildproof(id));
    building_with(id, "sealed", &node)
}

/// Create `entity` and assert the broker stored it.
async fn create(client: &TestClient, entity: &Value, scenario: &str) {
    let created = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(created.status, StatusCode::CREATED, "{scenario}: {:?}", created.body);
}

/// Create `entity` and assert the broker refused it as `BadRequestData`.
async fn assert_create_refused(client: &TestClient, entity: &Value, scenario: &str) {
    let response = client.post_entity(entity).with_ld_json().send().await.expect("create entity request");
    assert_eq!(response.status, StatusCode::BAD_REQUEST, "{scenario}: {:?}", response.body);
    let problem = response.json_value().expect("a refusal carries a problem details body");
    assert_eq!(problem["type"], json!(BAD_REQUEST_DATA), "{scenario}: {problem}");
}

/// Retrieve `id` under `query` and answer the rendered entity.
async fn retrieve(client: &TestClient, id: &str, query: &str) -> Value {
    let response = client
        .get_entity(id, query, "application/ld+json", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve {id}?{query}: {:?}", response.body);
    response.json_value().expect("a retrieval answers with an entity")
}

/// Replace one attribute wholesale (clause 5.6.19), which the test client has
/// no typed method for.
async fn replace_attribute(client: &TestClient, id: &str, attribute: &str, fragment: &Value) -> TestResponse {
    client
        .raw(Method::PUT, &format!("/ngsi-ld/v1/entities/{id}/attrs/{attribute}"))
        .header("content-type", "application/ld+json")
        .json(fragment)
        .send()
        .await
        .expect("replace attribute request")
}

/// Assert `proof` is the `ngsildproof` clause 4.5.2.2 describes: both seals
/// bare, and the Data Integrity structure intact.
///
/// A bare seal is asserted as a JSON string rather than merely as "not the
/// value that was written": the reification the defect produced put that same
/// value inside `{"type": "Property", "value": …}`, so only the JSON kind tells
/// the two apart.
fn assert_seals_are_bare(proof: &Value, entity_id: &str, scenario: &str) {
    assert!(
        proof["entityIdSealed"].is_string(),
        "{scenario}: clause 4.5.2.2 makes entityIdSealed a non-reified subproperty: {proof}"
    );
    assert!(
        proof["entityTypeSealed"].is_string(),
        "{scenario}: clause 4.5.2.2 makes entityTypeSealed a non-reified subproperty: {proof}"
    );
    assert_eq!(proof["entityIdSealed"], json!(entity_id), "{scenario}: {proof}");
    assert_eq!(proof["entityTypeSealed"], json!(SEALED_TYPE), "{scenario}: {proof}");
    assert_eq!(
        proof["value"],
        data_integrity_proof(),
        "{scenario}: clause 4.5.2.2 carries the W3C Data Integrity proof structure verbatim: {proof}"
    );
}

/// Assert nothing in `rendered` spells a seal as anything but its term.
///
/// `entityTypeSealed` is declared `"@type": "@vocab"` inside `ngsildproof`'s
/// scoped context, and a rendering that does not match that declaration falls
/// back to the compact IRI `ngsi-ld:entityTypeSealed`. Searching the serialized
/// body catches the fallback wherever in the entity it occurs, including under
/// an attribute this assertion does not name.
fn assert_no_prefixed_seal_spelling(rendered: &Value, scenario: &str) {
    let serialized = rendered.to_string();
    for spelling in [
        "ngsi-ld:entityIdSealed",
        "ngsi-ld:entityTypeSealed",
        "https://uri.etsi.org/ngsi-ld/entityIdSealed",
        "https://uri.etsi.org/ngsi-ld/entityTypeSealed",
        "default-context/entityIdSealed",
        "default-context/entityTypeSealed",
    ] {
        assert!(
            !serialized.contains(spelling),
            "{scenario}: clause 4.4 declares the seals as terms of ngsildproof's scoped context, but the rendering spells one '{spelling}': {serialized}"
        );
    }
}

/// Clause 4.5.2.2 and its seven siblings: on every attribute family the
/// `ngsildproof` member carries two non-reified subproperties, so what a client
/// wrote is what a normalized retrieval hands back.
#[tokio::test]
async fn regression_seal_reification_every_family_carries_a_bare_seal_pair_when_normalized() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (family, node) in attribute_families() {
        let id = scope.id(&format!("Building:seal-normalized-{family}"));
        create(client, &sealed_building(&id, &node), family).await;

        let rendered = retrieve(client, &id, "").await;
        let proof = &rendered["sealed"]["ngsildproof"];
        assert_seals_are_bare(proof, &id, family);
        assert_eq!(
            proof["type"],
            json!("Property"),
            "{family}: clause 4.5.2.2 defines ngsildproof as a Property: {proof}"
        );
        assert_no_prefixed_seal_spelling(&rendered, family);
    }

    scope.finish().await.expect("cleanup failed");
}

/// The concise representation of clause 4.5.2.3 and its siblings drops the
/// inferable `type` and nothing else, so both seals stay exactly where the
/// normalized representation put them.
#[tokio::test]
async fn regression_seal_reification_every_family_carries_a_bare_seal_pair_when_concise() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (family, node) in attribute_families() {
        let id = scope.id(&format!("Building:seal-concise-{family}"));
        create(client, &sealed_building(&id, &node), family).await;

        let rendered = retrieve(client, &id, "options=concise").await;
        let proof = &rendered["sealed"]["ngsildproof"];
        assert_seals_are_bare(proof, &id, family);
        assert_no_prefixed_seal_spelling(&rendered, family);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.4 declares `entityTypeSealed` as `{"@id": "ngsi-ld:entityTypeSealed",
/// "@type": "@vocab"}` inside `ngsildproof`'s scoped `@context`. The `@vocab`
/// coercion is what makes the stored value an IRI node, and it is what a
/// compaction has to match to answer the term: a seal the write path reified
/// into a Property node no longer matches it, and the rendering falls back to
/// the compact IRI.
#[tokio::test]
async fn regression_seal_reification_entity_type_sealed_compacts_to_its_term_not_a_prefixed_iri() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:seal-term-compaction");

    create(client, &sealed_building(&id, &json!({"type": "Property", "value": 21.5})), "term compaction").await;

    for query in ["", "options=concise"] {
        let rendered = retrieve(client, &id, query).await;
        let proof = &rendered["sealed"]["ngsildproof"];
        assert!(
            proof.get("entityTypeSealed").is_some(),
            "the term is the only spelling clause 4.4 declares: {proof}"
        );
        assert!(
            proof.get("ngsi-ld:entityTypeSealed").is_none(),
            "a prefixed IRI is the fallback of a value the term definition does not match: {proof}"
        );
        assert_no_prefixed_seal_spelling(&rendered, query);
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.2.2, "Prohibited": `entityIdSealed` and `entityTypeSealed` "shall
/// never be present, unless the Property name is `ngsildproof`". Making the
/// seals envelope members under `ngsildproof` must not make them admissible
/// anywhere else, on any of the eight families.
#[tokio::test]
async fn regression_seal_reification_a_seal_outside_ngsildproof_is_still_refused_on_every_family() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (family, node) in attribute_families() {
        for member in ["entityIdSealed", "entityTypeSealed"] {
            let id = scope.id(&format!("Building:forged-{family}-{member}"));
            let mut forged = node.clone();
            forged
                .as_object_mut()
                .expect("a family fixture is an attribute instance")
                .insert(member.to_owned(), json!(SEALED_TYPE));
            assert_create_refused(client, &building_with(&id, "sealed", &forged), &format!("{family}/{member}")).await;
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clauses 4.5.3.2 and 4.5.3.3 state "`entityIdSealed` and `entityTypeSealed`
/// shall never be present" for a Relationship, with none of the exception
/// clause 4.5.2.2 grants a Property. The name alone therefore admits nothing:
/// an attribute that calls itself `ngsildproof` and declares — or, concisely,
/// implies — a Relationship carries no admissible seal.
#[tokio::test]
async fn regression_seal_reification_a_relationship_named_ngsildproof_admits_no_seal() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();

    for (spelling, proof) in [
        (
            "normalized",
            json!({"type": "Relationship", "object": "urn:ngsi-ld:Proof:1", "entityIdSealed": "urn:ngsi-ld:Building:1"}),
        ),
        ("concise", json!({"object": "urn:ngsi-ld:Proof:1", "entityTypeSealed": SEALED_TYPE})),
    ] {
        let id = scope.id(&format!("Building:relationship-proof-{spelling}"));
        let node = json!({"type": "Property", "value": 21.5, "ngsildproof": proof});
        assert_create_refused(client, &building_with(&id, "sealed", &node), spelling).await;
    }

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.4 takes a bare Attribute Fragment whose name is in the request
/// URI rather than in the payload. Both halves of the requirement are decided
/// by that name: the rewrite of clause 4.5.2.3 must leave the seals unreified,
/// and the expansion must resolve them through `ngsildproof`'s scoped
/// `@context` (clause 4.4) instead of against the default `@vocab`.
#[tokio::test]
async fn regression_seal_reification_a_partial_attribute_update_of_ngsildproof_keeps_the_seals_bare() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:partial-update-ngsildproof");

    create(client, &building_with(&id, "ngsildproof", &ngsildproof(&id)), "seed").await;

    let updated = client
        .partial_update_attr(
            &id,
            "ngsildproof",
            &json!({
                "type": "Property",
                "entityIdSealed": "urn:ngsi-ld:Building:resealed",
                "entityTypeSealed": SEALED_TYPE,
                "value": data_integrity_proof(),
                "@context": [TEST_SUITE_CONTEXT]
            }),
        )
        .await
        .expect("partial update request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "partial update: {:?}", updated.body);

    let rendered = retrieve(client, &id, "").await;
    assert_seals_are_bare(&rendered["ngsildproof"], "urn:ngsi-ld:Building:resealed", "partial update");
    assert_no_prefixed_seal_spelling(&rendered, "partial update");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.19 replaces one attribute wholesale from the same bare Attribute
/// Fragment, and carries the same two obligations as clause 5.6.4.
#[tokio::test]
async fn regression_seal_reification_a_replace_attribute_of_ngsildproof_keeps_the_seals_bare() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:replace-ngsildproof");

    create(client, &building_with(&id, "ngsildproof", &ngsildproof(&id)), "seed").await;

    let replaced = replace_attribute(
        client,
        &id,
        "ngsildproof",
        &json!({
            "type": "Property",
            "entityIdSealed": "urn:ngsi-ld:Building:replaced",
            "entityTypeSealed": SEALED_TYPE,
            "value": data_integrity_proof(),
            "@context": [TEST_SUITE_CONTEXT]
        }),
    )
    .await;
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "replace attribute: {:?}", replaced.body);

    let rendered = retrieve(client, &id, "").await;
    assert_seals_are_bare(&rendered["ngsildproof"], "urn:ngsi-ld:Building:replaced", "replace attribute");
    assert_no_prefixed_seal_spelling(&rendered, "replace attribute");

    scope.finish().await.expect("cleanup failed");
}

/// The same two operations reaching an attribute that merely *carries* a proof:
/// the Fragment names `temperature`, and the seals sit one level down under the
/// `ngsildproof` sub-attribute, where clause 4.5.2.2 puts them in clause C.11.
#[tokio::test]
async fn regression_seal_reification_an_attribute_fragment_carrying_a_proof_keeps_the_seals_bare() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:fragment-nested-proof");

    create(client, &building_with(&id, "temperature", &json!({"type": "Property", "value": 21.5})), "seed").await;

    let fragment = json!({
        "type": "Property",
        "value": 22.5,
        "ngsildproof": ngsildproof(&id),
        "@context": [TEST_SUITE_CONTEXT]
    });

    let updated = client.partial_update_attr(&id, "temperature", &fragment).await.expect("partial update request");
    assert_eq!(updated.status, StatusCode::NO_CONTENT, "partial update: {:?}", updated.body);
    let after_update = retrieve(client, &id, "").await;
    assert_seals_are_bare(&after_update["temperature"]["ngsildproof"], &id, "partial update of a carrier");
    assert_no_prefixed_seal_spelling(&after_update, "partial update of a carrier");

    let replaced = replace_attribute(client, &id, "temperature", &fragment).await;
    assert_eq!(replaced.status, StatusCode::NO_CONTENT, "replace attribute: {:?}", replaced.body);
    let after_replace = retrieve(client, &id, "").await;
    assert_seals_are_bare(&after_replace["temperature"]["ngsildproof"], &id, "replace of a carrier");
    assert_no_prefixed_seal_spelling(&after_replace, "replace of a carrier");

    scope.finish().await.expect("cleanup failed");
}

/// A forged seal must stay refused on the two Attribute Fragment operations as
/// well: clause 4.5.2.2 decides on the name, and on these two the name comes
/// from the request URI.
#[tokio::test]
async fn regression_seal_reification_an_attribute_fragment_forging_a_seal_is_refused() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:fragment-forged-seal");

    create(client, &building_with(&id, "temperature", &json!({"type": "Property", "value": 21.5})), "seed").await;

    let forged = json!({
        "type": "Property",
        "value": 22.5,
        "entityIdSealed": "urn:ngsi-ld:Building:forged",
        "@context": [TEST_SUITE_CONTEXT]
    });

    let updated = client.partial_update_attr(&id, "temperature", &forged).await.expect("partial update request");
    assert_eq!(updated.status, StatusCode::BAD_REQUEST, "partial update: {:?}", updated.body);

    let replaced = replace_attribute(client, &id, "temperature", &forged).await;
    assert_eq!(replaced.status, StatusCode::BAD_REQUEST, "replace attribute: {:?}", replaced.body);

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.3 appends attributes from an Entity Fragment, and clause 5.6.17
/// merges one. Both go through the entity-level rewrite rather than the
/// Attribute Fragment one, so both are asserted separately from clauses 5.6.4
/// and 5.6.19.
#[tokio::test]
async fn regression_seal_reification_appending_and_merging_a_proof_keeps_the_seals_bare() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:append-and-merge-proof");

    create(client, &building_with(&id, "temperature", &json!({"type": "Property", "value": 21.5})), "seed").await;

    let appended = client
        .append_entity_attrs(
            &id,
            &building_with(&id, "sealed", &json!({"type": "Property", "value": 1, "ngsildproof": ngsildproof(&id)})),
        )
        .await
        .expect("append attributes request");
    assert_eq!(appended.status, StatusCode::NO_CONTENT, "append: {:?}", appended.body);
    let after_append = retrieve(client, &id, "").await;
    assert_seals_are_bare(&after_append["sealed"]["ngsildproof"], &id, "append");
    assert_no_prefixed_seal_spelling(&after_append, "append");

    let merged = client
        .merge_entity(
            &id,
            &json!({"temperature": {"type": "Property", "value": 23.5, "ngsildproof": ngsildproof(&id)}}),
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("merge entity request");
    assert_eq!(merged.status, StatusCode::NO_CONTENT, "merge: {:?}", merged.body);
    let after_merge = retrieve(client, &id, "").await;
    assert_seals_are_bare(&after_merge["temperature"]["ngsildproof"], &id, "merge");
    assert_no_prefixed_seal_spelling(&after_merge, "merge");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.7 creates entities in a batch through the same rewrite, and the
/// batch path has its own fast expansion, so it is asserted in its own right.
#[tokio::test]
async fn regression_seal_reification_a_batch_creation_keeps_the_seals_bare() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:batch-proof");

    let created = client
        .batch_create(&json!([sealed_building(&id, &json!({"type": "Property", "value": 21.5}))]))
        .await
        .expect("batch create request");
    assert_eq!(created.status, StatusCode::CREATED, "batch create: {:?}", created.body);

    let rendered = retrieve(client, &id, "").await;
    assert_seals_are_bare(&rendered["sealed"]["ngsildproof"], &id, "batch create");
    assert_no_prefixed_seal_spelling(&rendered, "batch create");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.11 writes the temporal evolution of an entity, and clause 4.5.8
/// renders one instance of it as the attribute instance it is — sub-attributes
/// and all. A proof written on a temporal instance therefore carries the same
/// requirement.
#[tokio::test]
async fn regression_seal_reification_the_temporal_write_path_keeps_the_seals_bare() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:temporal-proof");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": SEALED_TYPE,
            "temperature": [{
                "type": "Property",
                "value": 21.5,
                "observedAt": "2025-01-01T00:00:00Z",
                "ngsildproof": ngsildproof(&id)
            }],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create temporal entity: {:?}", created.body);

    let response = client
        .get_temporal_entity(&id, "timerel=after&timeAt=2024-01-01T00:00:00Z", Some(TEST_SUITE_CONTEXT))
        .await
        .expect("retrieve temporal entity request");
    assert_eq!(response.status, StatusCode::OK, "retrieve temporal entity: {:?}", response.body);
    let rendered = response.json_value().expect("a temporal retrieval answers with an entity");

    let instances = rendered["temperature"].as_array().expect("clause 4.5.8 renders instances as an array");
    let instance = instances.first().expect("the written instance is there");
    assert_seals_are_bare(&instance["ngsildproof"], &id, "temporal");
    assert_no_prefixed_seal_spelling(&rendered, "temporal");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.7.4.4 admits a temporal query limited to the local scope with no
/// other narrowing (input e), and clause 5.7.4.3 says that then "no further
/// restrictions have to be provided". A sealed instance is compacted through
/// the JSON-LD processor rather than the term walk, and the processor's
/// recursion once overflowed a Tokio worker's stack and aborted the broker on
/// exactly this query, taking every tenant down.
#[tokio::test]
async fn regression_r58_a_local_temporal_query_renders_a_sealed_instance() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:local-query-proof");

    let created = client
        .create_temporal_entity(&json!({
            "id": id,
            "type": SEALED_TYPE,
            "temperature": [{
                "type": "Property",
                "value": 21.5,
                "observedAt": "2025-01-01T00:00:00Z",
                "ngsildproof": ngsildproof(&id)
            }],
            "@context": [TEST_SUITE_CONTEXT]
        }))
        .await
        .expect("create temporal entity request");
    assert_eq!(created.status, StatusCode::CREATED, "create temporal entity: {:?}", created.body);

    let response = client
        .query_temporal_entities(
            &scope.scope_query("timerel=after&timeAt=2024-01-01T00:00:00Z&local=true"),
            Some(TEST_SUITE_CONTEXT),
        )
        .await
        .expect("query temporal entities request");
    assert_eq!(response.status, StatusCode::OK, "query temporal entities: {:?}", response.body);
    let rendered = response.json_value().expect("a temporal query answers with an entity array");

    let entities = rendered.as_array().expect("clause 5.7.4 answers with an array of entities");
    assert_eq!(entities.len(), 1, "only the sealed entity this run wrote matches: {rendered}");
    let entity = entities.first().expect("the sealed entity is there");
    assert_eq!(entity["id"], json!(id), "{entity}");
    let instance = entity["temperature"]
        .as_array()
        .and_then(|instances| instances.first())
        .expect("clause 4.5.8 renders the written instance");
    assert_seals_are_bare(&instance["ngsildproof"], &id, "local temporal query");
    assert_no_prefixed_seal_spelling(entity, "local temporal query");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.4 keeps only each attribute's value in the simplified
/// representation, discarding every sub-attribute. `ngsildproof` is a
/// sub-attribute, so it goes with the rest — and taking it with them is what
/// keeps `keyValues` the plain name-to-value map the clause defines.
#[tokio::test]
async fn regression_seal_reification_the_simplified_representation_drops_the_proof_with_every_sub_attribute() {
    let scope = TestEnvironment::from_suite("regression").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:simplified-proof");

    create(client, &sealed_building(&id, &json!({"type": "Property", "value": 21.5})), "seed").await;

    let rendered = retrieve(client, &id, "options=keyValues").await;
    assert_eq!(
        rendered["sealed"],
        json!(21.5),
        "clause 4.5.4 renders an attribute as its value alone: {rendered}"
    );
    assert_no_prefixed_seal_spelling(&rendered, "simplified");

    scope.finish().await.expect("cleanup failed");
}
