//! Create snapshot answers before its capture ends.

use andromeda_test_suite::{broker, context_source::ContextSource, eventually::retried, namespace::Namespace};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::NgsiLdMediaType,
    registration::{CSourceRegistration, EntityInfo, RegistrationInfo},
    snapshot::{QueryDocument, Snapshot, SnapshotId, SnapshotStatus},
};
use std::time::{Duration, Instant};
use wiremock::{Mock, ResponseTemplate, matchers::path_regex};

/// Pins: Create Snapshot over a type whose context source answers after three seconds answers 201 before the source
/// has answered, the Snapshot reads preparing, and it ends empty once the capture has asked the source once
/// Clause: 5.16.1.4
/// Tenant: private (it counts every request its context source receives, and the discovery and unscoped requests of
/// other tests reach every registration of the tenant)
#[gtest]
#[tokio::test]
async fn n_snp_cre_8126_create_snapshot_answers_before_its_capture_ends() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::private_tenant(&broker::front()?, &namespace).await?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let slow_type = namespace.entity_type("Slow")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
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
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([slow_type.as_str()]).build()?)
        .build()?;
    let started = Instant::now();
    broker.snapshots().create(&snapshot).send().await?;
    expect_that!(started.elapsed(), lt(Duration::from_secs(2)));
    let preparing = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .snapshots()
        .retrieve(&snapshot_id)
        .send()
        .await?;
    expect_that!(preparing.status(), eq("preparing".parse::<SnapshotStatus>()?));
    retried(async || {
        let retrieved = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .snapshots()
            .retrieve(&snapshot_id)
            .send()
            .await?;
        verify_that!(retrieved.status(), eq("empty".parse::<SnapshotStatus>()?))?;
        Ok(())
    })
    .await?;
    expect_that!(source.received().await?, len(eq(1)));
    broker.registrations().delete(&registration_id).send().await?;
    broker.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
