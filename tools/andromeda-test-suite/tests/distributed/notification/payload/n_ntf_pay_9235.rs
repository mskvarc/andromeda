//! A relayed instance deletion of each attribute family keeps its object form and dataset id.

use andromeda_test_suite::{
    body_json::{json_count, json_equals},
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

/// Pins: A relayed entity whose owner, location, name, category, config, readings and rooms are each an instance of
/// the dataset gps deleted in the object form is notified under a normalized subscription with each member as the
/// object it was notified as, its family's NGSI-LD Null in its value member and its datasetId kept
/// Clause: 5.8.6, 4.5.0, 4.5.18.2, 4.5.21.2, 4.5.22.2
#[gtest]
#[tokio::test]
async fn n_ntf_pay_9235_a_relayed_instance_deletion_of_each_family_keeps_its_object_form_and_dataset_id() -> Result<()> {
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
                .format(Representation::Normalized)
                .build()?,
        )
        .build()?;
    broker.subscriptions().create(&subscription).send().await?;
    let copy_id = placed_copy_id(&source, "s1", &vehicle_type).await?;
    let dataset_id = namespace.urn_text("Dataset", "gps");
    // The entity model writes no NGSI-LD Null as a value in any family, so the data is written as text.
    let data = format!(
        concat!(
            r#"[{{"id":"{deletion_id}","type":"{vehicle_type}","#,
            r#""owner":{{"type":"Relationship","object":"urn:ngsi-ld:null","datasetId":"{dataset_id}"}},"#,
            r#""location":{{"type":"GeoProperty","value":"urn:ngsi-ld:null","datasetId":"{dataset_id}"}},"#,
            r#""name":{{"type":"LanguageProperty","languageMap":{{"@none":"urn:ngsi-ld:null"}},"datasetId":"{dataset_id}"}},"#,
            r#""category":{{"type":"VocabProperty","vocab":"urn:ngsi-ld:null","datasetId":"{dataset_id}"}},"#,
            r#""config":{{"type":"JsonProperty","json":"urn:ngsi-ld:null","datasetId":"{dataset_id}"}},"#,
            r#""readings":{{"type":"ListProperty","valueList":["urn:ngsi-ld:null"],"datasetId":"{dataset_id}"}},"#,
            r#""rooms":{{"type":"ListRelationship","objectList":["urn:ngsi-ld:null"],"datasetId":"{dataset_id}"}},"#,
            r#""speed":{{"type":"Property","value":40}}}}]"#
        ),
        deletion_id = deletion_id,
        vehicle_type = vehicle_type,
        dataset_id = dataset_id
    );
    let body = notification_text(&namespace, &copy_id, &data)?;
    let answer = relay(&broker, &APPLICATION_LD_JSON, &body).await?;
    expect_that!(&answer, answered(StatusCode::OK));
    let notifications = sink.wait_for_at_least(1, 1).await?;
    expect_that!(notifications, len(eq(1)));
    let received = notifications.first().or_fail()?;
    // A bare NGSI-LD Null in every family is no value the entity model reads, so the notification is read as received.
    let body = &received.body;
    expect_that!(body, json_equals("$.data[0].owner.type", "Relationship"));
    expect_that!(body, json_equals("$.data[0].owner.object", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].owner.datasetId", dataset_id.as_str()));
    expect_that!(body, json_equals("$.data[0].location.type", "GeoProperty"));
    expect_that!(body, json_equals("$.data[0].location.value", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].location.datasetId", dataset_id.as_str()));
    expect_that!(body, json_equals("$.data[0].name.type", "LanguageProperty"));
    expect_that!(body, json_equals("$.data[0].name.languageMap['@none']", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].name.datasetId", dataset_id.as_str()));
    expect_that!(body, json_equals("$.data[0].category.type", "VocabProperty"));
    expect_that!(body, json_equals("$.data[0].category.vocab", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].config.type", "JsonProperty"));
    expect_that!(body, json_equals("$.data[0].config.json", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].readings.type", "ListProperty"));
    expect_that!(body, json_count("$.data[0].readings.valueList", 1));
    expect_that!(body, json_equals("$.data[0].readings.valueList[0]", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].rooms.type", "ListRelationship"));
    expect_that!(body, json_count("$.data[0].rooms.objectList", 1));
    expect_that!(body, json_equals("$.data[0].rooms.objectList[0]", "urn:ngsi-ld:null"));
    expect_that!(body, json_equals("$.data[0].rooms.datasetId", dataset_id.as_str()));
    expect_that!(body, json_equals("$.data[0].speed.value", 40));
    Ok(())
}
