//! Empty Entity type: an empty type array is refused by the API and the schema.
//!
//! An Entity always has at least one type (clause 4.5.1). Rust rejects the
//! empty array at the API boundary, and `chk_entity_search_entity_type_name_keys_present`
//! refuses it again for any writer that bypasses the API.

use andromeda_postgresql_schema::{
    current::{Entities, EntitySearch},
    name_dictionary::EntityTypeNames,
};
use andromeda_postgresql_seaquery::{pg_array::any, statement::Statement};
use andromeda_test_fixtures::context::TEST_SUITE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Alias, CommonTableExpression, Expr, ExprTrait, Order, Query, WithClause};
use serde_json::json;

/// Expanded Building type of the ETSI test-suite context.
const BUILDING_TYPE: &str = "https://ngsi-ld-test-suite/context#Building";

#[tokio::test]
async fn current_projection_rejects_an_entity_with_an_empty_type_array() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:empty-type");

    let entity = json!({
        "id": id,
        "type": [],
        "temperature": {"type": "Property", "value": 1.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let response = client.post_entity(&entity).send().await.expect("create entity request failed");
    assert_eq!(
        response.status,
        StatusCode::BAD_REQUEST,
        "an Entity with no type must be refused: {:?}",
        response.body
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_database_refuses_a_typeless_entity_row() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let id = scope.id("Building:empty-type-sql");

    let entity = Query::insert()
        .into_table(Entities::Table)
        .columns([Entities::Id])
        .values([Expr::val(id.as_str())])
        .expect("one value per column")
        .returning_col(Entities::EntityKey)
        .to_owned();
    let search = Query::insert()
        .into_table(EntitySearch::Table)
        .columns([
            EntitySearch::EntityKey,
            EntitySearch::EntityTypeNameKeys,
            EntitySearch::AttributeNameKeys,
            EntitySearch::AttributeInstanceNameKeys,
            EntitySearch::AttributeInstanceKinds,
        ])
        .select_from(
            Query::select()
                .column(Entities::EntityKey)
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i16>::new()))
                .from(Alias::new("ins"))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .to_owned();
    let failure = WithClause::new()
        .cte(CommonTableExpression::new().table_name(Alias::new("ins")).query(entity).to_owned())
        .to_owned()
        .query(search)
        .to_query()
        .execute(scope.database_pool())
        .await
        .expect_err("a typeless Entity row must be refused");
    let database_error = failure.as_database_error().expect("the refusal must come from the database");
    assert_eq!(
        database_error.code().as_deref(),
        Some("23514"),
        "the refusal must be the non-empty types check: {database_error}"
    );

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn current_projection_persists_every_type_of_a_multi_type_entity() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Building:multi-type");

    let entity = json!({
        "id": id,
        "type": ["Building", "TouristDestination"],
        "temperature": {"type": "Property", "value": 1.0},
        "@context": [TEST_SUITE_CONTEXT]
    });
    let created = client.post_entity(&entity).send().await.expect("create entity failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let types: Vec<String> = Query::select()
        .column((EntityTypeNames::Table, EntityTypeNames::Name))
        .from(Entities::Table)
        .inner_join(
            EntitySearch::Table,
            Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((Entities::Table, Entities::EntityKey)),
        )
        .inner_join(
            EntityTypeNames::Table,
            Expr::col((EntityTypeNames::Table, EntityTypeNames::EntityTypeNameKey)).eq(any(Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys)))),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id.as_str()))
        .order_by((EntityTypeNames::Table, EntityTypeNames::Name), Order::Asc)
        .to_query_scalar()
        .fetch_all(scope.database_pool())
        .await
        .expect("types must be readable");
    assert!(
        types.contains(&BUILDING_TYPE.to_owned()),
        "the expanded Building type must be stored: {types:?}"
    );
    assert_eq!(types.len(), 2, "both declared types must be stored: {types:?}");

    scope.finish().await.expect("cleanup failed");
}
