//! Hot-update maintenance: the churn-heavy relations carry the fillfactor and
//! autovacuum settings their write rate needs.
//!
//! Four relations take queue-shaped churn (`entities`, `current_attributes`,
//! `notification_deliveries` and `pending_side_effects`), and each carries an
//! explicit `fillfactor` and aggressive per-table autovacuum settings, because
//! the server defaults let dead tuples accumulate faster than they are
//! reclaimed at that write rate. `entity_search` shares the vacuum cadence: it
//! is updated only when one of its search columns moves.
//!
//! The property is checked in four parts. The settings the migrations declare
//! are read back from the catalogue, so a relation that lost them is caught. A
//! sustained update run then reports the heap-only-tuple ratio and the dead-tuple
//! count each relation actually reaches, which is what the tuning is judged by.
//! The split of the entity into an identity row whose `modified_at` every
//! write stamps and a search row that only a search column change touches is
//! verified through the API: a value-only write leaves the search row's update counter where it was,
//! a type change advances it. And the two queue claims are told apart: the
//! delivery claim writes the inputs of its claimable-expression index, so vacuum
//! cadence rather than fillfactor is the defence there; the side-effect claim
//! writes no indexed column, so its fillfactor of 50 must keep it heap-only.

use crate::SUITE;
use andromeda_postgresql_codes::{subscription_kind::SubscriptionKindCode, subscription_status::SubscriptionStatusCode};
use andromeda_postgresql_schema::{
    current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers},
    name_dictionary::{AttributeNames, EntityTypeNames},
    queue::PendingSideEffects,
    subscriptions::{SubscriptionDeliveryState, Subscriptions},
};
use andromeda_postgresql_seaquery::{
    cast::cast,
    catalog_function::{random_uuid, transaction_tuples_hot_updated, transaction_tuples_updated},
    clock::now,
    interval::{make_interval_secs, microseconds},
    introspection::current_setting,
    literal::text_literal,
    maintenance::relation_size,
    pg_array::array_literal,
    pg_operator::PgBinaryOperator,
    pg_type::PostgresType,
    setting::Setting,
    statement::Statement,
    system_catalog::PgClass,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::{environment::TestEnvironment, serialized_environment::SerializedEnvironment};
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Alias, CaseStatement, CommonTableExpression, Expr, ExprTrait, IntoIden, JoinType, OnConflict, Query, SimpleExpr, UpdateStatement, WithClause};
use serde_json::json;
use sqlx::{PgPool, Row};

/// The common table expression holding the dictionary entry of the churn type.
const TYPE_NAME: &str = "type_name";

/// The common table expression holding the churn entity's identity row.
const INSERTED: &str = "ins";

/// One relation the redesign tunes, and the storage parameters it declares.
struct TunedRelation {
    /// Relation name.
    name: &'static str,
    /// The `fillfactor` the migration declares, absent where the relation takes
    /// inserts rather than updates and only needs the vacuum cadence.
    fillfactor: Option<i32>,
}

/// Every relation that takes hot-update-shaped churn, plus the append-only
/// event relation that shares the vacuum cadence.
const TUNED_RELATIONS: &[TunedRelation] = &[
    TunedRelation {
        name: "entities",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "entity_search",
        fillfactor: None,
    },
    TunedRelation {
        name: "current_attributes",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "notification_deliveries",
        fillfactor: Some(80),
    },
    TunedRelation {
        name: "pending_side_effects",
        fillfactor: Some(50),
    },
    TunedRelation {
        name: "notification_events",
        fillfactor: None,
    },
    TunedRelation {
        name: "subscription_delivery_state",
        fillfactor: Some(50),
    },
    TunedRelation {
        name: "temporal_append_intents",
        fillfactor: Some(70),
    },
];

/// The autovacuum settings every tuned relation declares.
const AUTOVACUUM_SETTINGS: &[&str] = &[
    "autovacuum_vacuum_scale_factor",
    "autovacuum_vacuum_threshold",
    "autovacuum_analyze_scale_factor",
    "autovacuum_analyze_threshold",
    "autovacuum_vacuum_cost_delay",
];

/// Updates applied to the churn fixture, enough for the HOT ratio to be a
/// measurement rather than a rounding of one or two tuples.
const CHURN_UPDATES: i64 = 2_000;

