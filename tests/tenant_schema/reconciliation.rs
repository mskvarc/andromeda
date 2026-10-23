//! Migration reconciliation: the backfilled schema is internally consistent.
//!
//! The rebuild dropped the relations it read from, so there is no earlier
//! representation to compare against. What is checkable, and what matters, is
//! that the backfilled result is internally consistent: every
//! child row still names a parent that exists, every dictionary and catalogue
//! reference resolves, and no relation the rebuild replaced was left behind as
//! a second authority.

use crate::SUITE;
use andromeda_postgresql_schema::{
    csr::{Csr, RegistrationInformation},
    current::{CurrentAttributes, Entities, EntitySearch, EntityTypeMembers},
    name_dictionary::{AttributeNames, EntityTypeNames},
    subscriptions::{SubscriptionDeliveryState, Subscriptions},
    temporal::{AttributeHistory, ScopeHistory, TemporalAttributeStreams},
};
use andromeda_postgresql_seaquery::{
    aggregate::count_all,
    cast::cast,
    catalog_function::random_uuid,
    clock::now,
    literal::int_literal,
    pg_array::{any, array_cat},
    pg_type::PostgresType,
    scalar_subquery::scalar_subquery,
    statement::Statement,
    system_catalog::{InformationSchema, InformationSchemaTables},
    unnest::unnest_of,
};
use andromeda_test_fixtures::context::CORE_CONTEXT;
use andromeda_test_harness::environment::TestEnvironment;
use pretty_assertions::assert_eq;
use reqwest::StatusCode;
use sea_query::{Alias, Expr, ExprTrait, IntoIden, Query, SelectStatement};
use serde_json::{Value, json};
use sqlx::PgPool;

/// One referential invariant of the rebuilt schema, with the query that counts
/// the rows breaking it.
struct Invariant {
    /// What the query counts, phrased as the failure it reports.
    description: &'static str,
    /// A statement returning the number of offending rows.
    count: fn() -> SelectStatement,
}

/// The rows of `child` whose `child_key` matches no `parent_key` of `parent`.
fn orphans<Child, ChildKey, Parent, ParentKey>(child: Child, child_key: ChildKey, parent: Parent, parent_key: ParentKey) -> SelectStatement
where
    Child: IntoIden + Copy,
    ChildKey: IntoIden + Copy,
    Parent: IntoIden + Copy,
    ParentKey: IntoIden + Copy,
{
    Query::select()
        .expr(count_all())
        .from(child)
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(parent)
                    .and_where(Expr::col((parent, parent_key)).equals((child, child_key)))
                    .to_owned(),
            )
            .not(),
        )
        .to_owned()
}

/// The keys declared in the array column `array` of `table`, one row per key,
/// that match no `dictionary_key` of `dictionary`.
fn unresolved_keys<Table, Array, Dictionary, DictionaryKey>(
    table: Table,
    array: Array,
    dictionary: Dictionary,
    dictionary_key: DictionaryKey,
) -> SelectStatement
where
    Table: IntoIden + Copy,
    Array: IntoIden + Copy,
    Dictionary: IntoIden + Copy,
    DictionaryKey: IntoIden + Copy,
{
    unresolved_declared(
        Query::select()
            .expr_as(unnest_of(Expr::col((table, array))), Alias::new("declared"))
            .from(table)
            .to_owned(),
        dictionary,
        dictionary_key,
    )
}

/// The rows of `declared`, whose `declared` column holds one key each, that
/// match no `dictionary_key` of `dictionary`.
fn unresolved_declared<Dictionary, DictionaryKey>(declared: SelectStatement, dictionary: Dictionary, dictionary_key: DictionaryKey) -> SelectStatement
where
    Dictionary: IntoIden + Copy,
    DictionaryKey: IntoIden + Copy,
{
    Query::select()
        .expr(count_all())
        .from_subquery(declared, Alias::new("declared_keys"))
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(dictionary)
                    .and_where(Expr::col((dictionary, dictionary_key)).equals((Alias::new("declared_keys"), Alias::new("declared"))))
                    .to_owned(),
            )
            .not(),
        )
        .to_owned()
}

