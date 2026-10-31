//! Eligibility is judged over the selected entities and datasets.

use andromeda_test_suite::{
    aggregation_matcher::{has_aggregate, period},
    broker::{self, BrokerClient},
    namespace::Namespace,
    nth::nth,
    outcome::Upserted,
    problem::{answered, problem},
    unchecked_document::from_rendered,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::CreateOrUpdateTemporalEvolution},
    model::{
        AttributeName,
        DatasetId,
        EntityId,
        EntityTypeName,
        ErrorType,
        Lossless,
        NgsiLdMediaType,
        Property,
        RawJson,
        evolution::{AggregationMethod, TemporalEntity},
    },
    query::{
        temporal_query::{self, builders::TemporalQueryBuilder},
        type_selection,
    },
};

/// The temporal query after 2023-12-31 asking for the average over the whole range.
fn average_after_window() -> Result<TemporalQueryBuilder> {
    Ok(temporal_query::after("2023-12-31T00:00:00Z".parse()?).aggregation_methods([AggregationMethod::Avg]))
}

/// Creates the empty temporal evolution `id` of type `entity_type` with the evolution of `load` set to `instances`,
/// JSON text the model cannot write.
async fn create_with_load(broker: &BrokerClient, id: &EntityId, entity_type: &EntityTypeName, load: &AttributeName, instances: &str) -> Result<()> {
    let entity = TemporalEntity::builder().id(id.clone()).entity_type(entity_type.clone()).build()?;
    let instances: RawJson = instances.parse()?;
    let document = from_rendered(&entity.render(Lossless::Normalized))?.set_member(load.as_ref(), &instances);
    let request = broker
        .unchecked(&CreateOrUpdateTemporalEvolution::builder().entity(&entity).build())?
        .document(&document);
    let created = broker.send_unchecked(request).await;
    expect_that!(&created, answered(StatusCode::CREATED));
    Ok(())
}

/// Pins: avg over a JSON Array Property answers for a query selecting only the Entity holding arrays while an
/// Entity of another type holds Strings under the same name, and is refused with 400 `InvalidRequest` for a query
/// or a retrieval reaching the String holder, whatever page the query asks for; an Entity holding Strings in one
/// dataset and arrays in the default instance is refused unless `datasetId` selects the default instance
/// Clause: 5.7.4.4, 5.7.3.4, Table 4.5.19.1-1
#[gtest]
#[tokio::test]
async fn n_tmp_agg_9036_eligibility_is_judged_over_the_selected_entities_and_datasets() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let load = namespace.attribute("load")?;
    let truck_id = namespace.urn("Truck", "arrays")?;
    let label_id = namespace.urn("Label", "strings")?;
    let crate_id = namespace.urn("Crate", "datasets")?;
    let truck_type = namespace.entity_type("Truck")?;
    let label_type = namespace.entity_type("Label")?;
    let crate_type = namespace.entity_type("Crate")?;
    let text_dataset = namespace.dataset_id("Dataset", "text")?;
    // The model writes no `@json` value object and no JSON array, so those evolutions are set as text on the rendered documents.
    create_with_load(
        &broker,
        &truck_id,
        &truck_type,
        &load,
        concat!(
            r#"[{"type": "Property", "value": {"@type": "@json", "@value": [1, 2]}, "observedAt": "2024-01-01T00:00:00Z"},"#,
            r#"{"type": "Property", "value": {"@type": "@json", "@value": [1, 2, 3, 4]}, "observedAt": "2024-01-01T01:00:00Z"}]"#
        ),
    )
    .await?;
    let label = TemporalEntity::builder()
        .id(label_id.clone())
        .entity_type(label_type.clone())
        .property(load.clone(), Property::builder("beta").observed_at("2024-01-01T00:00:00Z".parse()?))
        .property(load.clone(), Property::builder("alpha").observed_at("2024-01-01T01:00:00Z".parse()?))
        .build()?;
    broker.temporal().create_or_update(&label).send().await?.created()?;
    create_with_load(
        &broker,
        &crate_id,
        &crate_type,
        &load,
        &format!(
            concat!(
                r#"[{{"type": "Property", "value": {{"@type": "@json", "@value": [1, 2]}}, "observedAt": "2024-01-01T00:00:00Z"}},"#,
                r#"{{"type": "Property", "value": "unlabelled", "datasetId": "{}", "observedAt": "2024-01-01T00:00:00Z"}}]"#
            ),
            text_dataset
        ),
    )
    .await?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let refusing = broker.with_responses().with_accept(Accept::new(NgsiLdMediaType::Json));
    let load_name = load.to_string();
    let selected = json
        .temporal()
        .query_aggregated(average_after_window()?)
        .entity_type(&truck_type)
        .attrs(vec![load_name.parse()?])
        .send()
        .await?;
    expect_that!(selected.items(), len(eq(1)));
    let truck = selected.items().first().or_fail()?;
    expect_that!(truck.id, eq(&truck_id));
    expect_that!(
        truck,
        has_aggregate(&load_name, AggregationMethod::Avg, all!(len(eq(1)), nth(0, period().value(3))))
    );
    let retrieved = json
        .temporal()
        .retrieve_aggregated(&truck_id)
        .temporal_q(average_after_window()?)
        .attrs(vec![load_name.parse()?])
        .send()
        .await?;
    expect_that!(retrieved.body(), has_aggregate(&load_name, AggregationMethod::Avg, nth(0, period().value(3))));
    let strings = refusing
        .temporal()
        .query_aggregated(average_after_window()?)
        .entity_type(&label_type)
        .attrs(vec![load_name.parse()?])
        .send()
        .await;
    expect_that!(&strings, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    let paged = refusing
        .temporal()
        .query_aggregated(average_after_window()?)
        .entity_type(type_selection::entity_type(&truck_type).or(&label_type))
        .attrs(vec![load_name.parse()?])
        .limit(1)
        .send()
        .await;
    expect_that!(&paged, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    for refused_id in [&label_id, &crate_id] {
        let refused = refusing
            .temporal()
            .retrieve_aggregated(refused_id)
            .temporal_q(average_after_window()?)
            .attrs(vec![load_name.parse()?])
            .send()
            .await;
        expect_that!(&refused, problem(StatusCode::BAD_REQUEST, ErrorType::InvalidRequest));
    }
    let default_instance = json
        .temporal()
        .retrieve_aggregated(&crate_id)
        .temporal_q(average_after_window()?)
        .attrs(vec![load_name.parse()?])
        .dataset_ids(vec![DatasetId::DefaultInstance])
        .send()
        .await?;
    expect_that!(
        default_instance.body(),
        has_aggregate(&load_name, AggregationMethod::Avg, nth(0, period().value(2)))
    );
    Ok(())
}
