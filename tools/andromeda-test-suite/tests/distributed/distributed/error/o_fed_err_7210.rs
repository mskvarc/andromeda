//! A query repeated within the cache duration is served from the cache with warning 110.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    header_matcher::absent,
    namespace::Namespace,
    problem::answered,
    source_answer,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        headers::{Accept, NgsiLdWarning, WarningCode},
        operation::RegisterContextSource,
    },
    model::RawJson,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Entities twice by type within a `cacheDuration` of PT10S answers the second time with NGSILD-Warning
/// code 110 and the source receives one request
/// Clause: 6.3.17, 5.2.34
/// Recorded: clause 6.3.17 makes caching of forwarded GET responses optional for a broker, so a cache hit and its
/// warning 110 are Andromeda behaviour
#[gtest]
#[tokio::test]
async fn o_fed_err_7210_a_query_repeated_within_the_cache_duration_is_served_from_the_cache_with_warning_110() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .property_names(["speed"])
            .build()?])
        .operations([DistributedOperation::QueryEntity])
        .build()?;
    // The model refuses it (a registration management), so `management` is written as text.
    let management: RawJson = r#"{"cacheDuration": "PT10S"}"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("management", &management);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_entities = [];
    source
        .answer(Method::GET, "/a/ngsi-ld/v1/entities", source_answer::entities(&source_entities))
        .await?;
    let page = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(page.warnings(), absent());
    let page_2 = broker.with_accept(Accept::any()).entities().query().entity_type(&vehicle_type).send().await?;
    expect_that!(
        page_2.warnings().valid().and_then(|warnings| warnings.iter().next()).map(NgsiLdWarning::code),
        some(eq(WarningCode::ResponseIsStale))
    );
    let forwarded = source.received_on(Method::GET, "/a/ngsi-ld/v1/entities").await?;
    expect_that!(forwarded, len(eq(1)));
    Ok(())
}