/// The type memberships out of step with the search rows' types, in either
/// direction.
fn membership_drift() -> SelectStatement {
    let declared = Query::select()
        .column((Entities::Table, Entities::EntityKey))
        .column((Entities::Table, Entities::Id))
        .expr_as(
            unnest_of(Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys))),
            Alias::new("declared"),
        )
        .from(EntitySearch::Table)
        .inner_join(
            Entities::Table,
            Expr::col((Entities::Table, Entities::EntityKey)).equals((EntitySearch::Table, EntitySearch::EntityKey)),
        )
        .to_owned();
    let declared_without_membership = Query::select()
        .expr(count_all())
        .from_subquery(declared, Alias::new("declared_types"))
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(EntityTypeMembers::Table)
                    .and_where(
                        Expr::col((EntityTypeMembers::Table, EntityTypeMembers::EntityTypeNameKey))
                            .equals((Alias::new("declared_types"), Alias::new("declared"))),
                    )
                    .and_where(Expr::col((EntityTypeMembers::Table, EntityTypeMembers::Id)).equals((Alias::new("declared_types"), Entities::Id)))
                    .and_where(Expr::col((EntityTypeMembers::Table, EntityTypeMembers::EntityKey)).equals((Alias::new("declared_types"), Entities::EntityKey)))
                    .to_owned(),
            )
            .not(),
        )
        .to_owned();
    let membership_without_declaration = Query::select()
        .expr(count_all())
        .from(EntityTypeMembers::Table)
        .and_where(
            Expr::exists(
                Query::select()
                    .expr(int_literal(1))
                    .from(EntitySearch::Table)
                    .and_where(Expr::col((EntitySearch::Table, EntitySearch::EntityKey)).equals((EntityTypeMembers::Table, EntityTypeMembers::EntityKey)))
                    .and_where(
                        Expr::col((EntityTypeMembers::Table, EntityTypeMembers::EntityTypeNameKey))
                            .eq(any(Expr::col((EntitySearch::Table, EntitySearch::EntityTypeNameKeys)))),
                    )
                    .to_owned(),
            )
            .not(),
        )
        .to_owned();
    Query::select()
        .expr(Expr::expr(scalar_subquery(declared_without_membership)).add(scalar_subquery(membership_without_declaration)))
        .to_owned()
}

/// The subscriptions without a delivery-state row plus the state rows without
/// a subscription.
fn delivery_state_drift() -> SelectStatement {
    Query::select()
        .expr(
            Expr::expr(scalar_subquery(orphans(
                Subscriptions::Table,
                Subscriptions::SubscriptionKey,
                SubscriptionDeliveryState::Table,
                SubscriptionDeliveryState::SubscriptionKey,
            )))
            .add(scalar_subquery(orphans(
                SubscriptionDeliveryState::Table,
                SubscriptionDeliveryState::SubscriptionKey,
                Subscriptions::Table,
                Subscriptions::SubscriptionKey,
            ))),
        )
        .to_owned()
}

