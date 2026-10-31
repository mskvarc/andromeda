//! Lang answers every instance in the requested language.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, attribute_of, property},
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeKind, LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{language_filter::LanguageFilter, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities selecting an Entity whose label `LanguageProperty` holds three
/// instances answers each as a `LanguageProperty` without lang, and with lang=en as a Property holding the English
/// string with lang en
/// Clause: 4.15, Table 6.18.3.2-1
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9028_lang_answers_every_instance_in_the_requested_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_lang_query_id = namespace.urn("Vehicle", "lang-query")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_lang_query_evolution = TemporalEntity::builder()
        .id(vehicle_lang_query_id.clone())
        .entity_type(vehicle_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Pump 0"), ("de", "Pumpe 0")])?).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Pump 1"), ("de", "Pumpe 1")])?).observed_at("2026-01-02T00:00:00Z".parse()?),
        )
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Pump 2"), ("de", "Pumpe 2")])?).observed_at("2026-01-03T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_lang_query_evolution).send().await?.created()?;
    let page = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_lang_query_id.clone()])
        .send()
        .await?;
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_evolution(
                "label",
                all!(
                    len(eq(3)),
                    contains(attribute_of(AttributeKind::LanguageProperty).observed_at("2026-01-01T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).with_member(Member::LanguageMap)),
                    contains(attribute_of(AttributeKind::LanguageProperty).observed_at("2026-01-02T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).with_member(Member::LanguageMap)),
                    contains(attribute_of(AttributeKind::LanguageProperty).observed_at("2026-01-03T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?).with_member(Member::LanguageMap))
                )
            )
        )
    );
    let page_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_lang_query_id.clone()])
        .lang("en".parse::<LanguageFilter>()?)
        .send()
        .await?;
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            has_evolution(
                "label",
                all!(
                    len(eq(3)),
                    contains(property().observed_at("2026-01-01T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).value("Pump 0")),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).lang("en")),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    )),
                    contains(property().observed_at("2026-01-02T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).value("Pump 1")),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).lang("en")),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    )),
                    contains(property().observed_at("2026-01-03T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?).value("Pump 2")),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?).lang("en")),
                    contains(any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-03T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    ))
                )
            )
        )
    );
    Ok(())
}
