//! A post temporal query honours its context source filter.

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
    http::{
        UncheckedDocument,
        headers::Accept,
        operation::{QueryMethod, RegisterContextSource},
    },
    model::{NgsiLdMediaType, Property, RawJson, Value, evolution::TemporalEntity},
    query::{IdPattern, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: A POST temporal query whose csf is `sourceGroup=="beta"` contacts no source and answers nothing, and with
/// `sourceGroup=="alpha"` contacts the registered source once and answers its Entity
/// Clause: 4.9, Table 5.2.23-1, 5.7.4.3
#[gtest]
#[tokio::test]
async fn n_fed_tmp_9010_a_post_temporal_query_honours_its_context_source_filter() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let csf_temporal_vehicle_remote_id = namespace.urn("CsfTemporalVehicle", "remote")?;
    let csf_temporal_vehicle_type = namespace.entity_type("CsfTemporalVehicle")?;
    let registration_id = namespace.registration_id("csf")?;
    let source_evolutions = [TemporalEntity::builder()
        .id(csf_temporal_vehicle_remote_id.clone())
        .entity_type(csf_temporal_vehicle_type.clone())
        .property(
            "speed",
            Property::builder(Value::Number("42.0".parse()?)).observed_at("2024-01-01T00:00:00Z".parse()?),
        )
        .build()?];
    source
        .answer(
            Method::GET,
            "/csf/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("csf")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(csf_temporal_vehicle_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    // The model refuses it (a registration property), so `sourceGroup` is written as text.
    let source_group: RawJson = r#"{"type": "Property", "value": "alpha"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("sourceGroup", &source_group);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&csf_temporal_vehicle_type)
        .csf(q::attr("sourceGroup").eq("beta"))
        .method(QueryMethod::Post)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(0)));
    let forwarded_request = SourceRequest::new(Method::GET, "/csf/ngsi-ld/v1/temporal/entities");
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(0)));
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::JsonLd))
        .temporal()
        .query(temporal_query::before("2099-01-01T00:00:00Z".parse()?))
        .entity_type(&csf_temporal_vehicle_type)
        .csf(q::attr("sourceGroup").eq("alpha"))
        .method(QueryMethod::Post)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(page_2.items(), nth(0, has_id(&csf_temporal_vehicle_remote_id)));
    expect_that!(source.received_matching(&forwarded_request).await?, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