/// Every parent-child and dictionary reference the rebuild had to preserve.
const INVARIANTS: &[Invariant] = &[
    Invariant {
        description: "history rows whose stream no longer exists",
        count: || {
            orphans(
                AttributeHistory::Table,
                AttributeHistory::StreamKey,
                TemporalAttributeStreams::Table,
                TemporalAttributeStreams::StreamKey,
            )
        },
    },
    Invariant {
        description: "streams whose temporal entity no longer exists",
        count: || {
            orphans(
                TemporalAttributeStreams::Table,
                TemporalAttributeStreams::EntityKey,
                Entities::Table,
                Entities::EntityKey,
            )
        },
    },
    Invariant {
        description: "scope history rows whose temporal entity no longer exists",
        count: || orphans(ScopeHistory::Table, ScopeHistory::EntityKey, Entities::Table, Entities::EntityKey),
    },
    Invariant {
        description: "attribute rows whose entity no longer exists",
        count: || orphans(CurrentAttributes::Table, CurrentAttributes::EntityKey, Entities::Table, Entities::EntityKey),
    },
    Invariant {
        description: "search rows whose entity no longer exists",
        count: || orphans(EntitySearch::Table, EntitySearch::EntityKey, Entities::Table, Entities::EntityKey),
    },
    Invariant {
        description: "entities without a search row",
        count: || orphans(Entities::Table, Entities::EntityKey, EntitySearch::Table, EntitySearch::EntityKey),
    },
    Invariant {
        description: "search rows naming a type key that does not exist",
        count: || {
            unresolved_keys(
                EntitySearch::Table,
                EntitySearch::EntityTypeNameKeys,
                EntityTypeNames::Table,
                EntityTypeNames::EntityTypeNameKey,
            )
        },
    },
    Invariant {
        description: "type memberships out of step with the search row's types, in either direction",
        count: membership_drift,
    },
    Invariant {
        description: "search rows naming an attribute key that does not exist",
        count: || {
            unresolved_keys(
                EntitySearch::Table,
                EntitySearch::AttributeNameKeys,
                AttributeNames::Table,
                AttributeNames::AttributeNameKey,
            )
        },
    },
    Invariant {
        description: "streams naming a dictionary key that does not exist",
        count: || {
            orphans(
                TemporalAttributeStreams::Table,
                TemporalAttributeStreams::AttributeNameKey,
                AttributeNames::Table,
                AttributeNames::AttributeNameKey,
            )
        },
    },
    Invariant {
        description: "attribute rows naming a dictionary key that does not exist",
        count: || {
            orphans(
                CurrentAttributes::Table,
                CurrentAttributes::AttributeNameKey,
                AttributeNames::Table,
                AttributeNames::AttributeNameKey,
            )
        },
    },
    Invariant {
        description: "registration information rows naming a dictionary key that does not exist",
        count: || {
            unresolved_declared(
                Query::select()
                    .expr_as(
                        unnest_of(array_cat(
                            Expr::col((RegistrationInformation::Table, RegistrationInformation::PropertyNameKeys)),
                            Expr::col((RegistrationInformation::Table, RegistrationInformation::RelationshipNameKeys)),
                        )),
                        Alias::new("declared"),
                    )
                    .from(RegistrationInformation::Table)
                    .to_owned(),
                AttributeNames::Table,
                AttributeNames::AttributeNameKey,
            )
        },
    },
    Invariant {
        description: "registrations without an information row (an information is never empty)",
        count: || orphans(Csr::Table, Csr::Id, RegistrationInformation::Table, RegistrationInformation::RegistrationId),
    },
    Invariant {
        description: "subscriptions without a delivery-state row, or state rows without a subscription",
        count: delivery_state_drift,
    },
    Invariant {
        description: "history rows populating neither instance identity column exactly once",
        count: || {
            Query::select()
                .expr(count_all())
                .from(AttributeHistory::Table)
                .and_where(
                    Expr::col(AttributeHistory::GeneratedInstanceId)
                        .is_null()
                        .eq(Expr::col(AttributeHistory::VerbatimInstanceId).is_null()),
                )
                .to_owned()
        },
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
    "temporal_entities",
    "temporal_entity_type_members",
    "temporal_stream_shapes",
];

/// Every relation an invariant reads, which has to hold rows for the count to
/// mean anything.
///
/// A referential check over an empty relation returns zero whatever the schema
/// does, so the population assertion is part of the test rather than a
/// precondition left to the suite that happened to run before it.
const POPULATED_RELATIONS: &[&str] = &[
    "entities",
    "entity_search",
    "temporal_attribute_streams",
    "attribute_history",
    "current_attributes",
    "attribute_names",
    "registration_information",
    "subscriptions",
    "subscription_delivery_state",
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

/// A registration whose information rows name dictionary keys.
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
        "snapshotQueries": [{"type": "Query", "entities": [{"type": "Device"}]}],
        "@context": [CORE_CONTEXT]
    })
}