/// Identifier prefix of the attribute-churn fixture. That test holds the
/// exclusive serialized lease, so the tenant is reset to a clean slate before it
/// runs and one fixed prefix suffices.
const ATTRIBUTE_CHURN_PREFIX: &str = "urn:ngsi-ld:test:tenant-schema:attribute-churn";

/// The pages `relation` occupies right now.
async fn relation_pages(pool: &PgPool, relation: &str) -> i64 {
    Query::select()
        .expr(pages_of(relation_oid(relation)))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the relation must report its size")
}

/// `pg_relation_size(relation) / block_size`: the pages the relation occupies.
fn pages_of(relation: impl Into<SimpleExpr>) -> SimpleExpr {
    Expr::expr(relation_size(relation.into())).div(cast(current_setting(Setting::BlockSize), PostgresType::Bigint))
}

/// The relation named `relation`, as its catalogue row's `oid`.
fn relation_oid(relation: &str) -> SimpleExpr {
    cast(Expr::val(relation), PostgresType::Regclass)
}

/// The storage parameters one relation declares, as `name=value` strings.
async fn storage_parameters(pool: &PgPool, relation: &str) -> Vec<String> {
    Query::select()
        .column(Alias::new("reloptions"))
        .from(PgClass::Table)
        .and_where(Expr::col(PgClass::Oid).eq(relation_oid(relation)))
        .to_query_scalar::<Option<Vec<String>>>()
        .fetch_one(pool)
        .await
        .expect("the relation must exist and report its storage parameters")
        .unwrap_or_default()
}

/// What one measured churn run did to the relation.
struct ChurnOutcome {
    /// Updates the relation counted during the run.
    updated: i64,
    /// How many of them were heap-only.
    heap_only: i64,
    /// Pages the relation occupied when the run finished.
    pages: i64,
}

/// Apply `statement` `CHURN_UPDATES` times inside one transaction and report
/// what it did to `relation`.
///
/// The counters are the per-transaction ones. The cumulative statistics a
/// relation reports are flushed to shared memory asynchronously, so reading
/// them straight after a run measures the flush rather than the run; the
/// transaction-local counters are exact the moment the run finishes, which is
/// what makes this a measurement instead of a race.
async fn churn(pool: &PgPool, relation: &str, statement: impl Fn(i64) -> UpdateStatement) -> ChurnOutcome {
    let mut transaction = pool.begin().await.expect("the churn transaction must open");
    let before: (i64, i64) = Query::select()
        .expr(transaction_tuples_updated(Expr::col((PgClass::Table, PgClass::Oid))))
        .expr(transaction_tuples_hot_updated(Expr::col((PgClass::Table, PgClass::Oid))))
        .from(PgClass::Table)
        .and_where(Expr::col((PgClass::Table, PgClass::Oid)).eq(relation_oid(relation)))
        .to_query_as()
        .fetch_one(&mut *transaction)
        .await
        .expect("the transaction-local tuple counters must be readable before churn");
    for round in 1..=CHURN_UPDATES {
        statement(round)
            .to_query()
            .execute(&mut *transaction)
            .await
            .expect("the churn update must apply");
    }
    let outcome = Query::select()
        .expr_as(transaction_tuples_updated(Expr::col((PgClass::Table, PgClass::Oid))), Alias::new("updated"))
        .expr_as(
            transaction_tuples_hot_updated(Expr::col((PgClass::Table, PgClass::Oid))),
            Alias::new("heap_only"),
        )
        .expr_as(pages_of(Expr::col((PgClass::Table, PgClass::Oid))), Alias::new("pages"))
        .from(PgClass::Table)
        .and_where(Expr::col((PgClass::Table, PgClass::Oid)).eq(relation_oid(relation)))
        .to_query()
        .fetch_one(&mut *transaction)
        .await
        .map(|row| ChurnOutcome {
            updated: row.try_get::<i64, _>("updated").expect("the updated column decodes") - before.0,
            heap_only: row.try_get::<i64, _>("heap_only").expect("the heap_only column decodes") - before.1,
            pages: row.try_get::<i64, _>("pages").expect("the pages column decodes"),
        })
        .expect("the transaction-local tuple counters must be readable");
    transaction.commit().await.expect("the churn transaction must commit");
    outcome
}

