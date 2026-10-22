//! A clone whose copy runs long stays live (clause 5.16.2.4).
//!
//! Clause 5.16.2.4 copies the source in the background and sets the clone's
//! status once all information has been stored; clause 5.16.3.4 retrieves that
//! status at any time and clause 5.16.5.4 deletes a Snapshot without
//! precondition. A copy of a large source outlasts the preparation lease's
//! renewal period, so each case here makes the copy wait on a lock the test
//! holds on one source relation: the lock graph of a long copy, without the
//! long copy. The stack runs a 30 s lease, renewed every 15 s, and several
//! processes over one database, any of which may claim the clone, so the copy
//! is found through the lock it waits on, never through the process that runs
//! it.

use crate::support::{
    REPLICA_BASE,
    broker_url,
    clean_database,
    client,
    create_entities_batch,
    create_snapshot_raw,
    published_branch_schema,
    snapshot_key,
    wait_for_snapshot_terminal,
};
use andromeda_postgresql_schema::snapshot::{SnapshotNotificationOutbox, SnapshotPreparation, Snapshots};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    clock::now,
    command::Command,
    interval::make_interval_secs,
    introspection::current_database,
    maintenance::to_regclass,
    pg_type::PostgresType,
    relation_lock::{LockMode, RelationLock},
    scalar_subquery::scalar_subquery,
    statement::Statement,
    system_catalog::{PgClass, PgDatabase, PgNamespace, PgStatActivity},
    transaction::{Isolation, TransactionStart},
};
use andromeda_test_harness::{configuration::RUNTIME_BROKER_BASE as BASE_URL, discovery_census::census_drift};
use reqwest::{Client, StatusCode};
use sea_query::{Alias, Expr, ExprTrait, LockBehavior, LockType, Query, SelectStatement, TableName};
use serde_json::{Value, json};
use serial_test::serial;
use sqlx::{PgPool, Postgres, Transaction, postgres::PgPoolOptions};
use std::{collections::BTreeSet, time::Duration};
use tokio::time::{Instant, sleep, sleep_until, timeout};

const CORE_CONTEXT: &str = "https://uri.etsi.org/ngsi-ld/v1/ngsi-ld-core-context-v1.9.jsonld";
const CAR: &str = "https://example.org/clone-liveness/CloneCar";
const SPEED: &str = "https://example.org/clone-liveness/speed";
/// The cars the source captures.
const CARS: usize = 40;
/// Past the 15 s renewal of the stack's 30 s preparation lease.
const PAST_RENEWAL: Duration = Duration::from_secs(18);
/// How long a status read may take while a copy runs.
const STATUS_DEADLINE: Duration = Duration::from_secs(2);
/// How long the clone has to end once the test releases its lock.
const TERMINAL_DEADLINE: Duration = Duration::from_secs(20);

/// The API base of the primary broker process, the suite's default broker.
fn primary() -> String {
    BASE_URL.to_string()
}

fn car(n: usize) -> Value {
    json!({"id": format!("urn:ngsi-ld:CloneCar:{n}"), "type": CAR, SPEED: {"type": "Property", "value": n}, "@context": CORE_CONTEXT})
}

/// Capture [`CARS`] cars into `source` and return its published branch schema.
async fn source_of_cars(pool: &PgPool, source: &str) -> String {
    let cars: Vec<Value> = (1..=CARS).map(car).collect();
    let (status, body) = create_entities_batch(&client(), &cars).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{body}");
    let capture = json!({
        "id": source,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": CAR}]}]
    });
    let (status, _, problem) = create_snapshot_raw(&client(), &capture).await;
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), source, TERMINAL_DEADLINE).await["snapshotStatus"],
        "success"
    );
    published_branch_schema(pool, source).await
}

/// A source relation the test holds `ACCESS EXCLUSIVE`, on a connection of its
/// own, so a copy reaching that relation waits until it is released.
struct RelationHold {
    transaction: Transaction<'static, Postgres>,
}

