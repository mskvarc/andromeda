//! A batch refuses only the entity nesting too deeply.

use andromeda_test_suite::{
    attribute_matcher::any_attribute,
    broker,
    entity_matcher::has_attribute,
    namespace::Namespace,
    nth::nth,
    outcome::Done,
    partial_outcome::PartialSuccess,
    problem::answered,
    report_matcher::batch_error,
    value::object,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{Entity, EntityFragment, ErrorType, NgsiLdMediaType, Value},
};

/// Pins: Batch Entity Creation, Upsert, Update and Merge of one entity with p nesting 33 levels and one with p
/// nesting 32 answer 207 with the first alone in errors as `BadRequestData` naming the limit, write the second, and
/// leave the first absent or as it was
/// Clause: 5.6.7.4, 5.6.8.4, 5.6.9.4, 5.6.20.4, 5.5.4
#[gtest]
#[tokio::test]
async fn a_lim_dep_9011_a_batch_refuses_only_the_entity_nesting_too_deeply() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let nesting_probe_create_refused_id = namespace.urn("NestingProbe", "create-refused")?;
    let nesting_probe_type = namespace.entity_type("NestingProbe")?;
    let nesting_probe_create_written_id = namespace.urn("NestingProbe", "create-written")?;
    let nesting_probe_upsert_refused_id = namespace.urn("NestingProbe", "upsert-refused")?;
    let nesting_probe_upsert_written_id = namespace.urn("NestingProbe", "upsert-written")?;
    let nesting_probe_update_refused_id = namespace.urn("NestingProbe", "update-refused")?;
    let nesting_probe_update_written_id = namespace.urn("NestingProbe", "update-written")?;
    let nesting_probe_merge_refused_id = namespace.urn("NestingProbe", "merge-refused")?;
    let nesting_probe_merge_written_id = namespace.urn("NestingProbe", "merge-written")?;
    let entities = [
        Entity::builder()
            .id(nesting_probe_create_refused_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
        Entity::builder()
            .id(nesting_probe_create_written_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", (0..32).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
    ];
    let report = broker.entities().batch_create(&entities).send().await?.partial()?;
    expect_that!(&report.success, len(eq(1)));
    expect_that!(&report.success, nth(0, eq(&nesting_probe_create_written_id)));
    expect_that!(&report.errors, len(eq(1)));
    expect_that!(
        &report.errors,
        nth(
            0,
            batch_error()
                .entity(&nesting_probe_create_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
                .with_detail_containing("32 levels")
        )
    );
    let answer = broker
        .with_responses()
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_create_refused_id)
        .send()
        .await;
    expect_that!(&answer, answered(StatusCode::NOT_FOUND));
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_create_written_id)
        .send()
        .await?;
    let entity = retrieved.body();
    expect_that!(
        entity,
        has_attribute(
            "p",
            any_attribute().value_member(
                [
                    "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k",
                    "k", "k", "k", "k"
                ],
                "leaf"
            )
        )
    );
    let entities_2 = [
        Entity::builder()
            .id(nesting_probe_upsert_refused_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", object([("k", Value::from("leaf"))]))
            .build()?,
        Entity::builder()
            .id(nesting_probe_upsert_written_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", object([("k", Value::from("leaf"))]))
            .build()?,
    ];
    broker.entities().batch_create(&entities_2).send().await?.done()?;
    let entities_3 = [
        Entity::builder()
            .id(nesting_probe_upsert_refused_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
        Entity::builder()
            .id(nesting_probe_upsert_written_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", (0..32).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
    ];
    let report_2 = broker.entities().batch_upsert(&entities_3).send().await?.partial()?;
    expect_that!(&report_2.success, len(eq(1)));
    expect_that!(&report_2.success, nth(0, eq(&nesting_probe_upsert_written_id)));
    expect_that!(&report_2.errors, len(eq(1)));
    expect_that!(
        &report_2.errors,
        nth(
            0,
            batch_error()
                .entity(&nesting_probe_upsert_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
                .with_detail_containing("32 levels")
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_upsert_refused_id)
        .send()
        .await?;
    let entity_2 = retrieved_2.body();
    expect_that!(
        entity_2,
        has_attribute("p", any_attribute().value_member(["k"], "leaf").without_value_member(["k", "k"]))
    );
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_upsert_written_id)
        .send()
        .await?;
    let entity_3 = retrieved_3.body();
    expect_that!(
        entity_3,
        has_attribute(
            "p",
            any_attribute().value_member(
                [
                    "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k",
                    "k", "k", "k", "k"
                ],
                "leaf"
            )
        )
    );
    let entities_4 = [
        Entity::builder()
            .id(nesting_probe_update_refused_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", object([("k", Value::from("leaf"))]))
            .build()?,
        Entity::builder()
            .id(nesting_probe_update_written_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", object([("k", Value::from("leaf"))]))
            .build()?,
    ];
    broker.entities().batch_create(&entities_4).send().await?.done()?;
    let fragments = [
        EntityFragment::for_entity(nesting_probe_update_refused_id.clone(), nesting_probe_type.clone())
            .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
        EntityFragment::for_entity(nesting_probe_update_written_id.clone(), nesting_probe_type.clone())
            .property("p", (0..32).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
    ];
    let report_3 = broker.entities().batch_update(&fragments).send().await?.partial()?;
    expect_that!(&report_3.success, len(eq(1)));
    expect_that!(&report_3.success, nth(0, eq(&nesting_probe_update_written_id)));
    expect_that!(&report_3.errors, len(eq(1)));
    expect_that!(
        &report_3.errors,
        nth(
            0,
            batch_error()
                .entity(&nesting_probe_update_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
                .with_detail_containing("32 levels")
        )
    );
    let retrieved_4 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_update_refused_id)
        .send()
        .await?;
    let entity_4 = retrieved_4.body();
    expect_that!(
        entity_4,
        has_attribute("p", any_attribute().value_member(["k"], "leaf").without_value_member(["k", "k"]))
    );
    let retrieved_5 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_update_written_id)
        .send()
        .await?;
    let entity_5 = retrieved_5.body();
    expect_that!(
        entity_5,
        has_attribute(
            "p",
            any_attribute().value_member(
                [
                    "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k",
                    "k", "k", "k", "k"
                ],
                "leaf"
            )
        )
    );
    let entities_5 = [
        Entity::builder()
            .id(nesting_probe_merge_refused_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", object([("k", Value::from("leaf"))]))
            .build()?,
        Entity::builder()
            .id(nesting_probe_merge_written_id.clone())
            .entity_type(nesting_probe_type.clone())
            .property("p", object([("k", Value::from("leaf"))]))
            .build()?,
    ];
    broker.entities().batch_create(&entities_5).send().await?.done()?;
    let fragments_2 = [
        EntityFragment::for_entity(nesting_probe_merge_refused_id.clone(), nesting_probe_type.clone())
            .property("p", (0..33).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
        EntityFragment::for_entity(nesting_probe_merge_written_id.clone(), nesting_probe_type.clone())
            .property("p", (0..32).fold(Value::from("leaf"), |inner, _| object([("k", inner)])))
            .build()?,
    ];
    let report_4 = broker.entities().batch_merge(&fragments_2).send().await?.partial()?;
    expect_that!(&report_4.success, len(eq(1)));
    expect_that!(&report_4.success, nth(0, eq(&nesting_probe_merge_written_id)));
    expect_that!(&report_4.errors, len(eq(1)));
    expect_that!(
        &report_4.errors,
        nth(
            0,
            batch_error()
                .entity(&nesting_probe_merge_refused_id)
                .problem(ErrorType::BadRequestData)
                .with_detail_containing("'https://uri.etsi.org/ngsi-ld/default-context/p'")
                .with_detail_containing("32 levels")
        )
    );
    let retrieved_6 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_merge_refused_id)
        .send()
        .await?;
    let entity_6 = retrieved_6.body();
    expect_that!(
        entity_6,
        has_attribute("p", any_attribute().value_member(["k"], "leaf").without_value_member(["k", "k"]))
    );
    let retrieved_7 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&nesting_probe_merge_written_id)
        .send()
        .await?;
    let entity_7 = retrieved_7.body();
    expect_that!(
        entity_7,
        has_attribute(
            "p",
            any_attribute().value_member(
                [
                    "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k", "k",
                    "k", "k", "k", "k"
                ],
                "leaf"
            )
        )
    );
    Ok(())
}
