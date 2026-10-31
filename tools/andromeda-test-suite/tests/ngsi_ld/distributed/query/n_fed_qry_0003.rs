//! A csf path forwards a query by a compound property written key.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::RegisterContextSource},
    model::{Entity, RawJson},
    query::q,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities with csf coverage[region]=="north" over two `queryEntity` registrations whose coverage values
/// hold region north and south is forwarded once to the north source and never to the south source, and answers the
/// Entity the north source returns
/// Clause: 4.9, 5.7.2, 6.4.3.2, 5.12
#[gtest]
#[tokio::test]
async fn n_fed_qry_0003_a_csf_path_forwards_a_query_by_a_compound_property_written_key() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let device_type = namespace.entity_type("Device")?;
    let device_north_id = namespace.urn("Device", "north")?;
    let device_south_id = namespace.urn("Device", "south")?;
    let source_entities = [Entity::output_builder().id(device_north_id.clone()).entity_type(device_type.clone()).build()?];
    source
        .answer(Method::GET, "/north/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let source_entities_2 = [Entity::output_builder().id(device_south_id).entity_type(device_type.clone()).build()?];
    source
        .answer(Method::GET, "/south/ngsi-ld/v1/entities", source_answer::entities(&source_entities_2))
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("north")?)
        .id(namespace.registration_id("north")?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration property), so `coverage` is written as text.
    let property_north: RawJson = r#"{"type": "Property", "value": {"region": "north"}}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("coverage", &property_north);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let registration_2 = CSourceRegistration::builder(source.endpoint("south")?)
        .id(namespace.registration_id("south")?)
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(device_type.clone()).with_id_pattern(namespace.id_pattern("Device")?)])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration property), so `coverage` is written as text.
    let property_south: RawJson = r#"{"type": "Property", "value": {"region": "south"}}"#.parse()?;
    let document_2 = UncheckedDocument::try_from(registration_2.render())?.set_member("coverage", &property_south);
    let request_2 = broker
        .unchecked(&RegisterContextSource { registration: &registration_2 })?
        .document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&device_type)
        .csf(q::attr("coverage").member("region").eq("north"))
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&device_north_id)));
    let forwarded = SourceRequest::new(Method::GET, "/north/ngsi-ld/v1/entities");
    expect_that!(source.received_matching(&forwarded).await?, len(eq(1)));
    let forwarded_2 = SourceRequest::new(Method::GET, "/south/ngsi-ld/v1/entities");
    expect_that!(source.received_matching(&forwarded_2).await?, len(eq(0)));
    Ok(())
}
