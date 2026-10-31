//! Temporal retrieve and query answer the accepted language.

use andromeda_test_suite::{
    attribute_matcher::{any_attribute, property},
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    wire::vary,
};
use googletest::prelude::*;
use http::HeaderName;
use ngsi_ld::{
    http::{
        headers::{Accept, AcceptLanguage},
        operation::QueryMethod,
    },
    model::{LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{language_filter::LanguageFilter, temporal_query},
};

/// Pins: Retrieve Temporal Evolution, Query Temporal Evolution by GET and by POST with Accept-Language de and no
/// lang answer every label instance as a Property holding the German string with lang de, and each response varies
/// on accept-language
/// Clause: 4.15, 5.7.3.4, 5.7.4.4
/// Recorded: Andromeda extension, not v1.9.1: the `accept_language` extension reads Accept-Language as the clause
/// 4.15 language filter where no lang names one; NGSI-LD carries the filter in lang alone
#[gtest]
#[tokio::test]
async fn a_ext_lng_0008_temporal_retrieve_and_query_answer_the_accepted_language() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Pump 0"), ("de", "Pumpe 0")])?).observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("en", "Pump 1"), ("de", "Pumpe 1")])?).observed_at("2026-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&vehicle_evolution).send().await?.created()?;
    let headed = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("de".parse::<LanguageFilter>()?)?)?
        .temporal()
        .retrieve(&vehicle_id)
        .temporal_q(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(vary(headed.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    let retrieved = &headed.outcome;
    let evolution = retrieved.body();
    expect_that!(
        evolution,
        has_evolution(
            "label",
            all!(
                len(eq(2)),
                contains(property().observed_at("2026-01-01T00:00:00Z".parse()?)),
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).value("Pumpe 0")),
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).lang("de")),
                contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                not(contains(
                    any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                )),
                contains(property().observed_at("2026-01-02T00:00:00Z".parse()?)),
                contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).value("Pumpe 1")),
                contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).lang("de")),
                contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                not(contains(
                    any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                ))
            )
        )
    );
    let headed_2 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("de".parse::<LanguageFilter>()?)?)?
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let page = headed_2.outcome;
    expect_that!(vary(headed_2.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    expect_that!(page.items(), len(eq(1)));
    expect_that!(
        page.items(),
        nth(
            0,
            has_evolution(
                "label",
                all!(
                    len(eq(2)),
                    contains(property().observed_at("2026-01-01T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).value("Pumpe 0")),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).lang("de")),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    )),
                    contains(property().observed_at("2026-01-02T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).value("Pumpe 1")),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).lang("de")),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    ))
                )
            )
        )
    );
    let headed_3 = broker
        .with_heads()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .with_header(AcceptLanguage::try_from("de".parse::<LanguageFilter>()?)?)?
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .method(QueryMethod::Post)
        .entity_type(&vehicle_type)
        .send()
        .await?;
    let page_2 = headed_3.outcome;
    expect_that!(vary(headed_3.head.headers()), contains(eq(&HeaderName::from_static("accept-language"))));
    expect_that!(page_2.items(), len(eq(1)));
    expect_that!(
        page_2.items(),
        nth(
            0,
            has_evolution(
                "label",
                all!(
                    len(eq(2)),
                    contains(property().observed_at("2026-01-01T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).value("Pumpe 0")),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).lang("de")),
                    contains(any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-01T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    )),
                    contains(property().observed_at("2026-01-02T00:00:00Z".parse()?)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).value("Pumpe 1")),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).lang("de")),
                    contains(any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?)).times(eq(1)),
                    not(contains(
                        any_attribute().observed_at("2026-01-02T00:00:00Z".parse()?).with_member(Member::LanguageMap)
                    ))
                )
            )
        )
    );
    Ok(())
}