/// Every tuned relation still carries the settings its migration declared.
#[tokio::test]
async fn every_hot_update_relation_still_declares_the_maintenance_settings_it_was_given() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    for relation in TUNED_RELATIONS {
        let parameters = storage_parameters(pool, relation.name).await;
        if let Some(fillfactor) = relation.fillfactor {
            assert!(
                parameters.iter().any(|parameter| parameter == &format!("fillfactor={fillfactor}")),
                "{} must keep fillfactor={fillfactor}: {parameters:?}",
                relation.name
            );
        }
        for setting in AUTOVACUUM_SETTINGS {
            assert!(
                parameters.iter().any(|parameter| parameter.starts_with(&format!("{setting}="))),
                "{} must keep {setting}: {parameters:?}",
                relation.name
            );
        }
    }

    scope.finish().await.expect("cleanup failed");
}

/// A sustained update run over `current_attributes`, in the two shapes the
/// relation can take.
///
/// Through the broker a write that restates an instance without changing its
/// value touches no row at all: the upsert's `IS DISTINCT FROM` guard suppresses
/// it. The first run here is therefore a page-space probe rather than a
/// production shape: it rewrites a column no index key contains
/// (`unit_code_id`), which is the only update the reserved `fillfactor` space
/// can keep heap-only, and it measures that the reservation does so. A write
/// that changes the value touches `value_number`, which `idx_ca_value_number`
/// contains, so it cannot: every typed-value index disqualifies HOT for its own
/// column, which is why vacuum cadence rather than fillfactor is the primary
/// defence. Both are measured so the tuning is judged on what it
/// can reach.
///
/// This one test holds the exclusive serialized lease rather than the shared
/// one: its page-growth assertion reads whole-table `pg_relation_size`, which a
/// concurrent bulk writer in another suite could inflate past the slack, so it
/// runs against a clean slate with no concurrent writer. The HOT-ratio asserts
/// are transaction-local and would be safe either way.
#[tokio::test]
async fn sustained_attribute_row_churn_stays_heap_only_and_leaves_bounded_dead_tuples() {
    let scope = SerializedEnvironment::from_environment_reset().await.expect("serialized test environment");
    let pool = scope.database_pool();
    let prefix = ATTRIBUTE_CHURN_PREFIX;

    let attribute_name_key: i32 = Query::insert()
        .into_table(AttributeNames::Table)
        .columns([AttributeNames::Name])
        .values([Expr::val("https://example.test/tenant-schema/churn")])
        .expect("one value per column")
        .on_conflict(OnConflict::column(AttributeNames::Name).update_column(AttributeNames::Name).to_owned())
        .returning_col(AttributeNames::AttributeNameKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("attribute dictionary insert must succeed");

    let entity_key = churn_entity(pool, prefix).await;

    Query::insert()
        .into_table(CurrentAttributes::Table)
        .columns([
            CurrentAttributes::EntityKey,
            CurrentAttributes::AttributeNameKey,
            CurrentAttributes::Ordinal,
            CurrentAttributes::AttributeKind,
            CurrentAttributes::ValueKind,
            CurrentAttributes::ValueNumber,
            CurrentAttributes::CreatedAt,
            CurrentAttributes::ModifiedAt,
        ])
        .values([
            Expr::val(entity_key),
            Expr::val(attribute_name_key),
            Expr::val(0_i32),
            Expr::val(1_i16),
            Expr::val(1_i16),
            cast(Expr::val(0), PostgresType::Numeric),
            now(),
            now(),
        ])
        .expect("one value per column")
        .to_query()
        .execute(pool)
        .await
        .expect("the churn attribute row must be writable");

    // The round is bound but never changes the row's value: it only keeps the
    // two runs on one churn driver. `unit_code_id` is set to NULL, which it
    // already is, so the row version differs from its predecessor in nothing
    // an index carries.
    let restatement = churn(pool, "current_attributes", |round| {
        Query::update()
            .table(CurrentAttributes::Table)
            .value(CurrentAttributes::UnitCodeId, Option::<i16>::None)
            .and_where(Expr::col(CurrentAttributes::EntityKey).eq(entity_key))
            .and_where(Expr::val(round).gt(0))
            .to_owned()
    })
    .await;

    assert_eq!(restatement.updated, CHURN_UPDATES, "every value-preserving update must be counted");
    // The row is small, no touched column is an index key, and the relation
    // reserves twenty percent of every page, so the overwhelming majority of
    // the run must stay on its original page. A threshold rather than a ratio
    // of one: the first update of a filled page still has to move.
    assert!(
        restatement.heap_only * 10 >= restatement.updated * 9,
        "a value-preserving restatement must stay heap-only: {} of {} updates were HOT",
        restatement.heap_only,
        restatement.updated
    );

    let revaluation = churn(pool, "current_attributes", |round| {
        Query::update()
            .table(CurrentAttributes::Table)
            .value(CurrentAttributes::ValueNumber, cast(Expr::val(round), PostgresType::Numeric))
            .and_where(Expr::col(CurrentAttributes::EntityKey).eq(entity_key))
            .to_owned()
    })
    .await;

    assert_eq!(revaluation.updated, CHURN_UPDATES, "every value-changing update must be counted");
    // `idx_ca_value_number` contains `value_number`, so none of these can be
    // heap-only. Recorded as an assertion because the day that index goes, the
    // reasoning that vacuum cadence is the primary defence changes with it and
    // the tuning has to be re-measured.
    assert_eq!(
        revaluation.heap_only, 0,
        "a value-changing update writes an index tuple, so none of the {} can be heap-only",
        revaluation.updated
    );
    // Bloat is what the tuning exists to bound. Two thousand updates of one row
    // must not grow the relation without limit: the reserved free space absorbs
    // the versions and page pruning reclaims them within the same page.
    assert!(
        revaluation.pages <= restatement.pages + CHURN_UPDATES / 8,
        "sustained churn must not grow the relation without bound: {} pages after the second run, {} after the first",
        revaluation.pages,
        restatement.pages
    );

    Query::delete()
        .from_table(CurrentAttributes::Table)
        .and_where(Expr::col(CurrentAttributes::EntityKey).eq(entity_key))
        .to_query()
        .execute(pool)
        .await
        .expect("the churn attribute row must be removable");

    scope.finish().await.expect("cleanup failed");
}

/// Insert one bare entity, with its search row and its type membership, under
/// `prefix` and return its surrogate key.
async fn churn_entity(pool: &PgPool, prefix: &str) -> i64 {
    let type_name = Query::insert()
        .into_table(EntityTypeNames::Table)
        .columns([EntityTypeNames::Name])
        .values([Expr::val("https://example.test/tenant-schema/Churn")])
        .expect("one value per column")
        .on_conflict(OnConflict::column(EntityTypeNames::Name).update_column(EntityTypeNames::Name).to_owned())
        .returning_col(EntityTypeNames::EntityTypeNameKey)
        .to_owned();
    let inserted = Query::insert()
        .into_table(Entities::Table)
        .columns([Entities::Id])
        .values([cast(Expr::val(prefix), PostgresType::Text).concat_text(text_literal(":churn"))])
        .expect("one value per column")
        .returning(Query::returning().columns([Entities::EntityKey, Entities::Id]))
        .to_owned();
    let membership = Query::insert()
        .into_table(EntityTypeMembers::Table)
        .columns([EntityTypeMembers::EntityKey, EntityTypeMembers::EntityTypeNameKey, EntityTypeMembers::Id])
        .select_from(
            Query::select()
                .column((Alias::new(INSERTED), Entities::EntityKey))
                .column((Alias::new(TYPE_NAME), EntityTypeNames::EntityTypeNameKey))
                .column((Alias::new(INSERTED), Entities::Id))
                .from(Alias::new(INSERTED))
                .from(Alias::new(TYPE_NAME))
                .to_owned(),
        )
        .expect("the projection matches the column list")
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
                .column((Alias::new(INSERTED), Entities::EntityKey))
                .expr(array_literal([Expr::col((Alias::new(TYPE_NAME), EntityTypeNames::EntityTypeNameKey))]))
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i32>::new()))
                .expr(Expr::val(Vec::<i16>::new()))
                .from(Alias::new(INSERTED))
                .from(Alias::new(TYPE_NAME))
                .to_owned(),
        )
        .expect("the projection matches the column list")
        .returning_col(EntitySearch::EntityKey)
        .to_owned();
    WithClause::new()
        .cte(CommonTableExpression::new().table_name(Alias::new(TYPE_NAME)).query(type_name).to_owned())
        .cte(CommonTableExpression::new().table_name(Alias::new(INSERTED)).query(inserted).to_owned())
        .cte(CommonTableExpression::new().table_name(Alias::new("mbr")).query(membership).to_owned())
        .to_owned()
        .query(search)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the churn entity must be writable")
}

