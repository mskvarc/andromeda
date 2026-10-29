//! ETSI `019_01`: several entities are queried by their ids, their types, an id pattern, attribute names, a geoquery and
//! scope expressions (clause 5.7.2).
//!
//! Each expectation compares the whole answer as the suite's `Compare Dictionaries Ignoring Keys` does: the
//! `instanceId` the case's exclude pattern hides is allowed below the root, and the `@context` of a response may be any
//! versioned core `@context`, alone or as a one-element list.

use crate::{
    suite_keywords::{OBSERVED_AT_WHOLE_SECOND, create_entity, create_entity_with_link, delete_entities},
    suite_payloads::{BUILDING_SIMPLE_TERMS, bare_building, building_simple_attributes, eiffel_tower_located, offstreet_parking, point, vehicle_parked},
};
use andromeda_test_suite::{
    broker::{self, BrokerClient},
    comparison::{ContextRule, matches_document},
    namespace::Namespace,
    problem::answered,
    vocabulary::Vocabulary,
    wire::json_body,
};
use googletest::prelude::*;
use http::StatusCode;
use ngsi_ld::{
    http::headers::Accept,
    model::{AttributeName, CORE_CONTEXT, Entity, NgsiLdMediaType, OneOrMany, Scope},
    query::{SelectorType, geoquery, scope_query::ScopeQuery},
};
use serde_json::{Value, json};

/// The terms of the `@context` of the type-selection case.
const TYPES_TERMS: [&str; 7] = ["Building", "Vehicle", "brandName", "isParked", "OffStreetParking", "name", "reliability"];

/// The terms of the `@context` of the geoquery case.
const GEO_TERMS: [&str; 3] = ["Building", "name", "subCategory"];

/// Creates the case's Building `n` in `scopes`, as `application/json` with the vocabulary named in a `Link` header and
/// no `@context` in the body, and requires 201.
async fn create_scoped(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace, n: u32, scopes: &[&str]) -> Result<()> {
    let scopes = scopes.iter().map(|scope| Ok(scope.parse::<Scope>()?)).collect::<Result<Vec<_>>>()?;
    let building = Entity::builder()
        .id(namespace.urn("Building", n)?)
        .entity_type("Building")
        .scope(OneOrMany::try_from(scopes)?)
        .build()?;
    create_entity_with_link(broker, vocabulary, &building).await
}

/// Creates the two scoped Buildings of the scope cases: `urn:ngsi-ld:Building:<ns>-1` in `/Madrid/Gardens/ParqueNorte`,
/// and `urn:ngsi-ld:Building:<ns>-2` in that scope and in `/CompanyA/OrganizationB/UnitC`.
async fn create_scoped_pair(broker: &BrokerClient, vocabulary: &Vocabulary, namespace: &Namespace) -> Result<()> {
    create_scoped(broker, vocabulary, namespace, 1, &["/Madrid/Gardens/ParqueNorte"]).await?;
    create_scoped(
        broker,
        vocabulary,
        namespace,
        2,
        &["/Madrid/Gardens/ParqueNorte", "/CompanyA/OrganizationB/UnitC"],
    )
    .await
}

/// Queries the Buildings of `namespace` whose scope matches `scope`, without an `Accept` header, and answers the body.
async fn scoped_answer(broker: &BrokerClient, namespace: &Namespace, scope: &str) -> Result<Value> {
    let received = broker
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .scope_q(scope.parse::<ScopeQuery>()?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    Ok(json_body(&received?)?)
}

/// Pins: Check that one can query several entities based on ids
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_01_01_query_several_entities_based_on_ids() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 3)?).await?;
    let received = broker
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .ids(vec![namespace.urn("Building", 1)?, namespace.urn("Building", 2)?])
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let building = namespace.expanded_term("Building")?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 1), "type": building},
            {"id": namespace.urn_text("Building", 2), "type": building},
        ]))
        .hiding("instanceId")
    );
    delete_entities(
        &broker,
        &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?, &namespace.urn("Building", 3)?],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on the entities types