impl RelationHold {
    /// Commit the holding transaction, letting the copy proceed.
    async fn release(self) {
        self.transaction.commit().await.expect("the test releases the relation");
    }
}

/// Hold `schema.table` against every other backend until it is released.
async fn hold_relation(pool: &PgPool, schema: &str, table: &str) -> RelationHold {
    // clone: the dedicated pool needs its own copy of the connect options, which `connect_options` only lends.
    let options = pool.connect_options().as_ref().clone();
    let holder = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("the holding connection");
    let mut transaction = TransactionStart::at(Isolation::ReadCommitted)
        .begin_in(&holder)
        .await
        .expect("the holding transaction");
    RelationLock::waiting(vec![TableName::from((Alias::new(schema), Alias::new(table)))], LockMode::AccessExclusive)
        .to_query()
        .execute(&mut *transaction)
        .await
        .expect("the test holds the relation");
    RelationHold { transaction }
}

/// How many backends wait for a lock on `schema.table`.
async fn waiting_on(pool: &PgPool, schema: &str, table: &str) -> i64 {
    Query::select()
        .expr(count_all())
        .from(Alias::new("pg_locks"))
        .and_where(Expr::col(Alias::new("granted")).not())
        .and_where(Expr::col(Alias::new("relation")).eq(to_regclass(Expr::val(format!("{schema}.{table}")))))
        .and_where(
            Expr::col(Alias::new("database")).eq(scalar_subquery(
                Query::select()
                    .column(Alias::new("oid"))
                    .from(PgDatabase::Table)
                    .and_where(Expr::col(PgDatabase::Datname).eq(current_database()))
                    .to_owned(),
            )),
        )
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the lock waits on the relation")
}

/// Wait until a copy waits on the relation the test holds.
async fn until_the_copy_waits_on(pool: &PgPool, schema: &str, table: &str) {
    timeout(Duration::from_secs(10), async {
        while waiting_on(pool, schema, table).await == 0 {
            sleep(Duration::from_millis(100)).await;
        }
    })
    .await
    .expect("the clone's copy reaches the held relation");
}

/// Clone `source` into `clone` through the broker at `base`.
async fn start_clone(base: &str, source: &str, clone: &str) {
    let response = client()
        .post(format!("{base}/snapshots/{source}/clone"))
        .header("Content-Type", "application/json")
        .json(&json!({"id": clone, "type": "Snapshot"}))
        .send()
        .await
        .expect("clone Snapshot request");
    let status = response.status();
    let problem: Value = response.json().await.unwrap_or_default();
    pretty_assertions::assert_eq!(status, StatusCode::CREATED, "{problem}");
}

/// A client whose every request gives up after `deadline`.
fn client_within(deadline: Duration) -> Client {
    Client::builder().timeout(deadline).build().expect("a client with a deadline")
}

/// The status of `snapshot` read through the broker at `base` within
/// [`STATUS_DEADLINE`].
async fn status_within_deadline(base: &str, snapshot: &str) -> Value {
    let response = client_within(STATUS_DEADLINE)
        .get(format!("{base}/snapshots/{snapshot}"))
        .send()
        .await
        .map_err(|error| format!("through {base}: {error}"))
        .expect("the status read answers within its deadline");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    response.json().await.expect("the Snapshot body")
}

/// The ids of the cars `snapshot` holds, read through the broker at `base`.
async fn car_ids(base: &str, snapshot: &str) -> BTreeSet<String> {
    let response = client()
        .get(format!("{base}/entities?type={CAR}&limit=100"))
        .header("NGSILD-Snapshot", snapshot)
        .send()
        .await
        .expect("Snapshot-scoped Entity query");
    pretty_assertions::assert_eq!(response.status(), StatusCode::OK);
    let entities: Vec<Value> = response.json().await.expect("the Entities");
    entities.iter().filter_map(|entity| entity["id"].as_str().map(str::to_owned)).collect()
}

/// The ids of the captured cars.
fn captured_ids() -> BTreeSet<String> {
    (1..=CARS).map(|n| format!("urn:ngsi-ld:CloneCar:{n}")).collect()
}

