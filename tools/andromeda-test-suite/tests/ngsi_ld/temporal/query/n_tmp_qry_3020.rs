//! Id takes precedence over id pattern in get and post temporal queries.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_lookup::with_id,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    temporal_page_body::entities,
    written_temporal_query::{get_parameters, post},
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::model::{Property, evolution::TemporalEntity};

/// Pins: Query Temporal Evolution of Entities applies id and ignores `idPattern` when both are given, through the
/// GET parameters and through the `EntitySelector` of the POST body
/// Clause: 5.2.33, 6.18.3.2, 6.24.3.1, 5.7.4.4
#[gtest]
#[tokio::test]
async fn n_tmp_qry_3020_id_takes_precedence_over_id_pattern_in_get_and_post_temporal_queries() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let a1_id = namespace.urn("Meter", "a1")?;
    let b1_id = namespace.urn("Meter", "b1")?;
    let meter_type = namespace.entity_type("Meter")?;
    for (id, power) in [(&a1_id, 1), (&b1_id, 2)] {
        let meter = TemporalEntity::builder()
            .id(id.clone())
            .entity_type(meter_type.clone())
            .property("power", Property::builder(power).observed_at("2020-01-01T00:00:00Z".parse()?))
            .build()?;
        broker.temporal().create_or_update(&meter).send().await?.created()?;
    }
    // The selector parameters refuse `id` beside `idPattern`, which the specification gives a meaning (clause 5.2.33),
    // so these requests are sent as written.
    let meter_type_text = meter_type.to_string();
    let pattern = format!("^urn:ngsi-ld:Meter:{namespace}-a.*");
    let time = [("timerel", "after"), ("timeAt", "2000-01-01T00:00:00Z")];
    let b1_text = b1_id.to_string();
    let selecting_b1 = [("type", meter_type_text.as_str()), ("id", b1_text.as_str()), ("idPattern", pattern.as_str())];
    let by_id = get_parameters(&broker, &[selecting_b1.as_slice(), time.as_slice()].concat(), "application/json").await??;
    expect_that!(by_id.status(), eq(StatusCode::OK));
    let page = entities(&by_id)?;
    expect_that!(page, len(eq(1)));
    expect_that!(page.first().map(|entity| &entity.id), some(eq(&b1_id)));
    expect_that!(page.first(), some(has_evolution("power", nth(0, any_attribute().value(2)))));
    let both_ids = format!("{a1_id},{b1_id}");
    let selecting_both = [("type", meter_type_text.as_str()), ("id", both_ids.as_str()), ("idPattern", pattern.as_str())];
    let by_ids = get_parameters(&broker, &[selecting_both.as_slice(), time.as_slice()].concat(), "application/json").await??;
    expect_that!(by_ids.status(), eq(StatusCode::OK));
    let page_2 = entities(&by_ids)?;
    expect_that!(page_2, len(eq(2)));
    expect_that!(with_id(&page_2, &a1_id), some(anything()));
    expect_that!(with_id(&page_2, &b1_id), some(anything()));
    let posted = post(
        &broker,
        "",
        &format!(
            r#"{{"type": "Query", "entities": [{{"type": "{meter_type_text}", "id": "{b1_id}", "idPattern": "{pattern}"}}], "temporalQ": {{"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}}}}"#
        ),
        "application/json",
        "application/json",
    )
    .await??;
    expect_that!(posted.status(), eq(StatusCode::OK));
    let page_3 = entities(&posted)?;
    expect_that!(page_3, len(eq(1)));
    expect_that!(page_3.first().map(|entity| &entity.id), some(eq(&b1_id)));
    expect_that!(page_3.first(), some(has_evolution("power", nth(0, any_attribute().value(2)))));
    let posted_2 = post(
        &broker,
        "",
        &format!(
            r#"{{"type": "Query", "entities": [{{"type": "{meter_type_text}", "id": ["{a1_id}", "{b1_id}"], "idPattern": "^urn:ngsi-ld:Meter:{namespace}-zzz.*"}}], "temporalQ": {{"timerel": "after", "timeAt": "2000-01-01T00:00:00Z"}}}}"#
        ),
        "application/json",
        "application/json",
    )
    .await??;
    expect_that!(posted_2.status(), eq(StatusCode::OK));
    let page_4 = entities(&posted_2)?;
    expect_that!(page_4, len(eq(2)));
    expect_that!(with_id(&page_4, &a1_id), some(anything()));
    expect_that!(with_id(&page_4, &b1_id), some(anything()));
    Ok(())
}
