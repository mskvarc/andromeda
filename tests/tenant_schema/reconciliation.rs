//! Verification matrix row "Migration reconciliation".
//!
//! The rebuild moved every temporal and current-state relation to new shapes
//! and then dropped the relations it read from, so the old representation is no
//! longer there to be compared against. What remains checkable, and what the
//! row is actually about, is that the backfilled result is internally
//! consistent: every child row still names a parent that exists, every
//! dictionary and catalogue reference resolves, and no relation the rebuild
//! replaced was left behind as a second authority.

use crate::SUITE;
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use serde_json::{Value, json};
use sqlx::PgPool;

/// One referential invariant of the rebuilt schema, with the query that counts
/// the rows breaking it.
struct Invariant {
    /// What the query counts, phrased as the failure it reports.
    description: &'static str,
    /// A statement returning the number of offending rows.
    count: &'static str,
}

/// Every parent-child and dictionary reference the rebuild had to preserve.
const INVARIANTS: &[Invariant] = &[
    Invariant {
        description: "history rows whose stream no longer exists",
        count: "SELECT COUNT(*) FROM attribute_history ah WHERE NOT EXISTS (SELECT 1 FROM temporal_attribute_streams s WHERE s.stream_key = ah.stream_key)",
    },
    Invariant {
        description: "streams whose temporal entity no longer exists",
        count: "SELECT COUNT(*) FROM temporal_attribute_streams s WHERE NOT EXISTS (SELECT 1 FROM temporal_entities te WHERE te.entity_key = s.temporal_entity_key)",
    },
    Invariant {
        description: "scope history rows whose temporal entity no longer exists",
        count: "SELECT COUNT(*) FROM scope_history sh WHERE NOT EXISTS (SELECT 1 FROM temporal_entities te WHERE te.entity_key = sh.temporal_entity_key)",
    },
    Invariant {
        description: "current projection rows whose entity no longer exists",
        count: "SELECT COUNT(*) FROM current_attribute_values cav WHERE NOT EXISTS (SELECT 1 FROM entities e WHERE e.entity_key = cav.entity_key)",
    },
    Invariant {
        description: "document rows whose entity no longer exists",
        count: "SELECT COUNT(*) FROM entity_documents d WHERE NOT EXISTS (SELECT 1 FROM entities e WHERE e.entity_key = d.entity_key)",
    },
    Invariant {
        description: "search rows whose entity no longer exists",
        count: "SELECT COUNT(*) FROM entity_search s WHERE NOT EXISTS (SELECT 1 FROM entities e WHERE e.entity_key = s.entity_key)",
    },
    Invariant {
        description: "entities without a document row",
        count: "SELECT COUNT(*) FROM entities e WHERE NOT EXISTS (SELECT 1 FROM entity_documents d WHERE d.entity_key = e.entity_key)",
    },
    Invariant {
        description: "entities without a search row",
        count: "SELECT COUNT(*) FROM entities e WHERE NOT EXISTS (SELECT 1 FROM entity_search s WHERE s.entity_key = e.entity_key)",
    },
    Invariant {
        description: "search rows naming a type key that does not exist",
        count: "SELECT COUNT(*) FROM entity_search s CROSS JOIN LATERAL unnest(s.type_ids) AS declared(key) WHERE NOT EXISTS (SELECT 1 FROM entity_type_names n WHERE n.id = declared.key)",
    },
    Invariant {
        description: "search rows naming an attribute key that does not exist",
        count: "SELECT COUNT(*) FROM entity_search s CROSS JOIN LATERAL unnest(s.attribute_name_ids) AS declared(key) WHERE NOT EXISTS (SELECT 1 FROM attribute_names an WHERE an.id = declared.key)",
    },
    Invariant {
        description: "streams naming a dictionary key that does not exist",
        count: "SELECT COUNT(*) FROM temporal_attribute_streams s WHERE NOT EXISTS (SELECT 1 FROM attribute_names an WHERE an.id = s.attribute_name_id)",
    },
    Invariant {
        description: "current projection rows naming a dictionary key that does not exist",
        count: "SELECT COUNT(*) FROM current_attribute_values cav WHERE NOT EXISTS (SELECT 1 FROM attribute_names an WHERE an.id = cav.attribute_name_id)",
    },
    Invariant {
        description: "routing projection rows naming a dictionary key that does not exist (the wildcard sentinel 0 names none)",
        count: "SELECT COUNT(*) FROM registration_information ri \
                CROSS JOIN LATERAL unnest(ri.property_name_ids || ri.relationship_name_ids) AS declared(key) \
                WHERE declared.key <> 0 AND NOT EXISTS (SELECT 1 FROM attribute_names an WHERE an.id = declared.key)",
    },
    Invariant {
        description: "registrations without a routing row (an unconstrained registration has its wildcard row)",
        count: "SELECT COUNT(*) FROM context_source_registrations csr WHERE NOT EXISTS (SELECT 1 FROM registration_information ri WHERE ri.registration_id = csr.id)",
    },
    Invariant {
        description: "snapshots whose context space does not exist or is not a Snapshot branch",
        count: "SELECT COUNT(*) FROM snapshots s WHERE NOT EXISTS (SELECT 1 FROM context_spaces cs WHERE cs.context_space_key = s.context_space_key AND cs.kind = 'snapshot')",
    },
    Invariant {
        description: "history rows naming a unit the catalogue does not hold",
        count: "SELECT COUNT(*) FROM attribute_history ah WHERE ah.unit_code_id IS NOT NULL AND NOT EXISTS (SELECT 1 FROM unit_codes uc WHERE uc.id = ah.unit_code_id)",
    },
    Invariant {
        description: "history rows naming a write record that does not exist",
        count: "SELECT COUNT(*) FROM attribute_history ah WHERE NOT EXISTS (SELECT 1 FROM write_records wr WHERE wr.write_id = ah.write_id)",
    },
    Invariant {
        description: "subscriptions without a delivery-state row, or state rows without a subscription",
        count: "SELECT (SELECT COUNT(*) FROM subscriptions s WHERE NOT EXISTS (SELECT 1 FROM subscription_delivery_state d WHERE d.subscription_key = s.subscription_key)) \
                + (SELECT COUNT(*) FROM subscription_delivery_state d WHERE NOT EXISTS (SELECT 1 FROM subscriptions s WHERE s.subscription_key = d.subscription_key))",
    },
    Invariant {
        description: "history rows populating neither instance identity column exactly once",
        count: "SELECT COUNT(*) FROM attribute_history WHERE (generated_instance_uuid IS NULL) = (verbatim_instance_id IS NULL)",
    },
];

