//! Attribute types newer than the source version are reformatted.

use andromeda_test_suite::{
    body_pattern::BodyPattern,
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    namespace::Namespace,
    problem::answered,
    source_answer,
    source_request::SourceRequest,
};
use googletest::prelude::*;
use http::{Method, StatusCode};
use ngsi_ld::{
    http::{
        UncheckedDocument,
        operation::{CreateEntity, RegisterContextSource},
    },
    model::{Entity, JsonProperty, LanguageMap, LanguageProperty, ListProperty, Lossless, RawJson, Value, VocabProperty, VocabTerm},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo, RegistrationMode},
};

/// Pins: Create Entity with a `LanguageProperty`, `JsonProperty`, `VocabProperty`, `ListProperty` and
/// `ListRelationship`, under an inclusive registration declaring `ngsildConformance` 1.0, is forwarded once with
/// the first four as Properties and the last as a Relationship
/// Clause: 4.3.6.8, 4.3.6.6
#[gtest]
#[tokio::test]
async fn n_fed_wrt_6406_attribute_types_newer_than_the_source_version_are_reformatted() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let registration = CSourceRegistration::builder(source.root_endpoint()?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .mode(RegistrationMode::Inclusive)
        .operations([DistributedOperation::CreateEntity])
        .build()?;
    // The model refuses it (a registration contextSourceInfo), so `contextSourceInfo` is written as text.
    let context_source_info: RawJson = r#"[{"key": "ngsildConformance", "value": "1.0"}]"#.parse()?;
    let document = UncheckedDocument::try_from(registration.render())?.set_member("contextSourceInfo", &context_source_info);
    let request = broker.unchecked(&RegisterContextSource { registration: &registration })?.document(&document);
    let answer = broker.send_unchecked(request).await;
    expect_that!(&answer, answered(StatusCode::CREATED));
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    source
        .answer(Method::POST, "/ngsi-ld/v1/entities", source_answer::status(StatusCode::CREATED))
        .await?;
    let vehicle = Entity::builder()
        .id(vehicle_id)
        .entity_type(vehicle_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "car"), ("es", "coche")])?),
        )
        .json_property("payload", JsonProperty::builder(r#"{"k":1}"#.parse::<RawJson>()?))
        .vocab_property("category", VocabProperty::builder("sedan".parse::<VocabTerm>()?))
        .list_property("readings", ListProperty::builder(vec![Value::from(1), Value::from(2), Value::from(3)]))
        .build()?;
    // The model refuses it (the model reads it as written otherwise [at $.partners.objectList[0] (written "urn:ngsi-ld:Partner:zqns-1")]: {"id":"urn:ngsi-ld:Vehicle:zqns-1","type":"Vehicle_zqns","label":{"type":"LanguageProperty","languageMap":{"en":"car","es":"coche"}},"payload":{"type":"JsonProperty","json":{"k":1}},"category":{"type":"VocabProperty","vocab":"sedan"},"readings":{"type":"ListProperty","valueList":[1,2,3]},"partners":{"type":"ListRelationship","objectList":[{"object":"urn:ngsi-ld:Partner:zqns-1"},{"object":"urn:ngsi-ld:Partner:zqns-2"}]}}), so `partners` is written as text.
    let partners: RawJson =
        format!(r#"{{"type": "ListRelationship", "objectList": ["urn:ngsi-ld:Partner:{namespace}-1", "urn:ngsi-ld:Partner:{namespace}-2"]}}"#).parse()?;
    let document_2 = UncheckedDocument::from_entity(&vehicle, Lossless::Normalized).set_member("partners", &partners);
    let request_2 = broker.unchecked(&CreateEntity::builder().entity(&vehicle).build())?.document(&document_2);
    let answer_2 = broker.send_unchecked(request_2).await;
    expect_that!(&answer_2, answered(StatusCode::CREATED));
    let forwarded_request = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?));
    let forwarded = source.received_matching(&forwarded_request).await?;
    expect_that!(forwarded, len(eq(1)));
    let forwarded_request_2 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::SelectsBelow("$.label".parse()?, "$[?(@.type == 'Property')].value".parse()?));
    let forwarded_2 = source.received_matching(&forwarded_request_2).await?;
    expect_that!(forwarded_2, len(eq(1)));
    let forwarded_request_3 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::SelectsBelow("$.payload".parse()?, "$[?(@.type == 'Property')].value".parse()?));
    let forwarded_3 = source.received_matching(&forwarded_request_3).await?;
    expect_that!(forwarded_3, len(eq(1)));
    let forwarded_request_4 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::SelectsBelow("$.category".parse()?, "$[?(@.type == 'Property')].value".parse()?));
    let forwarded_4 = source.received_matching(&forwarded_request_4).await?;
    expect_that!(forwarded_4, len(eq(1)));
    let forwarded_request_5 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::SelectsBelow("$.readings".parse()?, "$[?(@.type == 'Property')].value".parse()?));
    let forwarded_5 = source.received_matching(&forwarded_request_5).await?;
    expect_that!(forwarded_5, len(eq(1)));
    let forwarded_request_6 = SourceRequest::new(Method::POST, "/ngsi-ld/v1/entities")
        .body(BodyPattern::Selects(format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')]").parse()?))
        .body(BodyPattern::SelectsBelow(
            "$.partners".parse()?,
            "$[?(@.type == 'Relationship')].object".parse()?,
        ))
        // Table 4.3.6.8-3 types the `object` of a Relationship as String or String[], so every target is forwarded.
        .body(BodyPattern::Selects(
            format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')].partners.object[?(@ == 'urn:ngsi-ld:Partner:{namespace}-1')]").parse()?,
        ))
        .body(BodyPattern::Selects(
            format!("$[?(@.id == 'urn:ngsi-ld:Vehicle:{namespace}-1')].partners.object[?(@ == 'urn:ngsi-ld:Partner:{namespace}-2')]").parse()?,
        ));
    let forwarded_6 = source.received_matching(&forwarded_request_6).await?;
    expect_that!(forwarded_6, len(eq(1)));
    Ok(())
}
