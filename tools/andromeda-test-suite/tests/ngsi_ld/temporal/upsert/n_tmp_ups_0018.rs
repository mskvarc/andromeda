//! A language property gains a second instance.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of},
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
};

/// Pins: posting a second `LanguageProperty` instance for an existing Temporal Evolution answers 204 and the
/// history then holds both instances with their own `languageMaps`
/// Clause: 5.6.11.4, 5.6.12.4, 4.5.18
#[gtest]
#[tokio::test]
async fn n_tmp_ups_0018_a_language_property_gains_a_second_instance() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Fast Car")])?).observed_at("2020-08-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let vehicle_evolution_2 = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .language_property(
            "name",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Super Fast Car"), ("de", "Super Schnelles Auto")])?)
                .observed_at("2020-08-01T13:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution_2).send().await?.updated()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&vehicle_id)
        .send()
        .await?;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "name",
            all!(
                len(eq(2)),
                contains(attribute_of(AttributeKind::LanguageProperty).language("en", "Fast Car")),
                contains(any_attribute().language("en", "Fast Car")).times(eq(1)),
                contains(any_attribute().language("en", "Fast Car").observed_at("2020-08-01T12:00:00Z".parse()?)),
                contains(any_attribute().language("en", "Fast Car")).times(eq(1)),
                contains(attribute_of(AttributeKind::LanguageProperty).language("en", "Super Fast Car")),
                contains(any_attribute().language("en", "Super Fast Car")).times(eq(1)),
                contains(any_attribute().language("en", "Super Fast Car").language("de", "Super Schnelles Auto")),
                contains(any_attribute().language("en", "Super Fast Car")).times(eq(1)),
                contains(any_attribute().language("en", "Super Fast Car").observed_at("2020-08-01T13:00:00Z".parse()?)),
                contains(any_attribute().language("en", "Super Fast Car")).times(eq(1))
            )
        )
    );
    Ok(())
}