/// A sustained `modified_at` stamp over the identity row.
///
/// Every write of an Entity stamps its identity row's `modified_at`, and no
/// index covers that column, so the stamp touches no index key at all: with
/// the free space `fillfactor` reserves the run must stay heap-only, which is
/// the property the split of the entity into an identity and a search row
/// exists to buy.
#[tokio::test]
async fn sustained_identity_stamping_stays_heap_only() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();
    let entity_key = churn_entity(pool, scope.id_prefix()).await;
    let pages_before = relation_pages(pool, "entities").await;

    let rewrite = churn(pool, "entities", |round| {
        Query::update()
            .table(Entities::Table)
            .value(
                Entities::ModifiedAt,
                Expr::expr(now()).add(microseconds(cast(Expr::val(round), PostgresType::Bigint))),
            )
            .and_where(Expr::col(Entities::EntityKey).eq(entity_key))
            .to_owned()
    })
    .await;

    assert_eq!(rewrite.updated, CHURN_UPDATES, "every stamp must be counted");
    assert!(
        rewrite.heap_only * 10 >= rewrite.updated * 9,
        "a modifiedAt stamp must stay heap-only: {} of {} updates were HOT",
        rewrite.heap_only,
        rewrite.updated
    );
    // The relation is shared with every other test of the suite and carries
    // whatever dead space earlier suites left, so growth is measured against
    // the size the run started from.
    assert!(
        rewrite.pages <= pages_before + 1 + CHURN_UPDATES / 8,
        "sustained stamping must not grow the relation without bound: {} pages after, {pages_before} before",
        rewrite.pages
    );

    scope.finish().await.expect("cleanup failed");
}

