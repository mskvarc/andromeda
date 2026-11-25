//! A split temporal capture decides a scope query on the history assembled from its parts.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker::{self, BrokerClient},
    cleanup::Cleanup,
    config::BrokerRole,
    evolution_matcher::has_evolution,
    identity_matcher::has_id,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    peer_role::PeerRole,
    problem::answered,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{UncheckedDocument, headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{DateTime, EntityId, EntityTypeName, Lossless, Property, RawJson, evolution::TemporalEntity},
    query::{
        EntitySplitting,
        scope_query::ScopeQuery,
        temporal_query::{self, TemporalQuery},
    },
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
};

/// The instant every instance of the test is observed at.
const OBSERVED: &str = "2020-08-01T12:00:00Z";

/// Record on `client` the Temporal Evolution of `id` holding a `speed` of 50 and the Scope `scope`, both observed at
/// [`OBSERVED`].
async fn record_scoped(client: &BrokerClient, id: &EntityId, vehicle_type: &EntityTypeName, scope: &str) -> Result<()> {
    let evolution = TemporalEntity::builder()
        .id(id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(50).observed_at(OBSERVED.parse()?))
        .build()?;
    // The model refuses it (a temporal scope), so `scope` is written as text.
    let scope: RawJson = format!(r#"[{{"type": "Property", "value": "{scope}", "observedAt": "{OBSERVED}"}}]"#).parse()?;
    let document = UncheckedDocument::try_from(evolution.render(Lossless::Normalized))?.set_member("scope", &scope);
    let request = client
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&evolution).build())?
        .document(&document);
    let answer = client.send_unchecked(request).await;
    verify_that!(&answer, answered(StatusCode::CREATED))
}

/// Pins: A Snapshot whose temporal query after 2020-01-01 by type has `splitEntities` true and `scopeQ` /Madrid, over
/// an inclusive `queryTemporal` registration of peer broker B, captures the history of the Entity whose local part
/// holds only brand and whose scope /Madrid B holds, merged, the history B alone holds in /Madrid and the local one in
/// /Madrid, and none scoped /Paris
/// Clause: 5.16.1.4, 5.7.4.4, 4.18, 4.19
#[gtest]
#[tokio::test]
async fn n_snp_cre_8190_a_split_temporal_capture_decides_a_scope_query_on_the_history_assembled_from_its_parts() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let broker_b = broker::client(BrokerRole::B)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let vehicle_4_id = namespace.urn("Vehicle", 4)?;
    let vehicle_5_id = namespace.urn("Vehicle", 5)?;
    let vehicle_6_id = namespace.urn("Vehicle", 6)?;
    let registration_id = namespace.registration_id(1)?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let since = "2020-01-01T00:00:00Z".parse::<DateTime>()?;
    for (id, brand) in [(&vehicle_id, "Mercedes"), (&vehicle_2_id, "Volvo")] {
        let part = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(vehicle_type.clone())
            .property("brand", Property::builder(brand).observed_at(OBSERVED.parse()?))
            .build()?;
        broker.temporal().create_or_update(&part).send().await?.created()?;
    }
    for (id, scope) in [(&vehicle_5_id, "/Madrid"), (&vehicle_6_id, "/Paris")] {
        record_scoped(&broker, id, &vehicle_type, scope).await?;
    }
    for (id, scope) in [
        (&vehicle_id, "/Madrid"),
        (&vehicle_2_id, "/Paris"),
        (&vehicle_3_id, "/Madrid"),
        (&vehicle_4_id, "/Paris"),
    ] {
        record_scoped(&broker_b, id, &vehicle_type, scope).await?;
    }
    let registration = CSourceRegistration::builder(broker::peer(PeerRole::B)?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .temporal_query(
            QueryDocument::temporal_builder(TemporalQuery::after(since))
                .entities([vehicle_type.as_str()])
                .scope_q("/Madrid".parse::<ScopeQuery>()?)
                .split_entities(EntitySplitting::Split)
                .build()?,
        )
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    await_status(&broker, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let page = broker
        .with_accept(Accept::any())
        .on_snapshot(&snapshot_id)
        .temporal()
        .query(temporal_query::after(since))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    expect_that!(
        page.items(),
        unordered_elements_are![
            all!(
                has_id(&vehicle_id),
                has_evolution("speed", all!(len(eq(1)), nth(0, any_attribute().value(50)))),
                has_evolution("brand", all!(len(eq(1)), nth(0, any_attribute().value("Mercedes"))))
            ),
            has_id(&vehicle_3_id),
            has_id(&vehicle_5_id)
        ]
    );
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