/// The relations the rebuild replaced, which must not have been left in place
/// as a second authority for the same facts.
const REPLACED_RELATIONS: &[&str] = &[
    "attribute_current",
    "temporal_entity_attributes",
    "notification_outbox",
    "snapshot_entities",
    "snapshot_temporal_entities",
];

/// Every relation an invariant reads, which has to hold rows for the count to
/// mean anything.
///
/// A referential check over an empty relation returns zero whatever the schema
/// does, so the population assertion is part of the test rather than a
/// precondition left to the suite that happened to run before it.
const POPULATED_RELATIONS: &[&str] = &[
    "entities",
    "entity_documents",
    "entity_search",
    "temporal_entities",
    "temporal_attribute_streams",
    "attribute_history",
    "current_attribute_values",
    "attribute_names",
    "write_records",
    "registration_information",
    "subscriptions",
    "subscription_delivery_state",
    "unit_codes",
];

/// A subscription, so the delivery-state row created with it is there to be
/// checked against.
fn reconciled_subscription(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Subscription",
        "entities": [{"type": "Device"}],
        "notification": {"endpoint": {"uri": "http://127.0.0.1:8085/notify", "accept": "application/json"}},
        "@context": [CORE_CONTEXT]
    })
}

/// An entity exercising every relation the invariants join across: a unit-
/// bearing Property, a Relationship, and a scope.
fn reconciled_entity(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Device",
        "scope": "/Reconciliation",
        "weight": {"type": "Property", "value": 5, "unitCode": "KGM"},
        "serial": {"type": "Property", "value": "sn-1"},
        "installedBy": {"type": "Relationship", "object": "urn:ngsi-ld:Person:installer"},
        "@context": [CORE_CONTEXT]
    })
}

/// A registration whose selectors project routing rows naming dictionary keys.
fn reconciled_registration(id: &str) -> Value {
    json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "information": [{"entities": [{"type": "Device"}], "propertyNames": ["weight"], "relationshipNames": ["installedBy"]}],
        "operations": ["retrieveEntity"],
        "endpoint": "http://localhost:9/csource",
        "@context": [CORE_CONTEXT]
    })
}

/// A Snapshot whose control record owns a first-class context branch.
fn reconciled_snapshot(id: &str) -> Value {
    json!({
        "id": id,
        "type": "Snapshot",
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Device"}]}]
    })
}

/// Count the rows breaking one invariant.
async fn offending(pool: &PgPool, invariant: &Invariant) -> i64 {
    sqlx::query_scalar(invariant.count)
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| unreachable!("the reconciliation query must run: {error}"))
}

/// The number of rows one relation holds.
async fn population(pool: &PgPool, relation: &str) -> i64 {
    sqlx::query_scalar(sqlx::AssertSqlSafe(format!("SELECT COUNT(*) FROM {relation}")))
        .fetch_one(pool)
        .await
        .unwrap_or_else(|error| unreachable!("{relation} must be readable: {error}"))
}

