//! A relayed instance deletion of a dataset keeps its dataset id in the simplified format.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    context_source::ContextSource,
    namespace::Namespace,
    notification_sink::NotificationSink,
    problem::answered,
    relayed_copy::{notification_text, placed_copy_id, register_copy_source, relay},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    model::{APPLICATION_LD_JSON, NgsiLdMediaType, Representation},
    subscription::{Endpoint, NotificationParams, Subscription},
};

/// Pins: A relayed entity whose owner is a Property instance of the dataset gps deleted in the object form is
/// notified under a simplified subscription with owner the dataset map holding urn:ngsi-ld:null under gps, not the
/// bare urn:ngsi-ld:null that reports the whole attribute deleted
/// Clause: 5.8.6, 4.5.4
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9236_a_relayed_instance_deletion_of_a_dataset_keeps_its_dataset_id_in_the_simplified_format() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let sink = NotificationSink::start().await?;
    let source = ContextSource::start().await?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let deletion_id = namespace.urn("Vehicle", "deletion")?;
    register_copy_source(&broker, &source, &namespace, "s1").await?;
    let subscription = Subscription::builder()
        .id(namespace.subscription_id(1)?)
        .entities([vehicle_type.as_str()])
        .notification(
            NotificationParams::builder(Endpoint::builder(sink.endpoint(1)?).accept(NgsiLdMediaType::Json).build()?)
                .format(Representation::Simplified)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let dataset_id = namespace.urn_text("Dataset", "gps");
    // The entity model writes no NGSI-LD Null as a value, so the data is written as text.
    let data = format!(
        r#"[{{"id":"{deletion_id}","type":"{vehicle_type}","owner":{{"type":"Property","value":"urn:ngsi-ld:null","datasetId":"{dataset_id}"}},"speed":{{"type":"Property","value":40}}}}]"#
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    // A dataset map holding the NGSI-LD Null is no simplified value the entity model reads, so the notification is read as received.
    expect_that!(
        &received.body,
        json_equals(&format!("$.data[0].owner.dataset['{dataset_id}']"), "urn:ngsi-ld:null")
    );
    expect_that!(&received.body, json_absent("$.data[0].owner.dataset['@none']"));
    expect_that!(&received.body, json_equals("$.data[0].speed", 40));
    Ok(())
}
