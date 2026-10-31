//! An entity with a malformed id that an inclusive registration would match is refused, and reaches neither the broker
//! nor the context source (clauses 4.3.3, 4.3.6.2, 5.6.1 and 6.3.3).

use crate::{
    distributed_operations::{
        forwarding_source::{register, serve, source_endpoint, unregister},
        provision::entities::entity_sources::vehicle_type_registration,
    },
    vehicle_entity_payloads::vehicle_parked,
};
use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::{api_uri, json_body},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{ContextDelivery, UncheckedDocument, headers::Accept, operation::CreateEntity},
    model::Representation,
    query::Locality,
};
use std::time::Duration;
use tokio::time::sleep;

/// How long the case waits for a request that must not reach the source.
const ABSENCE_WINDOW: Duration = Duration::from_secs(5);

/// Pins: Check that if one requests the Context Broker to create an entity that matches an inclusive registration but
/// is malformed, this is created neither on the Context Broker nor on the Context Source
/// Clause: 4.3.3, 4.3.6.2, 5.6.1, 6.3.3
/// Absence: no request within 5 s
/// Tenant: private (a type-only registration matches every request of the tenant that names no entity type, clause
/// 5.12)
#[gtest]
#[tokio::test]
async fn d001_02_inc_request_to_create_an_entity_with_a_malformed_id_on_both_context_broker_and_context_source() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Vehicle", "brandName", "isParked", "providedBy"]).await?;
    let source = ContextSource::refusing()?;
    let _registration = register(
        &broker,
        &vocabulary,
        &vehicle_type_registration(&namespace, 1, &source_endpoint(&source, &namespace, "")?, None)?,
    )
    .await?;
    let source = serve(source).await?;
    let writer = broker.with_context(vocabulary.context())?.with_context_delivery(ContextDelivery::LdJsonBody);
    let entity = vehicle_parked(&namespace, 1)?;
    let document = UncheckedDocument::try_from(entity.render(Representation::Normalized))?.set_text("id", "InvalidUriExample");
    let request = writer
        .unchecked(&CreateEntity::builder().entity(&entity).build())?
        .uri(api_uri(&writer, "entities/")?)
        .document(&document);
    expect_that!(&writer.send_unchecked(request).await, answered(StatusCode::BAD_REQUEST));
    sleep(ABSENCE_WINDOW).await;
    expect_that!(source.received().await?, is_empty());
    let listed = broker
        .with_context(vocabulary.context())?
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type("Vehicle")
        .locality(Locality::Local)
        .send()
        .await;
    verify_that!(&listed, answered(StatusCode::OK))?;
    expect_that!(json_body(&listed?)?.as_array(), some(is_empty()));
    unregister(&broker, &namespace, 1).await
}