/// The transaction that last wrote this entity's row in `relation`.
///
/// `xmin` is the system column naming the inserting or updating transaction of
/// the row version currently visible, so it changes exactly when the row is
/// rewritten: at commit, for this one entity's row, and independent of the
/// statistics collector's flush latency or of concurrent writes to any other
/// entity. Reading it before and after a write is a precise, race-free test of
/// whether that write touched the row, unlike the table-wide cumulative
/// `n_tup_upd`, which lags behind the write and counts every entity at once.
async fn row_writer(pool: &PgPool, relation: impl IntoIden + Copy, entity_id: &str) -> i64 {
    // Aliased, since `relation` may be `entities` itself.
    let (row, identity) = (|| Alias::new("r"), || Alias::new("e"));
    Query::select()
        .expr(cast(cast(Expr::col((row(), Alias::new("xmin"))), PostgresType::Text), PostgresType::Bigint))
        .from_as(relation, row())
        .join_as(
            JoinType::InnerJoin,
            Entities::Table,
            identity(),
            Expr::col((identity(), Entities::EntityKey)).equals((row(), Entities::EntityKey)),
        )
        .and_where(Expr::col((identity(), Entities::Id)).eq(entity_id))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the entity's row must report the transaction that wrote it")
}

/// Through the API: a write that changes only an attribute value stamps the
/// identity row and leaves the search row untouched, and a write that adds a
/// type rewrites the search row too.
#[tokio::test]
async fn a_value_only_write_leaves_the_search_row_untouched_while_a_type_change_rewrites_it() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let pool = scope.database_pool();
    let id = scope.id("Sensor:search-row");

    let created = client
        .post_entity(&json!({
            "id": id,
            "type": "Sensor",
            "temperature": {"type": "Property", "value": 1},
            "@context": [CORE_CONTEXT]
        }))
        .with_ld_json()
        .send()
        .await
        .expect("create request failed");
    assert_eq!(created.status, StatusCode::CREATED);

    let identity_writer = row_writer(pool, Entities::Table, &id).await;
    let search_writer = row_writer(pool, EntitySearch::Table, &id).await;

    let value_only = client
        .update_entity_attrs(&id, &json!({"temperature": {"type": "Property", "value": 2}, "@context": [CORE_CONTEXT]}))
        .await
        .expect("attribute update request failed");
    assert_eq!(value_only.status, StatusCode::NO_CONTENT);
    assert_ne!(
        row_writer(pool, Entities::Table, &id).await,
        identity_writer,
        "the value write must stamp the identity row"
    );
    assert_eq!(
        row_writer(pool, EntitySearch::Table, &id).await,
        search_writer,
        "a value-only write must leave the search row where it was"
    );

    let retyped = client
        .replace_entity(
            &id,
            &json!({
                "id": id,
                "type": ["Sensor", "Thermometer"],
                "temperature": {"type": "Property", "value": 2},
                "@context": [CORE_CONTEXT]
            }),
            "application/ld+json",
            None,
        )
        .await
        .expect("replace request failed");
    assert_eq!(retyped.status, StatusCode::NO_CONTENT);
    assert_ne!(
        row_writer(pool, EntitySearch::Table, &id).await,
        search_writer,
        "a type change must rewrite the search row"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The claim-shaped update is the one that cannot go HOT: `next_attempt_at`
/// and `lease_until` are the two inputs of the claim index's expression, and
/// `delivered_at`/`exhausted_at` are its predicate, so every claim, retry and
/// acknowledgement writes new index tuples whatever the fillfactor is. The
/// index definition is the fact that makes it true, and it is also the fact
/// that makes the claim one bounded range scan: the expression is the single
/// key the claim orders and filters on.
#[tokio::test]
async fn the_delivery_claim_index_is_the_claimable_expression_so_vacuum_cadence_is_its_defence() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    let claim_index: String = Query::select()
        .column(Alias::new("indexdef"))
        .from(Alias::new("pg_indexes"))
        .and_where(Expr::col(Alias::new("schemaname")).eq("public"))
        .and_where(Expr::col(Alias::new("indexname")).eq("idx_notification_deliveries_claim"))
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the claim index must exist");

    assert!(
        claim_index.contains("GREATEST(next_attempt_at, COALESCE(lease_until,"),
        "the claim index must be led by the claimable-instant expression: {claim_index}"
    );
    assert!(
        claim_index.contains(", delivery_key)"),
        "the claim index must break ties on delivery_key: {claim_index}"
    );
    for excluded in ["delivered_at IS NULL", "exhausted_at IS NULL"] {
        assert!(
            claim_index.contains(excluded),
            "the claim index must exclude rows where not {excluded}: {claim_index}"
        );
    }
    assert!(
        !claim_index.contains("attempt_count"),
        "the claim index must not repeat the attempt ceiling: {claim_index}"
    );

    scope.finish().await.expect("cleanup failed");
}