/// Clause: 5.7.2
/// Corrected: `fix(019_01_02)`: key the query-by-type expectations by expanded iris (5.5.7)
/// Changed: providedBy, availableSpotNumber, totalSpotNumber stay out of the file vocabulary: the case reads them without
/// a @context, so they expand under the core default vocabulary as in the Robot run
/// Changed: observedAt is compared as written, where the case matches the whole-second pattern
#[gtest]
#[tokio::test]
async fn e019_01_02_query_several_entities_based_on_the_entities_types() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &TYPES_TERMS).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &vehicle_parked(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &offstreet_parking(&namespace, 1)?).await?;
    let types: SelectorType = format!("{},{}", namespace.expanded_term("Vehicle")?, namespace.expanded_term("OffStreetParking")?).parse()?;
    let received = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .entity_type(types)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([
        {
            "id": namespace.urn_text("Vehicle", 1),
            "type": namespace.expanded_term("Vehicle")?,
            namespace.expanded_term("brandName")?: {"type": "Property", "value": "Mercedes"},
            namespace.expanded_term("isParked")?: {
                "type": "Relationship",
                "object": namespace.urn_text("OffStreetParking", "Downtown1"),
                "observedAt": "2017-07-29T12:00:04Z",
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Person", "Bob")},
            },
            "@context": CORE_CONTEXT.as_str(),
        },
        {
            "id": namespace.urn_text("OffStreetParking", 1),
            "type": namespace.expanded_term("OffStreetParking")?,
            namespace.expanded_term("name")?: {"type": "Property", "value": "Downtown One"},
            "availableSpotNumber": {
                "type": "Property",
                "value": 121,
                "observedAt": "2017-07-29T12:05:02Z",
                namespace.expanded_term("reliability")?: {"type": "Property", "value": 0.7},
                "providedBy": {"type": "Relationship", "object": namespace.urn_text("Camera", "C1")},
            },
            "totalSpotNumber": {"type": "Property", "value": 200},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [-8.5, 41.2]}},
            "@context": CORE_CONTEXT.as_str(),
        },
    ]);
    expect_that!(
        &body,
        matches_document(expected).hiding("instanceId").context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete_entities(
        &broker,
        &[
            &namespace.urn("Building", 1)?,
            &namespace.urn("Vehicle", 1)?,
            &namespace.urn("OffStreetParking", 1)?,
        ],
    )
    .await;
    Ok(())
}

/// Pins: Check that one can query several entities based on the given id pattern
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_01_03_query_several_entities_based_on_the_given_id_pattern() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let received = broker
        .with_accept(Accept::any())
        .with_responses()
        .entities()
        .query()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .id_pattern(namespace.id_pattern("Building")?)
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let building = namespace.expanded_term("Building")?;
    expect_that!(
        &body,
        matches_document(json!([
            {"id": namespace.urn_text("Building", 2), "type": building},
            {"id": namespace.urn_text("Building", 1), "type": building},
        ]))
        .hiding("instanceId")
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on attribute names
/// Clause: 5.7.2
/// Tenant: private (a query selected by attribute names alone names no entity type, id or idPattern, so clause 5.12
/// matches it with every registration of the tenant that names those attributes or no attribute at all, and the broker
/// forwards it to each; in the shared tenant another file's context source then answers it beside the case's own
/// entities)
#[gtest]
#[tokio::test]
async fn e019_01_04_query_several_entities_based_on_attribute_names() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let tenant = broker::private_tenant(&broker, &namespace).await?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &BUILDING_SIMPLE_TERMS).await?;
    create_entity(&tenant, &vocabulary, &building_simple_attributes(&namespace, 1)?).await?;
    create_entity(&tenant, &vocabulary, &bare_building(&namespace, 2)?).await?;
    let attrs: Vec<AttributeName> = vec![
        namespace.expanded_term("airQualityLevel")?.parse()?,
        namespace.expanded_term("subCategory")?.parse()?,
    ];
    let received = tenant.with_accept(Accept::any()).with_responses().entities().query().attrs(attrs).send().await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let expected = json!([{
        "id": namespace.urn_text("Building", 1),
        "type": namespace.expanded_term("Building")?,
        namespace.expanded_term("airQualityLevel")?: {"type": "Property", "value": 4, "unitCode": "C62"},
        namespace.expanded_term("subCategory")?: {"type": "Property", "value": "tourism"},
    }]);
    expect_that!(&body, matches_document(expected).hiding("instanceId").hiding("observedAt"));
    let air_quality_level = namespace.expanded_term("airQualityLevel")?;
    expect_that!(
        body.get(0)
            .and_then(|building| building.get(&air_quality_level))
            .and_then(|level| level.get("observedAt"))
            .and_then(Value::as_str),
        some(matches_regex(OBSERVED_AT_WHOLE_SECOND))
    );
    delete_entities(&tenant, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on a geoquery
/// Clause: 5.7.2
#[gtest]
#[tokio::test]
async fn e019_01_05_query_several_entities_based_on_a_geoquery() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &GEO_TERMS).await?;
    create_entity(&broker, &vocabulary, &eiffel_tower_located(&namespace, 1)?).await?;
    create_entity(&broker, &vocabulary, &eiffel_tower_located(&namespace, 2)?).await?;
    let received = broker
        .with_accept(NgsiLdMediaType::JsonLd)
        .with_responses()
        .entities()
        .query()
        .entity_type(namespace.expanded_term("Building")?.as_str())
        .geo_q(geoquery::equals(point(13.3986, 52.5547)?))
        .send()
        .await;
    verify_that!(&received, answered(StatusCode::OK))?;
    let body = json_body(&received?)?;
    let eiffel = |n: u32| -> Result<Value> {
        Ok(json!({
            "id": namespace.urn_text("Building", n),
            "type": namespace.expanded_term("Building")?,
            namespace.expanded_term("name")?: {"type": "Property", "value": "Eiffel Tower"},
            namespace.expanded_term("subCategory")?: {"type": "Property", "value": "tourism"},
            "location": {"type": "GeoProperty", "value": {"type": "Point", "coordinates": [13.3986, 52.5547]}},
            "@context": CORE_CONTEXT.as_str(),
        }))
    };
    expect_that!(
        &body,
        matches_document(json!([eiffel(1)?, eiffel(2)?]))
            .hiding("instanceId")
            .context_rule(ContextRule::AnyCoreContextVersion)
    );
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on scopes
/// Clause: 4.19
/// Corrected: `fix(019_01_06)`: parenthesise the scope conjunction the scope query grammar requires (4.19)
/// Changed: the body is read as the list of entities the query answers, where the case also accepts one bare object
#[gtest]
#[tokio::test]
async fn e019_01_06_query_with_and_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_scoped_pair(&broker, &vocabulary, &namespace).await?;
    let body = scoped_answer(&broker, &namespace, "(/Madrid/Gardens/ParqueNorte;/CompanyA/OrganizationB/UnitC)").await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on scopes
