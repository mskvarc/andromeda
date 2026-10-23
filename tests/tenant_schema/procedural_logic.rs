//! No database procedural logic beyond what a constraint or an index needs: the
//! migrated tenant carries no trigger or function acting on the broker's behalf.
//!
//! Every multi-step flow the broker performs is a Rust-sequenced transaction:
//! nothing in the tenant schema derives or filters on the broker's behalf,
//! because logic living in PL/pgSQL is logic no type, no test and no reviewer
//! of the broker ever sees. The only triggers are the statement-level
//! references of `current_attributes`, which do what a foreign key would (a
//! referential check and the cascade from `entities`), and the only functions
//! are those triggers' and the immutable ones an index expression calls. The
//! catalogue checks below state that as a property of the migrated schema, and
//! the deletion flows show the same flows producing correct results.

use crate::SUITE;
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities},
    temporal::{AttributeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    literal::{int_literal, text_literal},
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    statement::Statement,
    system_catalog::{PgClass, PgNamespace},
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Alias, Expr, ExprTrait, Order, Query};
use serde_json::json;

/// The triggers the schema tolerates, as `table.trigger`: the referential
/// checks and the cascade standing in for the foreign keys of
/// `current_attributes`.
const TOLERATED_TRIGGERS: [&str; 5] = [
    "current_attributes.trg_current_attributes_entities_insert",
    "current_attributes.trg_current_attributes_entities_update",
    "current_attributes.trg_current_attributes_name_keys_insert",
    "current_attributes.trg_current_attributes_name_keys_update",
    "entities.trg_entities_current_attributes_cascade",
];

/// The functions the schema tolerates, one entry per overload: the tolerated
/// triggers' functions and the immutable wrapper a generated column needs to
/// cast a geometry to a geography.
const TOLERATED_FUNCTIONS: [&str; 4] = [
    "cascade_entity_current_attributes",
    "check_current_attribute_entities",
    "check_current_attribute_name_keys",
    "geom_to_geog",
];

/// A temporal entity with two instances of one attribute.
fn recorded_device(id: &str) -> serde_json::Value {
    json!({
        "id": id,
        "type": "Device",
        "temperature": [
            {"type": "Property", "value": 1, "observedAt": "2026-01-01T00:00:00Z"},
            {"type": "Property", "value": 2, "observedAt": "2026-01-01T01:00:00Z"}
        ],
        "@context": [CORE_CONTEXT]
    })
}

#[tokio::test]
async fn the_migrated_tenant_carries_only_reference_triggers_and_index_functions() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let trigger = |column: &str| Expr::col((Alias::new("pg_trigger"), Alias::new(column)));
    let triggers: Vec<String> = Query::select()
        .expr(
            cast(Expr::col((PgClass::Table, PgClass::Relname)), PostgresType::Text)
                .concat_text(text_literal("."))
                .concat_text(cast(trigger("tgname"), PostgresType::Text)),
        )
        .from(Alias::new("pg_trigger"))
        .inner_join(
            PgClass::Table,
            Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_trigger"), Alias::new("tgrelid"))),
        )
        .inner_join(
            PgNamespace::Table,
            Expr::col((PgNamespace::Table, PgNamespace::Oid)).equals((PgClass::Table, PgClass::Relnamespace)),
        )
        .and_where(trigger("tgisinternal").not())
        .and_where(Expr::col((PgNamespace::Table, PgNamespace::Nspname)).eq("public"))
        .order_by_expr(int_literal(1), Order::Asc)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the trigger catalogue must be readable");
    assert_eq!(triggers, TOLERATED_TRIGGERS, "the tenant schema declares only the reference triggers");

    // Functions an extension installed belong to that extension, not to the
    // broker, so they are excluded through their dependency record.
    let procedure = |column: &str| Expr::col((Alias::new("pg_proc"), Alias::new(column)));
    let functions: Vec<String> = Query::select()
        .expr(procedure("proname"))
        .from(Alias::new("pg_proc"))
        .inner_join(
            PgNamespace::Table,
            Expr::col((PgNamespace::Table, PgNamespace::Oid)).equals((Alias::new("pg_proc"), Alias::new("pronamespace"))),
        )
        .and_where(Expr::col((PgNamespace::Table, PgNamespace::Nspname)).eq("public"))
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(Alias::new("pg_depend"))
                    .and_where(Expr::col((Alias::new("pg_depend"), Alias::new("objid"))).equals((Alias::new("pg_proc"), Alias::new("oid"))))
                    .and_where(Expr::col((Alias::new("pg_depend"), Alias::new("deptype"))).eq(text_literal("e")))
                    .to_owned(),
            )
            .not(),
        )
        .order_by_expr(procedure("proname"), Order::Asc)
        .to_query_scalar()
        .fetch_all(pool)
        .await
        .expect("the function catalogue must be readable");
    assert_eq!(functions, TOLERATED_FUNCTIONS, "the schema declares only the tolerated functions");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.6: Entity deletion removes the current Entity and its attribute
