//! A temporal q pattern skips a declared Date.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, nth::nth, outcome::Upserted};
use googletest::prelude::*;
use ngsi_ld::{
    http::headers::Accept,
    model::{NgsiLdMediaType, Property, evolution::TemporalEntity},
    query::{q, temporal_query},
};

/// Pins: for a Diary whose `d` is "2024-01-15" declared an NGSI-LD Date and a Diary whose `d` is the same text with
/// no `valueType`, each observed in 2020, Query Temporal Evolution of Entities with q d~="2024.*" and with
/// q d!~="1999.*" selects only the undeclared one; both pattern operators answer not matching for a target value whose
/// data type is not String
/// Clause: 4.9, 4.6.3, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_9386_a_temporal_q_pattern_skips_a_declared_date() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let diary_type = namespace.entity_type("Diary")?;
    let dated_id = namespace.urn("Diary", "dated")?;
    let text_id = namespace.urn("Diary", "text")?;
    let dated = TemporalEntity::builder()
        .id(dated_id)
        .entity_type(diary_type.clone())
        .property(
            "d",
            Property::builder("2024-01-15")
                .value_type("https://uri.etsi.org/ngsi-ld/Date".parse()?)
                .observed_at("2020-01-01T12:00:00Z".parse()?),
        )
        .build()?;
    broker.temporal().create_or_update(&dated).send().await?.created()?;
    let text = TemporalEntity::builder()
        .id(text_id.clone())
        .entity_type(diary_type.clone())
        .property("d", Property::builder("2024-01-15").observed_at("2020-01-01T12:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&text).send().await?.created()?;
    for pattern in [q::attr("d").matches("\"2024.*\""), q::attr("d").not_matches("\"1999.*\"")] {
        let page = broker
            .with_accept(Accept::new(NgsiLdMediaType::Json))
            .temporal()
            .query(temporal_query::after("2019-01-01T00:00:00Z".parse()?))
            .entity_type(&diary_type)
            .q(pattern)
            .send()
            .await?;
        expect_that!(page.items(), len(eq(1)));
        expect_that!(page.items(), nth(0, has_id(&text_id)));
    }
    Ok(())
}
