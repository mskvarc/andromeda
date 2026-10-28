//! ETSI `D013_01_inc`: a batch upsert replacing two entities an inclusive registration matches is sent to the
//! context source and replaces the entities on the broker as well (clauses 4.3.3, 4.3.6.2 and 5.6.8).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, stub_count, unregister},
        provision::batch_entities::{
            batch_sources::{batch_registration, delete_vehicles},
            forwarded_documents::parked2_forwarded,
            upsert_batch_of_entities::upsert_request::upsert,
        },
        redirected_request::leading_context,
        vehicle_payloads::vehicle_parked2,
    },
    suite_keywords::{create_entity_with_context_string_any, ld_json_body_client},
    suite_payloads::vehicle_parked,
};
use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    comparison::matches_document,
    context_source::ContextSource,
    namespace::Namespace,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::headers::Accept, model::NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that if one requests the Context Broker to replace a batch of entities that match an inclusive
/// registration, these are replaced on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.8
/// Changed: the context source is the test's own server in place of the suite's HTTP control server: the registration
/// names its address, each Set Stub Reply is a stub matched in the order it was set by the server's rule (case,
/// trailing slashes, the query tokens, the attrs and urn rules), and a request no stub matches is answered 502 after
/// 5 s where the server closes the connection.
/// Changed: the registration's idPattern urn:ngsi-ld:Vehicle:* becomes `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that
/// names no entity type matches a registration on the id alone (clause 5.12), so the pattern as written would draw
/// every other test's requests on a Vehicle id into this test's source.
/// Changed: Get Stub Count counts the requests of the source the stub's rule matches.
/// Changed: the retrieval and the query are held to a success status, where the case reads the body of any status.
/// Changed: the teardown delete of the registration is held to the 204 a deleted registration answers, not only to a
/// status below 400.
#[gtest]
#[tokio::test]
async fn d013_01_inc_batch_upsert_entities_with_inclusive_registration_without_update_flag() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "isParked2"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string_any(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity_with_context_string_any(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let _vehicles = Cleanup::of(&broker).entity(&namespace.urn("Vehicle", 1)?).entity(&namespace.urn("Vehicle", 2)?);
    let _registration = register(
        &broker,
        &vocabulary,
        &batch_registration(&namespace, 1, &source_endpoint(&source, &namespace, "")?, None)?,
    )
    .await?;
    let source = serve(source).await?;
    let reader = broker.with_context(vocabulary.context())?;
    let first = namespace.urn("Vehicle", 1)?;
    let old = json_body(&reader.with_accept(Accept::any()).with_responses().entities().retrieve(&first).send().await?)?;
    let old_brand_name = old.get("brandName").or_fail()?;
    let rule = || ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entityoperations/upsert"));
    stub(&source, rule(), StatusCode::NO_CONTENT, None).await?;
    let writer = ld_json_body_client(&broker, &vocabulary)?;
    upsert(&writer, &[vehicle_parked2(&namespace, 1)?, vehicle_parked2(&namespace, 2)?], "replace").await?;
    let received = reader
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .ids(vec![first, namespace.urn("Vehicle", 2)?])
        .entity_type("Vehicle")
        .send()
        .await?;
    let body = json_body(&received)?;
    expect_that!(
        &body,
        matches_document(json!([parked2_forwarded(&namespace, 1), parked2_forwarded(&namespace, 2)])).hiding_everywhere("@context")
    );
    let entities = body.as_array().or_fail()?;
    for n in [1, 2] {
        let id = namespace.urn_text("Vehicle", n);
        let entity = entities
            .iter()
            .find(|entity| entity.get("id").and_then(serde_json::Value::as_str) == Some(id.as_str()));
        expect_that!(entity.and_then(leading_context), some(eq(vocabulary.url().as_str())));
    }
    expect_that!(body.pointer("/0/brandName"), some(not(eq(old_brand_name))));
    expect_that!(stub_count(&source, &rule()).await?, eq(1));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