/// Backends of this database idle inside a transaction over a branch statement
/// for longer than 3 s: a copy nobody resumes.
async fn stalled_copies(pool: &PgPool) -> i64 {
    Query::select()
        .expr(count_all())
        .from(PgStatActivity::Table)
        .and_where(Expr::col(PgStatActivity::Datname).eq(current_database()))
        .and_where(Expr::col(Alias::new("state")).eq("idle in transaction"))
        .and_where(Expr::col(Alias::new("query")).like("%snap\\_%"))
        .and_where(
            Expr::expr(now())
                .sub(Expr::col(Alias::new("state_change")))
                .gt(make_interval_secs(Expr::val(3.0))),
        )
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the backend activity")
}

/// The preparation row's claimant of the Snapshot keyed `key`, as text.
fn claimant_statement(key: i64) -> SelectStatement {
    Query::select()
        .expr(cast(Expr::col(SnapshotPreparation::ClaimedBy), PostgresType::Text))
        .from(SnapshotPreparation::Table)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(key))
        .to_owned()
}

/// The clone ended `success` and holds exactly the captured cars, with a
/// census that counts them.
async fn assert_complete_clone(pool: &PgPool, clone: &str) {
    pretty_assertions::assert_eq!(
        wait_for_snapshot_terminal(&client(), clone, TERMINAL_DEADLINE).await["snapshotStatus"],
        "success"
    );
    pretty_assertions::assert_eq!(car_ids(&primary(), clone).await, captured_ids());
    let schema = published_branch_schema(pool, clone).await;
    pretty_assertions::assert_eq!(census_drift(pool, &schema).await.expect("the census comparison"), Vec::<String>::new());
}

/// Delete `clone` through `delete_base` while its copy waits past the lease
/// renewal, then check that nothing of it remains.
async fn assert_delete_during_copy_completes(pool: &PgPool, clone_base: &str, delete_base: &str, clone: &str) {
    let source = "urn:ngsi-ld:Snapshot:clone-liveness-delete-source";
    let schema = source_of_cars(pool, source).await;
    let hold = hold_relation(pool, &schema, "entity_search").await;
    let cloned_at = Instant::now();
    start_clone(clone_base, source, clone).await;
    until_the_copy_waits_on(pool, &schema, "entity_search").await;
    let key = snapshot_key(pool, clone).await;

    let delete_url = format!("{delete_base}/snapshots/{clone}");
    let deletion = tokio::spawn(async move { client_within(Duration::from_secs(40)).delete(delete_url).send().await });
    sleep_until(cloned_at + PAST_RENEWAL).await;
    let released = Instant::now();
    hold.release().await;

    let deleted = deletion.await.expect("the delete task").expect("the delete answers");
    pretty_assertions::assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    assert!(
        released.elapsed() < Duration::from_secs(10),
        "the delete answered {:?} after the release",
        released.elapsed()
    );
    let primary = primary();
    for base in [primary.as_str(), REPLICA_BASE] {
        let response = client().get(format!("{base}/snapshots/{clone}")).send().await.expect("get Snapshot");
        pretty_assertions::assert_eq!(response.status(), StatusCode::NOT_FOUND, "through {base}");
        let problem: Value = response.json().await.expect("ProblemDetails");
        pretty_assertions::assert_eq!(problem["type"], "https://uri.etsi.org/ngsi-ld/errors/ResourceNotFound");
    }
    let branch_schemas = [format!("snap_{key}"), format!("snap_{key}_stg")];
    let schemas: i64 = Query::select()
        .expr(count_all())
        .from(PgNamespace::Table)
        .and_where(Expr::col(PgNamespace::Nspname).is_in(branch_schemas.iter().map(String::as_str)))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the branch schemas");
    pretty_assertions::assert_eq!(schemas, 0, "no branch schema of the clone remains");
    sleep(Duration::from_secs(5)).await;
    let remains: (i64, i64) = Query::select()
        .expr(scalar_subquery(
            Query::select()
                .expr(count_all())
                .from(SnapshotPreparation::Table)
                .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(key))
                .to_owned(),
        ))
        .expr(scalar_subquery(
            Query::select()
                .expr(count_all())
                .from(SnapshotNotificationOutbox::Table)
                .and_where(Expr::col(SnapshotNotificationOutbox::SnapshotKey).eq(key))
                .to_owned(),
        ))
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the clone's control rows");
    pretty_assertions::assert_eq!(remains, (0, 0), "no preparation row and no status notification for the deleted clone");
    let locks_left: i64 = Query::select()
        .expr(count_all())
        .from(Alias::new("pg_locks"))
        .inner_join(
            PgClass::Table,
            Expr::col((PgClass::Table, PgClass::Oid)).equals((Alias::new("pg_locks"), Alias::new("relation"))),
        )
        .inner_join(
            PgNamespace::Table,
            Expr::col((PgNamespace::Table, PgNamespace::Oid)).equals((PgClass::Table, PgClass::Relnamespace)),
        )
        .and_where(Expr::col((PgNamespace::Table, PgNamespace::Nspname)).is_in(branch_schemas.iter().map(String::as_str)))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the locks on the clone's schemas");
    pretty_assertions::assert_eq!(locks_left, 0, "no backend holds or waits on a lock in the clone's schemas");
}

