//! Refused-write isolation: a write whose content is refused inside the commit
//! transaction is set aside, and the other writes of its commit land.
//!
//! Clauses 5.6.7 to 5.6.10 report every entity of a batch on its own, and
//! clause 5.6.8.4 adds a `BatchEntityError` for "any kind of error or abnormal
//! situation" while processing one Entity. The request layer refuses what it
//! can per entity before a commit starts; these verifications hand the broker's
//! own committer content that layer would have refused, so the refusal is
//! raised only once the transaction is running -- where one write's refusal
//! would otherwise roll back every write sharing the transaction. What the
//! store then holds is read directly, so each assertion states what landed
//! rather than what a rendering suggests.

use crate::{
    broker_committer::committer,
    projection_rows::{entity_key, rows_for_attribute, temporal_entity_exists},
};
use andromeda_context_commit::{
    atomic_write::AtomicWrite,
    committer::ContextCommitter,
    entity_mutation::EntityMutation,
    entity_record::EntityRecord,
    error::CommitError,
    history_record::HistoryRecord,
    precondition::Precondition,
    submission_refusal::SubmissionRefusal,
    write_outcome::{WriteOutcome, single},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use serde_json::{Map, Value, json};
use sqlx::PgPool;
use std::sync::Arc;

/// The expanded Building type of the ETSI test-suite context.
const BUILDING_TYPE: &str = "https://ngsi-ld-test-suite/context#Building";

/// The expanded attribute every write carries.
const TEMPERATURE: &str = "https://example.test/current-projection/refused-write-isolation/temperature";

/// A code the UN/CEFACT catalogue registers: degrees Celsius.
const CELSIUS: &str = "CEL";

/// A code the UN/CEFACT catalogue does not register.
const UNREGISTERED: &str = "NOTACODE";

/// The expanded Entity one write stores: a `temperature` Property, with the
/// given `unitCode` when there is one, and the Building type unless the Entity
/// is written without one.
fn expanded_entity(id: &str, typed: Typed, unit_code: Option<&str>) -> Arc<Value> {
    let mut instance = Map::new();
    instance.insert("@type".to_owned(), json!(["https://uri.etsi.org/ngsi-ld/Property"]));
    instance.insert("https://uri.etsi.org/ngsi-ld/hasValue".to_owned(), json!([{"@value": 21.5}]));
    if let Some(code) = unit_code {
        instance.insert("https://uri.etsi.org/ngsi-ld/unitCode".to_owned(), json!([{"@value": code}]));
    }
    let mut entity = Map::new();
    entity.insert("@id".to_owned(), json!(id));
    if let Typed::Building = typed {
        entity.insert("@type".to_owned(), json!([BUILDING_TYPE]));
    }
    entity.insert(TEMPERATURE.to_owned(), json!([instance]));
    Arc::new(json!([entity]))
}

/// Whether an Entity is written with a type.
#[derive(Clone, Copy)]
enum Typed {
    /// The Entity names the Building type.
    Building,
    /// The Entity names no type at all, which clause 4.5.1 forbids.
    Untyped,
}

/// The write that creates `id`, as a batch create builds it.
fn creation(id: &str, typed: Typed, unit_code: Option<&str>) -> AtomicWrite {
    let payload = expanded_entity(id, typed, unit_code);
    AtomicWrite::builder()
        .entity_id(Arc::new(id.parse::<Identifier>().expect("a valid entity URI")))
        .entity(EntityMutation::Insert(
            EntityRecord::from_expanded(Arc::clone(&payload)).expect("readable members"), // clone: Arc ref-count bump; the same payload is the observation
        ))
        .history(vec![HistoryRecord::Observations(payload)])
        .precondition(Precondition::MustNotExist)
        .build()
}

/// Commit `writes` through the broker's own committer on the live branch.
async fn commit(pool: &PgPool, writes: Vec<AtomicWrite>) -> Result<Vec<WriteOutcome>, CommitError> {
    committer(pool).commit(&ContextTarget::Live, writes).await
}

/// Whether `id` landed whole: its Entity row, its projection row and its
/// Temporal Evolution.
async fn landed(pool: &PgPool, id: &str) -> bool {
    entity_key(pool, id).await.is_some() && rows_for_attribute(pool, id, "temperature").await.len() == 1 && temporal_entity_exists(pool, id).await
}

/// Whether nothing of `id` reached the store.
async fn absent(pool: &PgPool, id: &str) -> bool {
    entity_key(pool, id).await.is_none() && !temporal_entity_exists(pool, id).await
}

#[tokio::test]
async fn current_projection_a_unit_code_refused_at_commit_fails_only_its_own_write() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let first = scope.id("Building:isolation-unit-first");
    let refused = scope.id("Building:isolation-unit-refused");
    let last = scope.id("Building:isolation-unit-last");

    let outcomes = commit(
        pool,
        vec![
            creation(&first, Typed::Building, Some(CELSIUS)),
            creation(&refused, Typed::Building, Some(UNREGISTERED)),
            creation(&last, Typed::Building, None),
        ],
    )
    .await
    .expect("a refusal of one write must not fail the commit");

    assert!(
        matches!(
            outcomes.as_slice(),
            [
                WriteOutcome::Created,
                WriteOutcome::Refused(SubmissionRefusal::UnitCodeUnregistered),
                WriteOutcome::Created
            ]
        ),
        "{outcomes:?}"
    );
    assert!(landed(pool, &first).await, "the write before the refused one must land whole");
    assert!(landed(pool, &last).await, "the write after the refused one must land whole");
    assert!(absent(pool, &refused).await, "nothing of the refused write may reach the store");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 4.5.1: an Entity with no type is refused by the entity statement
/// itself, which writes every entity of the commit at once and names the one
/// it refused.
#[tokio::test]
async fn current_projection_an_untyped_entity_refused_by_the_entity_statement_fails_only_its_own_write() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let untyped = scope.id("Building:isolation-untyped");
    let typed = scope.id("Building:isolation-typed");

    let outcomes = commit(pool, vec![creation(&untyped, Typed::Untyped, None), creation(&typed, Typed::Building, None)])
        .await
        .expect("a refusal of one write must not fail the commit");

    assert!(
        matches!(
            outcomes.as_slice(),
            [WriteOutcome::Refused(SubmissionRefusal::UntypedEntity(_)), WriteOutcome::Created]
        ),
        "{outcomes:?}"
    );
    assert!(absent(pool, &untyped).await, "the untyped Entity must not be stored");
    assert!(landed(pool, &typed).await, "its typed neighbour must land whole");

    scope.finish().await.expect("cleanup failed");
}

