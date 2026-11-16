//! A posted temporal query with a ranked lang answers every instance as the get query does.

use andromeda_test_suite::{
    attribute_matcher::property,
    attribute_member::Member,
    broker,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
};
use googletest::prelude::*;
use ngsi_ld::{
    http::{headers::Accept, operation::QueryMethod},
    model::{LanguageMap, LanguageProperty, NgsiLdMediaType, evolution::TemporalEntity},
    query::{language_filter::LanguageFilter, temporal_query},
};

/// Pins: Query Temporal Evolution of Entities by POST of an Entity whose label `LanguageProperty` holds de, en and
/// fr in two instances with `lang` `fr-CH,en;q=0.9,*;q=0.5` answers each instance as a Property holding the en
/// string with lang en, the same Entity evolution the GET binding answers for that filter
/// Clause: 4.15, 6.24, Table 5.2.23-1
/// Recorded: Table 5.2.23-1 gives the posted Query the `lang` member of the `lang` query parameter, so the ranked
/// filter of clause 4.15 example 4 applies to both bindings alike
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9388_a_posted_temporal_query_with_a_ranked_lang_answers_every_instance_as_the_get_query_does() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vehicle_id = namespace.urn("Vehicle", "ranked-lang")?;
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let evolution = TemporalEntity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Pumpe 0"), ("en", "Pump 0"), ("fr", "Pompe 0")])?)
                .observed_at("2026-01-01T00:00:00Z".parse()?),
        )
        .language_property(
            "label",
            LanguageProperty::builder(LanguageMap::try_from_pairs([("de", "Pumpe 1"), ("en", "Pump 1"), ("fr", "Pompe 1")])?)
                .observed_at("2026-01-02T00:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&evolution).send().await?.created()?;
    let lang: LanguageFilter = "fr-CH,en;q=0.9,*;q=0.5".parse()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let posted = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .lang(lang.clone())
        .method(QueryMethod::Post)
        .send()
        .await?;
    expect_that!(posted.items(), len(eq(1)));
    expect_that!(
        posted.items(),
        nth(
            0,
            has_evolution(
                "label",
                all!(
                    len(eq(2)),
                    contains(
                        property()
                            .observed_at("2026-01-01T00:00:00Z".parse()?)
                            .value("Pump 0")
                            .lang("en")
                            .without(Member::LanguageMap)
                    ),
                    contains(
                        property()
                            .observed_at("2026-01-02T00:00:00Z".parse()?)
                            .value("Pump 1")
                            .lang("en")
                            .without(Member::LanguageMap)
                    )
                )
            )
        )
    );
    let by_get = json
        .temporal()
        .query(temporal_query::after("2000-01-01T00:00:00Z".parse()?))
        .entity_type(&vehicle_type)
        .ids(vec![vehicle_id.clone()])
        .lang(lang)
        .send()
        .await?;
    expect_that!(by_get.items(), eq(posted.items()));
    Ok(())
}