#[tokio::test]
async fn the_backfilled_schema_holds_no_orphan_row_and_no_unresolvable_reference() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let client = scope.client();
    let entity_id = scope.id("Device:reconciliation");
    let registration_id = scope.id("ContextSourceRegistration:reconciliation");

    let created = client
        .post_entity(&reconciled_entity(&entity_id))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);
    let registered = client
        .create_registration(&reconciled_registration(&registration_id))
        .await
        .expect("create registration request failed");
    assert_eq!(registered.status, StatusCode::CREATED, "create registration: {:?}", registered.body);
    let subscription_id = scope.id("Subscription:reconciliation");
    let subscribed = client
        .create_subscription(&reconciled_subscription(&subscription_id))
        .await
        .expect("create subscription request failed");
    assert_eq!(subscribed.status, StatusCode::CREATED, "create subscription: {:?}", subscribed.body);
    let snapshot_id = scope.id("Snapshot:reconciliation");
    let snapshotted = client
        .create_snapshot(&reconciled_snapshot(&snapshot_id))
        .await
        .expect("create snapshot request failed");
    assert_eq!(snapshotted.status, StatusCode::CREATED, "create snapshot: {:?}", snapshotted.body);

    let pool = scope.database_pool();
    for relation in POPULATED_RELATIONS {
        assert!(
            population(pool, relation).await > 0,
            "{relation} is empty, so the invariants over it cannot fail"
        );
    }
    for invariant in INVARIANTS {
        assert_eq!(offending(pool, invariant).await, 0, "{}", invariant.description);
    }

    scope.finish().await.expect("cleanup failed");
}

/// The invariant queries have to be able to report a violation, not merely to
/// return zero.
///
/// `attribute_history` is the relation to demonstrate it on: it is a
/// hypertable, hypertables carry no foreign keys, and the invariant over it is
/// therefore the only thing standing between the rebuild and a history row
/// whose stream does not exist. One deliberately orphaned row is counted, and
/// removing it restores the clean state.
#[tokio::test]
async fn an_invariant_violation_is_actually_counted_rather_than_silently_passing() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let pool = scope.database_pool();

    let orphan_history = "SELECT COUNT(*) FROM attribute_history ah \
                          WHERE ah.write_id = $1 AND NOT EXISTS (SELECT 1 FROM temporal_attribute_streams s WHERE s.stream_key = ah.stream_key)";
    let write_id = uuid::Uuid::new_v4();
    let before: i64 = sqlx::query_scalar(orphan_history)
        .bind(write_id)
        .fetch_one(pool)
        .await
        .expect("the orphan-history query must run");
    assert_eq!(before, 0, "the write identity is fresh, so it names no row yet");

    sqlx::query(
        "INSERT INTO attribute_history \
         (recorded_at, write_id, stream_key, write_ordinal, attribute_kind, value_kind, generated_instance_uuid, value_number) \
         VALUES (now(), $1, 9223372036854775807, 0, 1, 1, gen_random_uuid(), 1::numeric)",
    )
    .bind(write_id)
    .execute(pool)
    .await
    .expect("a history row naming no stream can be inserted, which is the state the invariant exists to catch");

    let during: i64 = sqlx::query_scalar(orphan_history)
        .bind(write_id)
        .fetch_one(pool)
        .await
        .expect("the orphan-history query must run");
    assert_eq!(during, 1, "the invariant must count the orphan it was given");

    sqlx::query("DELETE FROM attribute_history WHERE write_id = $1")
        .bind(write_id)
        .execute(pool)
        .await
        .expect("the orphan must be removable");

    let after: i64 = sqlx::query_scalar(orphan_history)
        .bind(write_id)
        .fetch_one(pool)
        .await
        .expect("the orphan-history query must run");
    assert_eq!(after, 0, "removing the orphan restores the clean state");

    scope.finish().await.expect("cleanup failed");
}

#[tokio::test]
async fn no_relation_the_rebuild_replaced_survives_as_a_second_authority() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");

    for relation in REPLACED_RELATIONS {
        let surviving: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM information_schema.tables WHERE table_schema = 'public' AND table_name = $1")
            .bind(relation)
            .fetch_one(scope.database_pool())
            .await
            .expect("the relation catalogue must be readable");
        assert_eq!(surviving, 0, "{relation} was replaced by the rebuild and must not still exist");
    }

    scope.finish().await.expect("cleanup failed");
}

/// Both temporal and current state are keyed by their own surrogate, and an
/// identifier names at most one of each: a duplicate would make retrieval pick
/// arbitrarily between two rows that claim the same Entity.
#[tokio::test]
async fn every_entity_identifier_names_at_most_one_current_and_one_temporal_row() {
    let scope = TestEnvironment::from_suite(SUITE).await.expect("isolated test environment");
    let entity_id = scope.id("Device:reconciliation-identity");
    let created = scope
        .client()
        .post_entity(&reconciled_entity(&entity_id))
        .send()
        .await
        .expect("create entity request failed");
    assert_eq!(created.status, StatusCode::CREATED, "create entity: {:?}", created.body);

    let duplicated_current: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM (SELECT id FROM entities GROUP BY id HAVING COUNT(*) > 1) AS duplicated")
        .fetch_one(scope.database_pool())
        .await
        .expect("the entity relation must be readable");
    assert_eq!(duplicated_current, 0, "an identifier names at most one current Entity");

    let duplicated_temporal: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM (SELECT id FROM temporal_entities GROUP BY id HAVING COUNT(*) > 1) AS duplicated")
        .fetch_one(scope.database_pool())
        .await
        .expect("the temporal entity relation must be readable");
    assert_eq!(duplicated_temporal, 0, "an identifier names at most one temporal Entity");

    scope.finish().await.expect("cleanup failed");
}
