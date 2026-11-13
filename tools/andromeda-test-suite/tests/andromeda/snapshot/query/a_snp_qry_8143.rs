//! A Snapshot-scoped Subscription is still notified after its tenant is suspended and resumed.

use andromeda_test_suite::{
    broker,
    header_condition::HeaderCondition,
    identity_matcher::has_id,
    namespace::Namespace,
    notification_sink::NotificationSink,
    raw_call::andromeda_call,
    snapshot_capture::await_status,
};
use googletest::prelude::*;
use http::{HeaderName, Method, StatusCode};
use ngsi_ld::{
    model::{Entity, Lossless, NgsiLdMediaType, Representation},
    snapshot::{QueryDocument, SettledStatus, Snapshot, SnapshotId, SnapshotStatus},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: a Subscription created with NGSILD-Snapshot in a tenant that is then suspended and resumed, each answering
/// 204, is notified exactly once of an Entity created under that Snapshot afterwards, with the NGSILD-Tenant and
/// NGSILD-Snapshot headers naming its tenant and Snapshot
/// Clause: 5.8.6, 6.3.22, 5.5.10
/// Tenant: private (the subject is a whole tenant's runtime, released by the suspension and built again on resume)
/// Recorded: Andromeda extension, not v1.9.1: tenant management (the /tenants resources), which clause 5.5.10 leaves
/// implementation-specific
#[gtest]
#[tokio::test]
async fn a_snp_qry_8143_a_snapshot_scoped_subscription_is_still_notified_after_its_tenant_is_suspended_and_resumed() -> Result<()> {
    let namespace = Namespace::fresh();
    let front = broker::front()?;
    let private = broker::private_tenant(&front, &namespace).await?;
    let sink = NotificationSink::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let snapshot_id = format!("urn:ngsi-ld:Snapshot:{namespace}-1").parse::<SnapshotId>()?;
    let snapshot = Snapshot::builder()
        .id(snapshot_id.clone())
        .query(QueryDocument::builder().entities([vehicle_type.as_str()]).build()?)
        .build()?;
    private.snapshots().create(&snapshot).send().await?;
    await_status(&private, &snapshot_id, SnapshotStatus::Settled(SettledStatus::Empty)).await?;
    let subscription_id = namespace.subscription_id("scoped")?;
    let subscription = Subscription::builder()
        .id(subscription_id.clone())
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint("scoped")?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    private.on_snapshot(&snapshot_id).subscriptions().create(&subscription).send().await?;
    // The Tenant resource is an Andromeda extension no NGSI-LD operation covers, so its requests are sent as written.
    let tenant_path = format!("tenants/{namespace}");
    for active in ["false", "true"] {
        let body = format!(r#"{{"isActive":{active}}}"#);
        let answer = andromeda_call(
            &front,
            Method::PATCH,
            &tenant_path,
            &[("Content-Type", "application/json")],
            Some(body.as_str()),
        )
        .await?;
        expect_that!(answer.status(), eq(StatusCode::NO_CONTENT));
    }
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property("speed", 1)
        .build()?;
    private.on_snapshot(&snapshot_id).entities().create(&vehicle).send().await?;
    let fields = [
        HeaderCondition::Is(HeaderName::from_static("ngsild-tenant"), namespace.token().to_owned()),
        HeaderCondition::Is(HeaderName::from_static("ngsild-snapshot"), snapshot_id.to_string()),
    ];
    sink.wait_for_at_least_matching("scoped", &[], &fields, 1).await?;
    let received = sink.received("scoped").await?;
    expect_that!(received, len(eq(1)));
    let notification = received.first().or_fail()?.entities(Lossless::Normalized)?;
    expect_that!(notification.subscription_id(), eq(&subscription_id));
    expect_that!(notification.data().as_slice(), elements_are![has_id(&vehicle_id)]);
    private.on_snapshot(&snapshot_id).subscriptions().delete(&subscription_id).send().await?;
    private.snapshots().delete(&snapshot_id).send().await?;
    Ok(())
}
