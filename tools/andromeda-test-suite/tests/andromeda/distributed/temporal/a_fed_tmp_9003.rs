//! Last then prev pages back through the union.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    paged_federation::{local_evolutions, serve_temporal_pages},
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    query::{IdPattern, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Temporal Evolution of Entities at limit 1,000 over an inclusive registration whose source holds 2,500
/// Temporal Evolutions answered in pages of 1,000 linked by next, beside 14 the broker holds itself (4 interleaving
/// the source's identifiers, 10 shared with it) answers a last pointer to the final 1,000 identifiers, from which
/// prev pointers walk back through the identifier order, the short first page of 504 offering no prev
/// Clause: 5.7.4.4, 5.5.9.1, 6.3.10
#[gtest]
#[tokio::test]
async fn a_fed_tmp_9003_last_then_prev_pages_back_through_the_union() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let paged_temporal_type = namespace.entity_type("PagedTemporal")?;
    let registration_id = namespace.registration_id(1)?;
    serve_temporal_pages(&source, &namespace).await?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(paged_temporal_type.clone()).with_id_pattern(format!("urn:ngsi-ld:.+:{namespace}-.*").parse::<IdPattern>()?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    for evolution in &local_evolutions(&namespace)? {
        broker.temporal().create_or_update(evolution).send().await?.created()?;
    }
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .temporal()
        .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
        .entity_type(&paged_temporal_type)
        .limit(1000)
        .send()
        .await?;
    let id = |suffix: &str| namespace.urn("PagedTemporal", suffix);
    let last_page = json.send(&first.last_page().or_fail()?).await?;
    expect_that!(last_page.items(), len(eq(1000)));
    expect_that!(last_page.items(), nth(0, has_id(&id("01500L")?)));
    expect_that!(last_page.items(), nth(500, has_id(&id("02000")?)));
    expect_that!(last_page.items(), nth(999, has_id(&id("02499")?)));
    let prev_page = json.send(&last_page.prev_page().or_fail()?).await?;
    expect_that!(prev_page.items(), len(eq(1000)));
    expect_that!(prev_page.items(), nth(0, has_id(&id("00503")?)));
    expect_that!(prev_page.items(), nth(500, has_id(&id("01001")?)));
    expect_that!(prev_page.items(), nth(999, has_id(&id("01500")?)));
    let prev_page_2 = json.send(&prev_page.prev_page().or_fail()?).await?;
    expect_that!(prev_page_2.items(), len(eq(504)));
    expect_that!(prev_page_2.items(), nth(0, has_id(&id("00000")?)));
    expect_that!(prev_page_2.items(), nth(252, has_id(&id("00251")?)));
    expect_that!(prev_page_2.items(), nth(503, has_id(&id("00502")?)));
    expect_that!(prev_page_2.prev(), none());
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