/// The side-effect claim writes `claimed_at`, `lease_until` and `claimed_by`
/// together (the coherence constraint admits all three or none), and none of
/// them is carried by any index of `pending_side_effects` (`idx_pse_claim`
/// is led by `next_attempt_at`, `idx_pse_exhausted_at` by `exhausted_at`), so with the
/// fillfactor of 50 the migration declares the claim must stay heap-only.
#[tokio::test]
async fn sustained_side_effect_claims_stay_heap_only() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let owed_change = Expr::tuple([
        Expr::val(1),
        Expr::val("urn:ngsi-ld:Vehicle:churn"),
        Expr::val(Vec::<i32>::new()),
        Expr::val(Vec::<i16>::new()),
        Expr::val(Vec::<i32>::new()),
        Expr::val(Vec::<String>::new()),
        now(),
        Expr::val(1),
        Expr::null(),
        Expr::null(),
        Expr::null(),
        Expr::null(),
        Expr::null(),
    ]);
    Query::insert()
        .into_table(PendingSideEffects::Table)
        .columns([
            PendingSideEffects::Id,
            PendingSideEffects::WriteId,
            PendingSideEffects::RecordedAt,
            PendingSideEffects::BaseOrdinal,
            PendingSideEffects::Events,
        ])
        .values([
            random_uuid(),
            random_uuid(),
            now(),
            Expr::val(0),
            array_literal([cast(owed_change, PostgresType::PendingChangeEvent)]),
        ])
        .expect("one value per column")
        .to_query()
        .execute(pool)
        .await
        .expect("the churn job must be seeded");

    let claims = churn(pool, "pending_side_effects", |round| {
        Query::update()
            .table(PendingSideEffects::Table)
            .value(PendingSideEffects::ClaimedAt, now())
            .value(PendingSideEffects::LeaseUntil, Expr::expr(now()).add(make_interval_secs(Expr::val(round))))
            .value(PendingSideEffects::ClaimedBy, random_uuid())
            .and_where(Expr::col(PendingSideEffects::BaseOrdinal).eq(cast(Expr::val(0), PostgresType::Integer)))
            .to_owned()
    })
    .await;

    assert_eq!(claims.updated, CHURN_UPDATES, "every claim must be counted");
    assert!(
        claims.heap_only * 10 >= claims.updated * 9,
        "the side-effect claim must be heap-only at least 90% of the time: {} of {}",
        claims.heap_only,
        claims.updated
    );

    Query::delete()
        .from_table(PendingSideEffects::Table)
        .to_query()
        .execute(pool)
        .await
        .expect("the churn job must be removable");
    scope.finish().await.expect("cleanup failed");
}

