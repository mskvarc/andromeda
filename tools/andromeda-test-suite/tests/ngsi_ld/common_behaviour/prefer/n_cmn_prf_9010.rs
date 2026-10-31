//! Version 1 0 amends the sub attributes.

use andromeda_test_suite::{
    body_json::{json_absent, json_equals},
    broker,
    namespace::Namespace,
    outcome::Done,
    versioned_answer::send_versioned,
};
use cefact_units::UnitCode;
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity},
    },
    model::{Entity, LanguageMap, LanguageProperty, Lossless, NgsiLdMediaType, Property},
};

/// Pins: Retrieve Entity with options=normalized and Prefer ngsi-ld=1.0 answers 203 with a sub-Property without
/// `unitCode` and `observedAt` and a `LanguageProperty` sub-attribute reformatted as a Property
/// Clause: 4.3.6.8, 4.5.2, 4.5.18
#[gtest]
#[tokio::test]
async fn n_cmn_prf_9010_version_1_0_amends_the_sub_attributes() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "sub-attributes")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type)
        .property(
            "speed",
            Property::builder(21)
                .property(
                    "accuracy",
                    Property::builder(1).unit_code(UnitCode::Cel).observed_at("2025-01-01T00:00:00Z".parse()?),
                )
                .language_property("title", LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Fast")])?)),
        )
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let normalized = RetrieveEntity::builder()
        .id(&vehicle_id)
        .format(Lossless::Normalized)
        .spelling(RepresentationSpelling::default().in_options().explicit_normalized())
        .build();
    // The answers are adapted to an older version, so their wire form is held as received.
    let current = send_versioned(&json, &normalized, None).await?;
    expect_that!(current.status, eq(StatusCode::OK));
    expect_that!(&current.body, json_equals("$.speed.accuracy.unitCode", "CEL"));
    expect_that!(&current.body, json_equals("$.speed.title.type", "LanguageProperty"));
    let adapted = send_versioned(&json, &normalized, Some("1.0")).await?;
    expect_that!(adapted.status, eq(StatusCode::NON_AUTHORITATIVE_INFORMATION));
    expect_that!(adapted.preference_applied.as_deref(), some(eq("ngsi-ld=1.0")));
    expect_that!(&adapted.body, json_equals("$.speed.accuracy.type", "Property"));
    expect_that!(&adapted.body, json_equals("$.speed.accuracy.value", 1));
    expect_that!(&adapted.body, json_absent("$.speed.accuracy.unitCode"));
    expect_that!(&adapted.body, json_absent("$.speed.accuracy.observedAt"));
    expect_that!(&adapted.body, json_equals("$.speed.title.type", "Property"));
    expect_that!(&adapted.body, json_equals("$.speed.title.value.en", "Fast"));
    expect_that!(&adapted.body, json_absent("$.speed.title.languageMap"));
    Ok(())
}