/// Clause 5.16.2.4: a clone whose copy outlasts the lease renewal period still
/// completes once its copy can proceed, and no copy is left idle inside its
/// transaction.
#[tokio::test]
#[serial(snapshot)]
async fn a_clone_whose_copy_outlasts_the_lease_renewal_period_completes() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-liveness-source";
    let clone = "urn:ngsi-ld:Snapshot:clone-liveness-a";
    let schema = source_of_cars(pool, source).await;
    let hold = hold_relation(pool, &schema, "entity_search").await;

    start_clone(&primary(), source, clone).await;
    until_the_copy_waits_on(pool, &schema, "entity_search").await;
    sleep(Duration::from_secs(20)).await;
    hold.release().await;

    let watched = Instant::now();
    while watched.elapsed() < Duration::from_secs(5) {
        pretty_assertions::assert_eq!(stalled_copies(pool).await, 0, "a copy is idle in its transaction");
        sleep(Duration::from_millis(250)).await;
    }
    assert_complete_clone(pool, clone).await;

    environment.reset_and_finish().await.expect("reset");
}

/// Clause 5.16.3.4 retrieves a Snapshot's status at any time, and a read stamps
/// lastUsedAt (clause 4.8, Table 5.2.41-2): a status read never waits for the
/// copy of the clone it reads.
#[tokio::test]
#[serial(snapshot)]
async fn a_clone_status_stays_readable_while_its_copy_runs() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-liveness-status-source";
    let clone = "urn:ngsi-ld:Snapshot:clone-liveness-status";
    let schema = source_of_cars(pool, source).await;
    let hold = hold_relation(pool, &schema, "entity_search").await;

    start_clone(&primary(), source, clone).await;
    until_the_copy_waits_on(pool, &schema, "entity_search").await;
    let reading = Instant::now();
    while reading.elapsed() < Duration::from_secs(20) {
        pretty_assertions::assert_eq!(status_within_deadline(&primary(), clone).await["snapshotStatus"], "preparing");
        sleep(Duration::from_secs(2)).await;
    }
    hold.release().await;

    assert_complete_clone(pool, clone).await;
    let read = status_within_deadline(&primary(), clone).await;
    assert!(read["lastUsedAt"].is_string(), "{read}");

    environment.reset_and_finish().await.expect("reset");
}