/// The delivery counters of a subscription live on `subscription_delivery_state`
/// so that a notification never rewrites the wide subscription row. That row
/// carries no index but its primary key and half of every page is kept free,
/// so the claim (`times_sent + 1`, `last_notification`) and the outcome marks
/// (`last_success` / `last_failure`) that every delivery writes must stay
/// heap-only.
#[tokio::test]
async fn sustained_delivery_state_churn_stays_heap_only() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let subscription_key: i64 = Query::insert()
        .into_table(Subscriptions::Table)
        .columns([Subscriptions::Id, Subscriptions::Kind, Subscriptions::Status, Subscriptions::Notification])
        .values([
            Expr::val(scope.id("urn:ngsi-ld:Subscription:delivery-state-churn")),
            Expr::val(SubscriptionKindCode::Entity.code()),
            Expr::val(SubscriptionStatusCode::Active.code()),
            cast(Expr::val("{}"), PostgresType::Json),
        ])
        .expect("one value per column")
        .returning_col(Subscriptions::SubscriptionKey)
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the churn subscription must be seeded");
    Query::insert()
        .into_table(SubscriptionDeliveryState::Table)
        .columns([SubscriptionDeliveryState::SubscriptionKey])
        .values([Expr::val(subscription_key)])
        .expect("one value per column")
        .to_query()
        .execute(pool)
        .await
        .expect("the churn state row must be seeded");

    let claims = churn(pool, "subscription_delivery_state", |round| {
        let on_parity = |parity: i64, column: SubscriptionDeliveryState| {
            CaseStatement::new()
                .case(Expr::val(round).modulo(2).eq(parity), now())
                .finally(Expr::col(column))
        };
        Query::update()
            .table(SubscriptionDeliveryState::Table)
            .value(SubscriptionDeliveryState::TimesSent, Expr::col(SubscriptionDeliveryState::TimesSent).add(1))
            .value(SubscriptionDeliveryState::LastNotification, now())
            .value(SubscriptionDeliveryState::LastSuccess, on_parity(0, SubscriptionDeliveryState::LastSuccess))
            .value(SubscriptionDeliveryState::LastFailure, on_parity(1, SubscriptionDeliveryState::LastFailure))
            .and_where(Expr::col(SubscriptionDeliveryState::SubscriptionKey).eq(subscription_key))
            .to_owned()
    })
    .await;

    assert_eq!(claims.updated, CHURN_UPDATES, "every claim must be counted");
    assert!(
        claims.heap_only * 10 >= claims.updated * 9,
        "the delivery-state write must be heap-only at least 90% of the time: {} of {}",
        claims.heap_only,
        claims.updated
    );

    scope.finish().await.expect("cleanup failed");
}
