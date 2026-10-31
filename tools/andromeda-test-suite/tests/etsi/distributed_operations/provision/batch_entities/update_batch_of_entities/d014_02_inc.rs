//! A batch update with `noOverwrite` of two entities an inclusive registration matches is sent to the context source
//! with the option and keeps the broker's `isParked` (clauses 4.3.3, 4.3.6.2 and 5.6.9).

use crate::{
    distributed_operations::{
        fork_rule::ForkRule,
        forwarding_source::{register, serve, source_endpoint, source_path, stub, unregister},
        provision::batch_entities::{
            batch_sources::{batch_registration, delete_vehicles},
            update_batch_of_entities::update_request::update_vehicles,
        },
        redirected_request::{forwarded_options, wait_for_redirected},
    },
    entity_operations::create_entity_with_context_string,
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{broker, context_source::ContextSource, namespace::Namespace, vocabulary::Vocabulary, wire::json_body};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{http::operation::Overwrite, model::NgsiLdMediaType};
use serde_json::json;

/// Pins: Check that if one request the Context Broker to update a batch of entities that match an inclusive
/// registration, they are updated on the Context Source too
/// Clause: 4.3.3, 4.3.6.2, 5.6.9
/// Expected: the local isParked follows the overwrite flag (clauses 5.6.9.4, 5.6.3.4)
/// Isolation: the registration's idPattern is `urn:ngsi-ld:Vehicle:<ns>-.*`: a request that names no entity type
/// matches a registration on the id alone (clause 5.12), so a pattern without the namespace would draw every other
/// test's requests on a Vehicle id into this test's source
#[gtest]
#[tokio::test]
async fn d014_02_inc_update_batch_entities_with_the_no_overwrite_option() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy", "speed", "source"]).await?;
    let source = ContextSource::refusing()?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity_with_context_string(&broker, &vocabulary, &vehicle_parked(&namespace, 2)?).await?;
    let _registration = register(
        &broker,
        &vocabulary,
        &batch_registration(&namespace, 1, &source_endpoint(&source, &namespace, "")?, None)?,
    )
    .await?;
    let source = serve(source).await?;
    stub(
        &source,
        ForkRule::resource(Method::POST, &source_path(&namespace, "/ngsi-ld/v1/entityoperations/update")),
        StatusCode::NO_CONTENT,
        None,
    )
    .await?;
    update_vehicles(&broker, &vocabulary, &namespace, Overwrite::Keep).await?;
    let forwarded = wait_for_redirected(&source, 0).await?;
    expect_that!(
        forwarded_options(&forwarded),
        any![elements_are![contains_substring("noOverwrite")], all![len(gt(1)), contains(eq("noOverwrite"))]]
    );
    let received = broker
        .with_context(vocabulary.context())?
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Vehicle", 1)?, namespace.urn("Vehicle", 2)?])
        .entity_type("Vehicle")
        .send()
        .await?;
    let body = json_body(&received)?;
    let parking = json!(namespace.urn_text("OffStreetParking", "Downtown1"));
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/isParked/object"), some(eq(&parking)));
    expect_that!(body.pointer("/1/isParked/object"), some(eq(&parking)));
    delete_vehicles(&broker, &namespace).await?;
    unregister(&broker, &namespace, 1).await
}
