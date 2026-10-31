//! A normalized object list renders object keyed members.

use andromeda_test_suite::{
    attribute_matcher::attribute_of,
    body_json::json_equals,
    broker,
    entity_matcher::has_attribute,
    evolution_matcher::has_evolution,
    namespace::Namespace,
    outcome::{Done, Upserted},
    simplified_pairs::object_list_pairs,
    versioned_answer::send_versioned,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{
        headers::Accept,
        operation::{RepresentationSpelling, RetrieveEntity},
    },
    model::{AttributeKind, Entity, ListRelationship, Lossless, NgsiLdMediaType, evolution::TemporalEntity},
    query::temporal_query,
};

/// Pins: A temporal `ListRelationship` written with a normalized `objectList` reads back with each member a JSON object
/// holding object, as a pair of bare URIs under temporalValues, and as bare URIs in the concise representation of an
/// Entity written the same way
/// Clause: 4.5.9, 4.5.22.2, Table 5.2.37-1, 5.7.3.4
#[gtest]
#[tokio::test]
async fn n_tmp_trl_9012_a_normalized_object_list_renders_object_keyed_members() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let aggregation_subject_object_list_id = namespace.urn("AggregationSubject", "object-list")?;
    let aggregation_subject_type = namespace.entity_type("AggregationSubject")?;
    let city_antwerp_id = namespace.urn("City", "antwerp")?;
    let city_rotterdam_id = namespace.urn("City", "rotterdam")?;
    let aggregation_subject_object_list_concise_id = namespace.urn("AggregationSubject", "object-list-concise")?;
    let route = vec![city_antwerp_id.clone(), city_rotterdam_id.clone()];
    let aggregation_subject_object_list_evolution = TemporalEntity::builder()
        .id(aggregation_subject_object_list_id.clone())
        .entity_type(aggregation_subject_type.clone())
        .list_relationship("route", ListRelationship::builder(route.clone()).observed_at("2024-01-01T00:00:00Z".parse()?))
        .build()?;
    broker
        .temporal()
        .create_or_update(&aggregation_subject_object_list_evolution)
        .send()
        .await?
        .created()?;
    let retrieved = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve(&aggregation_subject_object_list_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .send()
        .await?;
    expect_that!(
        retrieved.body(),
        has_evolution(
            "route",
            all!(
                len(eq(1)),
                contains(
                    attribute_of(AttributeKind::ListRelationship)
                        .list_length(2)
                        .list_object(0, city_antwerp_id.clone())
                        .list_object(1, city_rotterdam_id.clone())
                )
            )
        )
    );
    let retrieved_2 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .temporal()
        .retrieve_values(&aggregation_subject_object_list_id)
        .temporal_q(temporal_query::after("2023-12-31T00:00:00Z".parse()?))
        .spelling(RepresentationSpelling::default().in_options())
        .send()
        .await?;
    let pairs = object_list_pairs(retrieved_2.body(), "route").or_fail()?;
    expect_that!(pairs, len(eq(1)));
    expect_that!(pairs.first().map(|pair| &pair.value), some(eq(&route)));
    let aggregation_subject_object_list_concise = Entity::builder()
        .id(aggregation_subject_object_list_concise_id.clone())
        .entity_type(aggregation_subject_type)
        .list_relationship("route", ListRelationship::builder(route))
        .build()?;
    broker.entities().create(&aggregation_subject_object_list_concise).send().await?.done()?;
    let retrieved_3 = broker
        .with_accept(Accept::new(NgsiLdMediaType::Json))
        .entities()
        .retrieve(&aggregation_subject_object_list_concise_id)
        .send()
        .await?;
    expect_that!(
        retrieved_3.body(),
        has_attribute(
            "route",
            attribute_of(AttributeKind::ListRelationship)
                .list_object(0, city_antwerp_id.clone())
                .list_object(1, city_rotterdam_id.clone())
        )
    );
    // The concise reading of a list is held as received, since the typed reader does not tell bare URIs from objects.
    let concise = RetrieveEntity::builder()
        .id(&aggregation_subject_object_list_concise_id)
        .format(Lossless::Concise)
        .spelling(RepresentationSpelling::default().in_options())
        .build();
    let answer = send_versioned(&broker.with_accept(Accept::new(NgsiLdMediaType::Json)), &concise, None).await?;
    expect_that!(answer.status, eq(StatusCode::OK));
    expect_that!(&answer.body, json_equals("$.route.objectList[0]", city_antwerp_id.to_string()));
    expect_that!(&answer.body, json_equals("$.route.objectList[1]", city_rotterdam_id.to_string()));
    Ok(())
}
