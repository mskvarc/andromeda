//! An existence filter holds for every attribute family a source answers.

use andromeda_test_suite::{
    broker,
    cleanup::Cleanup,
    context_source::ContextSource,
    identity_matcher::has_id,
    namespace::Namespace,
    outcome::Upserted,
    source_answer,
};
use googletest::prelude::*;
use http::Method;
use ngsi_ld::{
    http::{headers::Accept, operation::RepresentationSpelling},
    model::{JsonProperty, LanguageMap, LanguageProperty, Property, RawJson, VocabProperty, VocabTerm, evolution::TemporalEntity},
    query::{EntitySplitting, q, temporal_query},
    registration::{CSourceRegistration, DistributedOperation, EntityInfo, RegistrationInfo},
};

/// Pins: Query Temporal Evolution of Entities with `splitEntities` true and q says|settings|gender keeps the three
/// Entities whose source parts hold a `LanguageProperty`, a `JsonProperty` and a `VocabProperty` respectively
/// Clause: 5.7.4.4, 4.9, 4.5.7
#[gtest]
#[tokio::test]
async fn n_fed_tmp_7427_an_existence_filter_holds_for_every_attribute_family_a_source_answers() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let source = ContextSource::start().await?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_2_id = namespace.urn("Vehicle", 2)?;
    let vehicle_3_id = namespace.urn("Vehicle", 3)?;
    let registration_id = namespace.registration_id(1)?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_2_evolution = TemporalEntity::builder()
        .id(vehicle_2_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_2_evolution).send().await?.created()?;
    let vehicle_3_evolution = TemporalEntity::builder()
        .id(vehicle_3_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", Property::builder(40).observed_at("2020-08-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&vehicle_3_evolution).send().await?.created()?;
    let registration = CSourceRegistration::builder(source.endpoint("a")?)
        .id(registration_id.clone())
        .information([RegistrationInfo::builder()
            .entities([EntityInfo::new(vehicle_type.clone()).with_id_pattern(namespace.id_pattern("Vehicle")?)])
            .build()?])
        .operations([DistributedOperation::QueryTemporal])
        .build()?;
    broker.registrations().register(&registration).send().await?;
    // Registrations are deleted when the test ends: a forward to one left behind fails at its endpoint, and the
    // cooldown that opens there holds back a later test's registration naming the same endpoint.
    let _registration_cleanup = Cleanup::of(&broker).registration(&registration_id);
    let source_evolutions = [
        TemporalEntity::builder()
            .id(vehicle_id.clone())
            .entity_type(vehicle_type.clone())
            .language_property(
                "says",
                LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "yes")])?).observed_at("2020-08-01T12:30:00Z".parse()?),
            )
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_2_id.clone())
            .entity_type(vehicle_type.clone())
            .json_property(
                "settings",
                JsonProperty::builder(r#"{"mode":"eco"}"#.parse::<RawJson>()?).observed_at("2020-08-01T12:30:00Z".parse()?),
            )
            .build()?,
        TemporalEntity::builder()
            .id(vehicle_3_id.clone())
            .entity_type(vehicle_type.clone())
            .vocab_property(
                "gender",
                VocabProperty::builder("Male".parse::<VocabTerm>()?).observed_at("2020-08-01T12:30:00Z".parse()?),
            )
            .build()?,
    ];
    source
        .answer(
            Method::GET,
            "/a/ngsi-ld/v1/temporal/entities",
            source_answer::temporal_entities(&source_evolutions),
        )
        .await?;
    let page = broker
        .with_accept(Accept::any())
        .temporal()
        .query_values(temporal_query::before("2030-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .split_entities(EntitySplitting::Split)
        .q(q::attr("says").exists().or(q::attr("settings").exists()).or(q::attr("gender").exists()))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    expect_that!(page.items(), len(eq(3)));
    expect_that!(page.items(), contains(has_id(&vehicle_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_2_id)));
    expect_that!(page.items(), contains(has_id(&vehicle_3_id)));
    Ok(())
}
