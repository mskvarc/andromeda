//! A follow up changing an input beside its query is bad request data.

use andromeda_test_suite::{broker, identity_matcher::has_id, namespace::Namespace, outcome::Done, problem::problem, received_status::ExpectedStatus};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::{headers::Accept, operation::EntityMapRequest},
    model::{Entity, ErrorType, NgsiLdMediaType},
    query::language_filter::LanguageFilter,
};

/// Pins: Query Entities by type under an `EntityMap` that Create `EntityMap` for Query Entities created by that type
/// with lang fr and one `datasetId` answers the mapped Entity with the same lang and `datasetId` and limit 1, and
/// answers 400 `BadRequestData` with lang en, with another `datasetId`, and with no lang
/// Clause: 5.5.14, 5.7.2.3
/// Recorded: clause 5.5.14 excepts only the specification of Entity identifiers, the pagination parameters and the
/// temporal query from the parameters a request under an `EntityMap` repeats, so the language filter and the
/// `datasetId` are held to those of the request that created it
#[gtest]
#[tokio::test]
async fn n_map_qen_9022_a_follow_up_changing_an_input_beside_its_query_is_bad_request_data() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let json = broker.with_accept(Accept::new(NgsiLdMediaType::Json));
    let vehicle_type = namespace.entity_type("Vehicle")?;
    let vehicle_id = namespace.urn("Vehicle", 1)?;
    let fleet = namespace.dataset_id("Dataset", "fleet")?;
    let other_fleet = namespace.dataset_id("Dataset", "other")?;
    let french: LanguageFilter = "fr".parse()?;
    let english: LanguageFilter = "en".parse()?;
    let vehicle = Entity::builder()
        .id(vehicle_id.clone())
        .entity_type(vehicle_type.clone())
        .property("speed", 50)
        .build()?;
    broker.entities().create(&vehicle).send().await?.done()?;
    let created = json
        .with_heads()
        .entity_maps()
        .create()
        .entity_type(&vehicle_type)
        .lang(french.clone())
        .dataset_ids(vec![fleet.clone()])
        .send()
        .await?
        .expecting(StatusCode::CREATED)?;
    let entity_map_request = EntityMapRequest::reuse(created.entity_map().valid().or_fail()?.clone());
    let page = json
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .lang(french.clone())
        .dataset_ids(vec![fleet.clone()])
        .limit(1)
        .entity_map(&entity_map_request)
        .send()
        .await?;
    expect_that!(page.items(), elements_are![has_id(&vehicle_id)]);
    let other_language = json
        .with_responses()
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .lang(english)
        .dataset_ids(vec![fleet.clone()])
        .entity_map(&entity_map_request)
        .send()
        .await;
    expect_that!(&other_language, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let other_dataset = json
        .with_responses()
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .lang(french)
        .dataset_ids(vec![other_fleet])
        .entity_map(&entity_map_request)
        .send()
        .await;
    expect_that!(&other_dataset, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    let no_language = json
        .with_responses()
        .entities()
        .query()
        .entity_type(&vehicle_type)
        .dataset_ids(vec![fleet])
        .entity_map(&entity_map_request)
        .send()
        .await;
    expect_that!(&no_language, problem(StatusCode::BAD_REQUEST, ErrorType::BadRequestData));
    broker.entity_maps().delete(&created.body().id).send().await?;
    Ok(())
}