/// Andromeda policy under clause 5.16.2.4: the clone is its
/// source as of the start of the copy, and the copy never blocks a write to the
/// source.
#[tokio::test]
#[serial(snapshot)]
async fn a_clone_copies_its_source_as_of_one_moment_without_blocking_source_writes() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-liveness-moment-source";
    let clone = "urn:ngsi-ld:Snapshot:clone-liveness-b";
    let schema = source_of_cars(pool, source).await;
    let hold = hold_relation(pool, &schema, "scope_history").await;

    start_clone(&primary(), source, clone).await;
    until_the_copy_waits_on(pool, &schema, "scope_history").await;
    let late = json!({"id": "urn:ngsi-ld:CloneCar:late", "type": CAR, SPEED: {"type": "Property", "value": 0}, "@context": CORE_CONTEXT});
    let written = client_within(Duration::from_secs(5))
        .post(broker_url("/entities"))
        .header("Content-Type", "application/ld+json")
        .header("NGSILD-Snapshot", source)
        .json(&late)
        .send()
        .await
        .expect("a write to the source answers within 5 s while the clone copies it");
    pretty_assertions::assert_eq!(written.status(), StatusCode::CREATED);
    hold.release().await;

    assert_complete_clone(pool, clone).await;
    let mut source_cars = captured_ids();
    source_cars.insert("urn:ngsi-ld:CloneCar:late".to_owned());
    pretty_assertions::assert_eq!(car_ids(&primary(), source).await, source_cars);
    pretty_assertions::assert_eq!(census_drift(pool, &schema).await.expect("the census comparison"), Vec::<String>::new());

    environment.reset_and_finish().await.expect("reset");
}

/// Clause 5.16.5.4 deletes a Snapshot without precondition: a delete of a clone
/// whose copy is in flight completes once the copy ends, and leaves no branch,
/// no preparation row and no status notification behind.
#[tokio::test]
#[serial(snapshot)]
async fn deleting_a_clone_whose_copy_is_in_flight_completes_and_leaves_no_branch_behind() {
    let environment = clean_database().await;
    let pool = environment.database_pool();

    assert_delete_during_copy_completes(pool, &primary(), &primary(), "urn:ngsi-ld:Snapshot:clone-liveness-c").await;

    environment.reset_and_finish().await.expect("reset");
}

/// Clauses 5.16.2.4 and 5.16.3.4 across replicas: one process copies the
/// clone, its claim never changes hands during the copy, and both processes
/// answer its status and its data.
#[tokio::test]
#[serial(snapshot)]
async fn a_clone_is_copied_by_exactly_one_replica_and_read_through_any() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-liveness-replica-source";
    let clone = "urn:ngsi-ld:Snapshot:clone-liveness-r1";
    let schema = source_of_cars(pool, source).await;
    let hold = hold_relation(pool, &schema, "entity_search").await;

    start_clone(REPLICA_BASE, source, clone).await;
    until_the_copy_waits_on(pool, &schema, "entity_search").await;
    let key = snapshot_key(pool, clone).await;
    let mut claimants = BTreeSet::new();
    let sampling = Instant::now();
    let primary = primary();
    for base in [primary.as_str(), REPLICA_BASE].into_iter().cycle() {
        if sampling.elapsed() >= Duration::from_secs(20) {
            break;
        }
        let claimant: Option<String> = claimant_statement(key).to_query_scalar().fetch_one(pool).await.expect("the clone's claim");
        claimants.insert(claimant.expect("the copying process holds the claim"));
        pretty_assertions::assert_eq!(waiting_on(pool, &schema, "entity_search").await, 1, "exactly one copy runs");
        pretty_assertions::assert_eq!(status_within_deadline(base, clone).await["snapshotStatus"], "preparing");
        sleep(Duration::from_millis(500)).await;
    }
    pretty_assertions::assert_eq!(claimants.len(), 1, "the claim never changed hands: {claimants:?}");
    hold.release().await;

    assert_complete_clone(pool, clone).await;
    pretty_assertions::assert_eq!(car_ids(REPLICA_BASE, clone).await, captured_ids());

    environment.reset_and_finish().await.expect("reset");
}