/// Count the rows breaking one invariant.
async fn offending(pool: &PgPool, invariant: &Invariant) -> i64 {
    (invariant.count)()
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the reconciliation query must run")
}

/// The number of rows one relation holds.
async fn population(pool: &PgPool, relation: &str) -> i64 {
    Query::select()
        .expr(count_all())
        .from(Alias::new(relation))
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the counted relation must be readable")
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

    let write_id = uuid::Uuid::new_v4();
    let orphan_history = || {
        Query::select()
            .expr(count_all())
            .from(AttributeHistory::Table)
            .and_where(Expr::col(AttributeHistory::WriteId).eq(write_id))
            .and_where(
                Expr::exists(
                    Query::select()
                        .expr(int_literal(1))
                        .from(TemporalAttributeStreams::Table)
                        .and_where(
                            Expr::col((TemporalAttributeStreams::Table, TemporalAttributeStreams::StreamKey))
                                .equals((AttributeHistory::Table, AttributeHistory::StreamKey)),
                        )
                        .to_owned(),
                )
                .not(),
            )
            .to_owned()
    };
    let before: i64 = orphan_history()
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the orphan-history query must run");
    assert_eq!(before, 0, "the write identity is fresh, so it names no row yet");

    Query::insert()
        .into_table(AttributeHistory::Table)
        .columns([
            AttributeHistory::RecordedAt,
            AttributeHistory::WriteId,
            AttributeHistory::StreamKey,
            AttributeHistory::WriteOrdinal,
            AttributeHistory::AttributeKind,
            AttributeHistory::ValueKind,
            AttributeHistory::GeneratedInstanceId,
            AttributeHistory::ValueNumber,
        ])
        .values([
            now(),
            Expr::val(write_id),
            Expr::val(i64::MAX),
            Expr::val(0_i32),
            Expr::val(1_i16),
            Expr::val(1_i16),
            random_uuid(),
            cast(Expr::val(1), PostgresType::Numeric),
        ])
        .expect("one value per column")
        .to_query()
        .execute(pool)
        .await
        .expect("a history row naming no stream can be inserted, which is the state the invariant exists to catch");

    let during: i64 = orphan_history()
        .to_query_scalar()
        .fetch_one(pool)
        .await
        .expect("the orphan-history query must run");
    assert_eq!(during, 1, "the invariant must count the orphan it was given");

    Query::delete()
        .from_table(AttributeHistory::Table)
        .and_where(Expr::col(AttributeHistory::WriteId).eq(write_id))
        .to_query()
        .execute(pool)
        .await
        .expect("the orphan must be removable");

    let after: i64 = orphan_history()
        .to_query_scalar()
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
        let surviving: i64 = Query::select()
            .expr(count_all())
            .from((InformationSchema, InformationSchemaTables::Table))
            .and_where(Expr::col(InformationSchemaTables::TableSchema).eq("public"))
            .and_where(Expr::col(InformationSchemaTables::TableName).eq(*relation))
            .to_query_scalar()
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

    let duplicated_current: i64 = duplicated_identifiers(Entities::Table, Entities::Id)
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the entity relation must be readable");
    assert_eq!(duplicated_current, 0, "an identifier names at most one current Entity");

    let duplicated_temporal: i64 = duplicated_identifiers(Entities::Table, Entities::Id)
        .to_query_scalar()
        .fetch_one(scope.database_pool())
        .await
        .expect("the temporal entity relation must be readable");
    assert_eq!(duplicated_temporal, 0, "an identifier names at most one temporal Entity");

    scope.finish().await.expect("cleanup failed");
}

/// The count of identifiers `table` holds more than once.
fn duplicated_identifiers(table: impl IntoIden + Copy, id: impl IntoIden + Copy) -> SelectStatement {
    Query::select()
        .expr(count_all())
        .from_subquery(
            Query::select().column(id).from(table).group_by_col(id).and_having(count_all().gt(1)).to_owned(),
            Alias::new("duplicated"),
        )
        .to_owned()
}
