//! Lang answers every instance in the requested language.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of, property},
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, DateTime, LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{language_filter::LanguageFilter, temporal_query},
};

/// Pins: Retrieve Temporal Evolution of an Entity whose label `LanguageProperty` holds three instances answers each as a
/// `LanguageProperty` without lang, and with lang=en or lang=de answers each as a Property holding the string of that
/// language with lang and its observedAt
/// Clause: 4.15, Table 6.19.3.1-1
#[gtest]
#[tokio::test]
async fn n_tmp_ret_9013_lang_answers_every_instance_in_the_requested_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_lang_retrieve_id = namespace.urn("Vehicle", "lang-retrieve")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let days = ["2026-01-01T00:00:00Z", "2026-01-02T00:00:00Z", "2026-01-03T00:00:00Z"]
        .into_iter()
        .map(str::parse::<DateTime>)
        .collect::<Result<Vec<_>, _>>()?;
    let vehicle_lang_retrieve_evolution = days
        .iter()
        .enumerate()
        .try_fold(
            TemporalEntity::builder().id(vehicle_lang_retrieve_id.clone()).entity_type(vehicle_type),
            |builder, (index, day)| -> Result<_> {
                let map = LanguageMap::try_from_pairs([("en", format!("Pump {index}")), ("de", format!("Pumpe {index}"))])?;
                Ok(builder.language_property("label", LanguageProperty::builder(map).observed_at(*day)))
            },
        )?
        .build()?;
    broker.temporal().create_or_update(&vehicle_lang_retrieve_evolution).send().await?.created()?;
    let client = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let retrieved = client
        .temporal()
        .retrieve(&vehicle_lang_retrieve_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(retrieved.body(), has_evolution("label", len(eq(3))));
    for (index, day) in days.iter().enumerate() {
        expect_that!(
            retrieved.body(),
            has_evolution(
                "label",
                contains(
                    attribute_of(AttributeKind::LanguageProperty)
                        .observed_at(*day)
                        .language("en", &format!("Pump {index}"))
                        .language("de", &format!("Pumpe {index}"))
                )
            )
        );
    }
    for (language, prefix) in [("en", "Pump"), ("de", "Pumpe")] {
        let retrieved_2 = client
            .temporal()
            .retrieve(&vehicle_lang_retrieve_id)
            .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
            .lang(language.parse::<LanguageFilter>()?)
            .send()
            .await?;
        expect_that!(retrieved_2.body(), has_evolution("label", len(eq(3))));
        for (index, day) in days.iter().enumerate() {
            expect_that!(
                retrieved_2.body(),
                has_evolution(
                    "label",
                    contains(property().observed_at(*day).value(format!("{prefix} {index}")).lang(language))
                )
            );
            expect_that!(
                retrieved_2.body(),
                has_evolution("label", not(contains(any_attribute().observed_at(*day).with_member(Member::LanguageMap))))
            );
        }
    }
    Ok(())
}