/// Clause 5.16.2.4: a clone's claim is not taken over while its copy runs,
/// even once its lease has lapsed; the lapse itself, a non-key update of the
/// preparation row, does not wait for the copy.
#[tokio::test]
#[serial(snapshot)]
async fn a_lapsed_clone_lease_is_not_taken_over_while_the_copy_holds_its_fence() {
    let environment = clean_database().await;
    let pool = environment.database_pool();
    let source = "urn:ngsi-ld:Snapshot:clone-liveness-lapse-source";
    let clone = "urn:ngsi-ld:Snapshot:clone-liveness-r2";
    let schema = source_of_cars(pool, source).await;
    let hold = hold_relation(pool, &schema, "entity_search").await;

    start_clone(&primary(), source, clone).await;
    until_the_copy_waits_on(pool, &schema, "entity_search").await;
    let key = snapshot_key(pool, clone).await;
    let claimant: Option<String> = claimant_statement(key).to_query_scalar().fetch_one(pool).await.expect("the clone's claim");
    let lapse = Query::update()
        .table(SnapshotPreparation::Table)
        .value(SnapshotPreparation::LeaseUntil, Expr::expr(now()).sub(make_interval_secs(Expr::val(1.0))))
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(key))
        .to_query();
    timeout(Duration::from_secs(1), lapse.execute(pool))
        .await
        .expect("the lapse does not wait for the copy")
        .expect("the lease lapses");
    let mut claim = pool.begin().await.expect("the claim transaction");
    let claimed: Option<i64> = Query::select()
        .column((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey))
        .from(Snapshots::Table)
        .inner_join(
            SnapshotPreparation::Table,
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::SnapshotKey)).equals((Snapshots::Table, Snapshots::SnapshotKey)),
        )
        .and_where(Expr::col((Snapshots::Table, Snapshots::Id)).eq(clone))
        .and_where(Expr::col((SnapshotPreparation::Table, SnapshotPreparation::Status)).eq(1))
        .and_where(
            Expr::col((SnapshotPreparation::Table, SnapshotPreparation::LeaseUntil))
                .is_null()
                .or(Expr::col((SnapshotPreparation::Table, SnapshotPreparation::LeaseUntil)).lte(Expr::current_timestamp())),
        )
        .lock_with_tables_behavior(LockType::Update, [SnapshotPreparation::Table], LockBehavior::SkipLocked)
        .to_query_scalar()
        .fetch_optional(&mut *claim)
        .await
        .expect("the claim statement");
    claim.rollback().await.expect("the claim is given up");
    pretty_assertions::assert_eq!(claimed, None, "the copy's fence keeps the clone from being claimed");
    let still: Option<String> = claimant_statement(key).to_query_scalar().fetch_one(pool).await.expect("the clone's claim");
    pretty_assertions::assert_eq!(still, claimant, "the claim is unchanged");
    hold.release().await;

    assert_complete_clone(pool, clone).await;
    let published: (Option<String>, Option<String>) = Query::select()
        .expr(cast(Expr::col(SnapshotPreparation::ClaimedBy), PostgresType::Text))
        .expr(cast(Expr::col(SnapshotPreparation::LeaseUntil), PostgresType::Text))
        .from(SnapshotPreparation::Table)
        .and_where(Expr::col(SnapshotPreparation::SnapshotKey).eq(key))
        .to_query_as()
        .fetch_one(pool)
        .await
        .expect("the published clone's preparation row");
    pretty_assertions::assert_eq!(published, (None, None), "the publication cleared the claim");

    environment.reset_and_finish().await.expect("reset");
}

/// Clause 5.16.5.4 across replicas: a delete served by one process completes
/// while another process copies the clone.
#[tokio::test]
#[serial(snapshot)]
async fn deleting_a_clone_through_the_replica_while_the_other_process_copies_completes() {
    let environment = clean_database().await;
    let pool = environment.database_pool();

    assert_delete_during_copy_completes(pool, &primary(), REPLICA_BASE, "urn:ngsi-ld:Snapshot:clone-liveness-r3").await;

    environment.reset_and_finish().await.expect("reset");
}
