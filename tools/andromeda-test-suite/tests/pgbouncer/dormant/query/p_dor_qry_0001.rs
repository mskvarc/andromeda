//! A geo-query sent first into a tenant loaded again after its eviction finds every Entity in range.

use andromeda_test_suite::{
    broker,
    config::BrokerRole,
    eviction::evict_by_pressure,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    tenant_set::TenantSet,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{
        Entity,
        NgsiLdMediaType,
        geometry::{Position, Shape},
    },
    query::geoquery,
};

/// Pins: in a tenant holding Spots at (13.38893, 52.534473), about 220 m from it and about 4.8 km from it, evicted under
/// the pressure of 40 other tenants and seen dormant, Query Entities of the Spot type with near;maxDistance==1000 around
/// (13.38893, 52.534473) sent as the first request into the tenant answers the two nearer Spots, and sent again answers
/// the same two
/// Clause: 4.10, 5.7.2.4
/// Tenant: private (the subject is a whole tenant leaving the runtime cache)
#[gtest]
#[tokio::test]
async fn p_dor_qry_0001_a_geo_query_sent_first_into_a_tenant_loaded_again_finds_every_entity_in_range() -> Result<()> {
    let namespace = Namespace::fresh();
    let dormant = broker::client(BrokerRole::Dormant)?;
    let set = TenantSet::provision(&dormant, &namespace, 1, 1).await?;
    let member = set.member(0);
    let spot_type = namespace.entity_type("Spot")?;
    let centre_id = namespace.urn("Spot", "centre")?;
    let near_id = namespace.urn("Spot", "near")?;
    let reference = Shape::Point(Position::try_new(13.38893, 52.534_473)?);
    let spots = [
        (centre_id.clone(), reference.clone()),
        (near_id.clone(), Shape::Point(Position::try_new(13.392, 52.535)?)),
        (namespace.urn("Spot", "far")?, Shape::Point(Position::try_new(13.46, 52.534_473)?)),
    ];
    for (id, location) in spots {
        let spot = Entity::builder()
            .id(id)
            .entity_type(spot_type.clone())
            .geo_property("location", location)
            .build()?;
        member.client.entities().create(&spot).send().await?.done()?;
    }
    evict_by_pressure(&dormant, &member.tenant).await?;
    let json = member.client.with_accept(Accept::new(NgsiLdMediaType::Json));
    let first = json
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference.clone()))
        .send()
        .await?;
    expect_that!(first.items(), unordered_elements_are![has_id(&centre_id), has_id(&near_id)]);
    let again = json
        .entities()
        .query()
        .entity_type(&spot_type)
        .geo_q(geoquery::near_max(1000, reference))
        .send()
        .await?;
    expect_that!(again.items(), unordered_elements_are![has_id(&centre_id), has_id(&near_id)]);
    Ok(())
}
