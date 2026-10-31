//! A clone of a preparing snapshot waits and carries its data.

use andromeda_test_suite::{
    broker,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Done,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, NgsiLdMediaType},
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotClone, SnapshotId, SnapshotStatus},
};
use std::time::Duration;
use wiremock::{Mock, ResponseTemplate, matchers::path_regex};

/// Pins: Clone Snapshot of a Snapshot still preparing (its capture waits three seconds for a context source)
/// answers 201, the clone reads preparing until its source has ended, then ends success and holds the Entity its
/// source captured
/// Clause: 5.16.2.4, 5.16.1.4
#[gtest]
#[tokio::test]
async fn n_snp_cln_8103_a_clone_of_a_preparing_snapshot_waits_and_carries_its_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let slow_type = namespace.entity_type("Slow")?;
    let slow_id = namespace.urn("Slow", 1)?;
    let registration_id = namespace.registration_id(1)?;
    let first_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let second_id = format!("urn:ngsi-ld:Snapshot:{namespace}-2").parse::<SnapshotId>()?;
    let slow = Entity::builder()
        .id(slow_id.clone())
        .entity_type(slow_type.clone())
        .property("speed", 7)
        .build()?;
    broker.entities().create(&slow).send().await?.done()?;
    source
        .mount(
            Mock::given(path_regex("^/slow/.*")).respond_with(
                ResponseTemplate::new(200)
                    .set_body_raw("[]", "application/json")
                    .set_delay(Duration::from_secs(3)),
            ),
        )
        .await?;
    let registration = CSourceRegistration::builder(source.endpoint("slow")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(slow_type.clone()).with_id_pattern(namespace.id_pattern("Slow")?)])
            .property_names([namespace.attribute("speed")?.to_string()])
            .build()?])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    let snapshot = Snapshot::builder()
        .id(first_id.clone())
        .query(QueryDocument::builder().entities([slow_type.as_str()]).build()?)
        .build()?;
    broker.snapshots().create(&snapshot).send().await?;
    let preparing = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&first_id)
        .send()
        .await?;
    expect_that!(preparing.status(), eq("preparing".parse::<SnapshotStatus>()?));
    broker
        .snapshots()
        .clone_snapshot(&first_id, &SnapshotClone::builder().id(second_id.clone()).build()?)
        .send()
        .await?;
    let clone_preparing = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&second_id)
        .send()
        .await?;
    expect_that!(clone_preparing.status(), eq("preparing".parse::<SnapshotStatus>()?));
    await_status(&broker, &first_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    await_status(&broker, &second_id, SnapshotStatus::Settled(SettledStatus::Success)).await?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .on_snapshot(&second_id)
        .entities()
        .query()
        .entity_type(&slow_type)
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(page.items(), contains(has_id(&slow_id)));
    broker.snapshots().delete(&second_id).send().await?;
    broker.snapshots().delete(&first_id).send().await?;
    broker.registrations().delete(&registration_id).send().await?;
    Ok(())
}