/// Clause: 4.19
#[gtest]
#[tokio::test]
async fn e019_01_06_query_non_empty_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_scoped_pair(&broker, &vocabulary, &namespace).await?;
    let body = scoped_answer(&broker, &namespace, "/#").await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    expect_that!(body.pointer("/1/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on scopes
/// Clause: 4.19
#[gtest]
#[tokio::test]
async fn e019_01_06_query_with_full_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_scoped_pair(&broker, &vocabulary, &namespace).await?;
    let body = scoped_answer(&broker, &namespace, "/Madrid/Gardens/ParqueNorte").await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    expect_that!(body.pointer("/1/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on scopes
/// Clause: 4.19
/// Changed: the body is read as the list of entities the query answers, where the case also accepts one bare object
#[gtest]
#[tokio::test]
async fn e019_01_06_query_with_hash_matching() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_scoped_pair(&broker, &vocabulary, &namespace).await?;
    let body = scoped_answer(&broker, &namespace, "/CompanyA/#").await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(1)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on scopes
/// Clause: 4.19
#[gtest]
#[tokio::test]
async fn e019_01_06_query_with_or_scope() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_scoped_pair(&broker, &vocabulary, &namespace).await?;
    let body = scoped_answer(&broker, &namespace, "/Madrid/Gardens/ParqueNorte,/CompanyA/OrganizationB/UnitC").await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    expect_that!(body.pointer("/1/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}

/// Pins: Check that one can query several entities based on scopes
/// Clause: 4.19
#[gtest]
#[tokio::test]
async fn e019_01_06_query_with_plus_matching() -> Result<()> {
    let namespace = Namespace::fresh();
    let broker = broker::front()?;
    let vocabulary = Vocabulary::register(&broker, &namespace, &["Building"]).await?;
    create_scoped_pair(&broker, &vocabulary, &namespace).await?;
    let body = scoped_answer(&broker, &namespace, "/Madrid/+/ParqueNorte").await?;
    expect_that!(body.as_array().map(Vec::len), some(eq(2)));
    expect_that!(body.pointer("/0/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    expect_that!(body.pointer("/1/type"), some(eq(&json!(namespace.expanded_term("Building")?))));
    delete_entities(&broker, &[&namespace.urn("Building", 1)?, &namespace.urn("Building", 2)?]).await;
    Ok(())
}