/// instances while the temporal evolution stays queryable.
#[tokio::test]
async fn core_entity_deletion_removes_current_state_and_leaves_temporal_evolution() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:core-deletion");

    // A temporal write establishes current state as well as the evolution, so
    // this one request is the whole fixture.
    let created = client
        .create_temporal_entity(&recorded_device(&id))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let deleted = client.delete_entity(&id).await.expect("delete entity request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);

    let entities: i64 = Query::select()
        .expr(count_all())
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(id.as_str()))
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the entity relation must be readable");
    assert_eq!(entities, 0, "current state is gone");
    let instances: i64 = Query::select()
        .expr(count_all())
        .from(CurrentAttributes::Table)
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((CurrentAttributes::Table, CurrentAttributes::EntityKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id.as_str()))
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the attribute instances must be readable");
    assert_eq!(instances, 0, "the attribute instances went with it");

    let temporal = client.get_temporal_entity(&id, "", None).await.expect("temporal retrieve request failed");
    assert_eq!(
        temporal.status,
        StatusCode::OK,
        "the temporal evolution outlives Entity deletion: {:?}",
        temporal.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.16: Temporal Evolution deletion removes every stream, history and
/// scope row of the temporal entity and leaves current state untouched.
#[tokio::test]
async fn temporal_evolution_deletion_removes_every_history_row_and_leaves_current_state() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:temporal-deletion");

    // A temporal write establishes current state as well as the evolution, so
    // this one request is the whole fixture.
    let created = client
        .create_temporal_entity(&recorded_device(&id))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let deleted = client.delete_temporal_entity(&id).await.expect("delete temporal entity request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete: {:?}", deleted.body);

    let orphan_history: i64 = Query::select()
        .expr(count_all())
        .from(AttributeHistory::Table)
        .inner_join(
            TemporalAttributeStreams::Table,
            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey)).equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
        )
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((TemporalAttributeStreams::Table, TemporalAttributeStreams::EntityKey)),
        )
        .and_where(Expr::col((Entities::Table, Entities::Id)).eq(id.as_str()))
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the history relation must be readable");
    assert_eq!(orphan_history, 0, "no history row survives temporal deletion");
    let temporal_entities: i64 = Query::select()
        .expr(count_all())
        .from(Entities::Table)
        .and_where(Expr::col(Entities::Id).eq(id.as_str()))
        .and_where(Expr::col(Entities::EvolutionCreatedAt).is_not_null())
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the temporal entity relation must be readable");
    assert_eq!(temporal_entities, 0, "the temporal entity is gone");

    let retrieved = client
        .get_entity(&id, "", "application/json", None)
        .await
        .expect("retrieve entity request failed");
    assert_eq!(
        retrieved.status,
        StatusCode::OK,
        "current state is untouched by temporal deletion: {:?}",
        retrieved.body
    );

    scope.finish().await.expect("cleanup failed");
}

/// Clause 5.6.15: instance deletion removes exactly the named instance, and it
/// is removed physically rather than hidden behind a soft-deleted row.
#[tokio::test]
async fn temporal_instance_deletion_removes_exactly_the_named_instance() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let id = scope.id("Device:instance-deletion");

    let created = client
        .create_temporal_entity(&recorded_device(&id))
        .await
        .expect("create temporal entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create: {:?}", created.body);

    let retrieved = client.get_temporal_entity(&id, "", None).await.expect("temporal retrieve request failed");
    assert_eq!(retrieved.status, StatusCode::OK, "retrieve: {:?}", retrieved.body);
    let body = retrieved.json_value().expect("temporal entity must be valid JSON");
    let instances = body["temperature"].as_array().expect("two recorded instances");
    assert_eq!(instances.len(), 2, "the fixture records two instances: {body}");
    let doomed = instances[0]["instanceId"].as_str().expect("an instance identifier").to_owned();
    let surviving = instances[1]["instanceId"].as_str().expect("an instance identifier").to_owned();

    let deleted = client
        .raw(
            reqwest::Method::DELETE,
            &format!(
                "/ngsi-ld/v1/temporal/entities/{}/attrs/temperature/{}",
                urlencoding::encode(&id),
                urlencoding::encode(&doomed)
            ),
        )
        .send()
        .await
        .expect("delete instance request failed");
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "delete instance: {:?}", deleted.body);

    let after = client.get_temporal_entity(&id, "", None).await.expect("temporal retrieve request failed");
    assert_eq!(after.status, StatusCode::OK, "retrieve: {:?}", after.body);
    let after_body = after.json_value().expect("temporal entity must be valid JSON");
    let remaining: Vec<&str> = after_body["temperature"]
        .as_array()
        .expect("one remaining instance")
        .iter()
        .filter_map(|instance| instance["instanceId"].as_str())
        .collect();
    assert_eq!(remaining, vec![surviving.as_str()], "exactly the named instance is gone: {after_body}");

    scope.finish().await.expect("cleanup failed");
}