/// Clause 6.14.3.1 answers 207 when "only some or none" of the entities were
/// created: a commit whose every write was refused still answers for each.
#[tokio::test]
async fn current_projection_a_commit_whose_every_write_is_refused_reports_each_refusal_and_stores_nothing() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let unit = scope.id("Building:isolation-all-unit");
    let untyped = scope.id("Building:isolation-all-untyped");

    let outcomes = commit(
        pool,
        vec![creation(&unit, Typed::Building, Some(UNREGISTERED)), creation(&untyped, Typed::Untyped, None)],
    )
    .await
    .expect("refusals of every write must not fail the commit");

    assert!(
        matches!(
            outcomes.as_slice(),
            [
                WriteOutcome::Refused(SubmissionRefusal::UnitCodeUnregistered),
                WriteOutcome::Refused(SubmissionRefusal::UntypedEntity(_))
            ]
        ),
        "{outcomes:?}"
    );
    assert!(absent(pool, &unit).await);
    assert!(absent(pool, &untyped).await);

    scope.finish().await.expect("cleanup failed");
}

/// A one-write commit has no neighbour to protect: the refused write is its
/// outcome, and every caller reading that one outcome sees the refusal as the
/// commit's own failure, answered `BadRequestData` as before.
#[tokio::test]
async fn current_projection_a_refused_single_write_is_the_commits_refusal() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let refused = scope.id("Building:isolation-single");

    let outcomes = commit(pool, vec![creation(&refused, Typed::Building, Some(UNREGISTERED))])
        .await
        .expect("the refusal is the write's outcome");
    assert!(
        matches!(single(outcomes), Err(CommitError::Submission(SubmissionRefusal::UnitCodeUnregistered))),
        "a one-write commit reports its refused write as its own refusal"
    );
    assert!(absent(pool, &refused).await);

    scope.finish().await.expect("cleanup failed");
}

/// The rows a landed write's projection holds are the ones it wrote, unit and
/// all: setting a neighbour aside and running the rest again leaves no trace
/// of the attempt that was rolled back.
#[tokio::test]
async fn current_projection_writes_landed_after_a_refusal_hold_exactly_what_they_wrote() {
    let scope = TestEnvironment::from_suite("current-projection").await.expect("isolated test environment");
    let pool = scope.database_pool();
    let kept = scope.id("Building:isolation-kept");
    let refused = scope.id("Building:isolation-kept-refused");

    commit(
        pool,
        vec![
            creation(&refused, Typed::Building, Some(UNREGISTERED)),
            creation(&kept, Typed::Building, Some(CELSIUS)),
        ],
    )
    .await
    .expect("a refusal of one write must not fail the commit");

    let rows = rows_for_attribute(pool, &kept, "temperature").await;
    assert_eq!(rows.len(), 1, "one projection row, not one per attempt: {rows:?}");
    assert_eq!(rows[0].value_text.as_deref(), Some("21.5"));
    let unit_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM current_attribute_values AS c JOIN entities AS e ON e.entity_key = c.entity_key \
         JOIN unit_codes AS u ON u.id = c.unit_code_id WHERE e.id = $1 AND u.code = $2",
    )
    .bind(&kept)
    .bind(CELSIUS)
    .fetch_one(pool)
    .await
    .expect("the projected unit must be readable");
    assert_eq!(unit_rows, 1, "the landed write keeps its unit");
    let history_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM attribute_history AS h JOIN temporal_attribute_streams AS s ON s.stream_key = h.stream_key \
         JOIN temporal_entities AS t ON t.temporal_entity_key = s.temporal_entity_key WHERE t.id = $1",
    )
    .bind(&kept)
    .fetch_one(pool)
    .await
    .expect("the recorded history must be readable");
    assert_eq!(history_rows, 1, "one history row, not one per attempt");

    scope.finish().await.expect("cleanup failed");
}
