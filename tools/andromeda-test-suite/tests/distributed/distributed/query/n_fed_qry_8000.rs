//! A registration scope updated away from the scope query takes its source out of the query.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    config::{BrokerRole, PeerRole},
    entity_matcher::has_attribute,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    problem::answered,
    written_json::json_request_to,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, OneOrMany, Scope},
    query::scope_query::ScopeQuery,
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Query Entities on broker A by type and `scopeQ` /Europe/Madrid/# over an inclusive registration of broker
/// B scoped /Europe/Madrid answers the Entity B holds in that scope; once the registration scope is updated to
/// /Europe/Berlin the registration reads back with it and the same query answers nothing
/// Clause: 5.7.2.4, 5.10.2.4, 5.9.3, 4.19
#[gtest]
#[tokio::test]
async fn n_fed_qry_8000_a_registration_scope_updated_away_from_the_scope_query_takes_its_source_out_of_the_query() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let building_id = namespace.urn("Building", 1)?;
    let building_type = namespace.entity_type("Building")?;
    let registration_id = namespace.registration_id(1)?;
    let building = Entity::builder()
        .id(building_id.clone())
        .entity_type(building_type.clone())
        .property("speed", 10)
        .scope(["/Europe/Madrid"])
        .build()?;
    broker_b.entities().create(&building).send().await?.done()?;
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(building_type.clone()).with_id_pattern(namespace.id_pattern("Building")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryEntity])
        .scope(OneOrMany::try_from(vec!["/Europe/Madrid".parse::<Scope>()?])?)
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let page = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/Europe/Madrid/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), nth(0, has_id(&building_id)));
    expect_that!(page.items(), nth(0, has_attribute("speed", any_attribute().value(10))));
    // The registration fragment always writes a `type`, so the fragment with the scope alone is sent as written.
    let update = json_request_to(
        &broker,
        Method::PATCH,
        &format!("csourceRegistrations/{registration_id}"),
        "{\n    \"scope\": [\n        \"/Europe/Berlin\"\n    ]\n}",
    )?;
    let answer_2 = broker.send_unchecked(update).await;
    expect_that!(&answer_2, answered(StatusCode::NO_CONTENT));
    let retrieved = broker.with_accept(Accept::any()).registrations().retrieve(&registration_id).send().await?;
    expect_that!(
        retrieved.scope().map(|scopes| scopes.iter().map(ToString::to_string).collect::<Vec<_>>()),
        some(contains(eq("/Europe/Berlin")))
    );
    let page_2 = broker
        .with_accept(Accept::any())
        .entities()
        .query()
        .entity_type(&building_type)
        .scope_q("/Europe/Madrid/#".parse::<ScopeQuery>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), is_empty());
    Ok(())
}
