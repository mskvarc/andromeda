//! The `q`, the `q_written_members` and the `csf` v1.0.0 stored for a
//! Subscription, in the expanded spelling with each name an absolute IRI and
//! unquoted when it held no dot, read back after the upgrade as the filters
//! v1.0.0 read, in the live branch and in a Snapshot's. A Subscription holding
//! no such name keeps the row version v1.0.0 wrote.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    snapshot_records::insert_v1_0_0_snapshot,
    statements::{insert, instant, json_document, name, read_row_versions, smallint},
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::database::name_cache::NameCacheConfig;
use andromeda_context_space::{context_target::ContextTarget, snapshot_branch::SnapshotBranch};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_ngsi_ld_query::{
    filter::{Filter, attribute_path::AttributePath, query_term::QueryTerm},
    name_stage::{Expanded, Written},
};
use andromeda_postgresql_context_store::{snapshot::repo::SnapshotRepository, subscription::repo::SubscriptionRepository};
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_snapshot::store::SnapshotStore;
use andromeda_subscription::store::SubscriptionStore;
use pretty_assertions::assert_eq;
use sea_query::{Expr, ExprTrait, Query, SimpleExpr};
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;

/// The Snapshot whose branch holds a Subscription.
const SNAPSHOT: &str = "urn:ngsi-ld:Snapshot:named";

/// One Subscription row as v1.0.0 stored it, and the texts the upgrade leaves in it.
struct StoredSubscription {
    /// The Subscription's URI.
    id: &'static str,
    /// `1` for an entity Subscription, `2` for a Context Source Registration Subscription.
    kind: i16,
    /// The `q` and the `q_written_members` v1.0.0 stored, and the two after the upgrade.
    q: Option<([&'static str; 2], [&'static str; 2])>,
    /// The `csf` v1.0.0 stored, and the one after the upgrade.
    csf: Option<(&'static str, &'static str)>,
}

/// The Subscriptions of the live branch.
const SUBSCRIPTIONS: [StoredSubscription; 6] = [
    StoredSubscription {
        id: "urn:ngsi-ld:Subscription:named-weight",
        kind: 1,
        q: Some((["urn:x:weight>1000", "urn:x:weight>1000"], [r#""urn:x:weight">1000"#, r#""urn:x:weight">1000"#])),
        csf: None,
    },
    StoredSubscription {
        id: "urn:ngsi-ld:Subscription:named-conjunction",
        kind: 1,
        q: Some((
            [
                r#"ex:speed==5;"https://example.org/a".urn:x:b>1"#,
                r#"ex:speed==5;"https://example.org/a".urn:x:b>1"#,
            ],
            [
                r#""ex:speed"==5;"https://example.org/a"."urn:x:b">1"#,
                r#""ex:speed"==5;"https://example.org/a"."urn:x:b">1"#,
            ],
        )),
        csf: None,
    },
    StoredSubscription {
        id: "urn:ngsi-ld:Subscription:named-typed-hop",
        kind: 1,
        q: Some((
            [
                r#""https://example.org/rel"{"https://example.org/T1","https://example.org/T2":urn:x:b}==1"#,
                r#""https://example.org/rel"{"https://example.org/T1","https://example.org/T2":urn:x:b}==1"#,
            ],
            [
                r#""https://example.org/rel"{"https://example.org/T1","https://example.org/T2":"urn:x:b"}==1"#,
                r#""https://example.org/rel"{"https://example.org/T1","https://example.org/T2":"urn:x:b"}==1"#,
            ],
        )),
        csf: None,
    },
    StoredSubscription {
        id: "urn:ngsi-ld:Subscription:named-hop",
        kind: 1,
        q: Some((
            [r#""https://example.org/rel"{urn:x:b}==1"#, r#""https://example.org/rel"{urn:x:b}==1"#],
            [r#""https://example.org/rel"{"urn:x:b"}==1"#, r#""https://example.org/rel"{"urn:x:b"}==1"#],
        )),
        csf: None,
    },
    StoredSubscription {
        id: "urn:ngsi-ld:Subscription:named-members",
        kind: 1,
        q: Some((
            [
                r#""https://example.org/address"["https://example.org/city"]=="Berlin""#,
                r#""https://example.org/address"[ex:city]=="Berlin""#,
            ],
            [
                r#""https://example.org/address"["https://example.org/city"]=="Berlin""#,
                r#""https://example.org/address"["ex:city"]=="Berlin""#,
            ],
        )),
        csf: None,
    },
    StoredSubscription {
        id: "urn:ngsi-ld:Subscription:named-source",
        kind: 2,
        q: None,
        csf: Some((r#"urn:x:tier=="gold""#, r#""urn:x:tier"=="gold""#)),
    },
];

/// A Subscription whose `q` holds no unquoted name with a colon, which the upgrade leaves as v1.0.0 wrote it.
const UNTOUCHED: StoredSubscription = StoredSubscription {
    id: "urn:ngsi-ld:Subscription:named-untouched",
    kind: 1,
    q: Some((
        [r#""https://example.org/speed">50"#, r#""https://example.org/speed">50"#],
        [r#""https://example.org/speed">50"#, r#""https://example.org/speed">50"#],
    )),
    csf: None,
};

/// Inserts `subscription` as v1.0.0 wrote it into the branch `pool` reaches.
async fn insert_subscription(pool: &PgPool, subscription: &StoredSubscription) -> Result<(), UpgradeTestError> {
    let mut row: Vec<(&str, SimpleExpr)> = vec![
        ("id", Expr::val(subscription.id)),
        ("kind", smallint(subscription.kind)),
        ("status", smallint(1)),
        ("created_at", instant("2025-01-27T21:00:00Z")),
        ("modified_at", instant("2025-01-27T21:00:00Z")),
        ("active_since", instant("2025-01-27T21:00:00Z")),
        ("selected_since", instant("2025-01-27T21:00:00Z")),
        (
            "notification",
            json_document(&json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})),
        ),
    ];
    if subscription.kind == 2 {
        row.push(("context_source_revision", Expr::val(0_i64)));
    }
    if let Some(([q, written_members], _)) = subscription.q {
        row.push(("q", Expr::val(q)));
        row.push(("q_written_members", Expr::val(written_members)));
    }
    if let Some((csf, _)) = subscription.csf {
        row.push(("csf", Expr::val(csf)));
    }
    insert(pool, "subscriptions", row).await
}

/// The `q`, the `q_written_members` and the `csf` the row of the Subscription `id` holds.
async fn stored_texts(pool: &PgPool, id: &str) -> Result<(Option<String>, Option<String>, Option<String>), UpgradeTestError> {
    let select = Query::select()
        .columns([name("q"), name("q_written_members"), name("csf")])
        .from(name("subscriptions"))
        .and_where(Expr::col(name("id")).eq(id))
        .to_owned();
    Ok(select.to_query_as::<(Option<String>, Option<String>, Option<String>)>().fetch_one(pool).await?)
}

/// The filter this build reads from the stored text `text`, the reading the broker's repository makes of a stored `q` or `csf`.
fn read_as_stored(text: &str) -> Result<Filter<Expanded>, UpgradeTestError> {
    Ok(text.parse::<Filter<Written>>()?.into_expanded()?)
}

/// Expects every Subscription of `SUBSCRIPTIONS` in the branch `rows` reaches to hold the texts the upgrade leaves, and each entity Subscription to read back through the broker's own repository on the tenant `pool`, from the branch `target` names, with them.
async fn expect_quoted_subscriptions(pool: &PgPool, rows: &PgPool, target: &ContextTarget) -> Result<(), UpgradeTestError> {
    let attribute_names = Arc::new(NameCache::<AttributeName>::new(pool, &NameCacheConfig::default()));
    let store = SubscriptionRepository::new(pool, attribute_names);
    for subscription in &SUBSCRIPTIONS {
        let (q, written_members, csf) = stored_texts(rows, subscription.id).await?;
        let expected_q = subscription.q.map(|(_, [q, written_members])| (q.to_owned(), written_members.to_owned()));
        assert_eq!(q.zip(written_members), expected_q, "{}", subscription.id);
        assert_eq!(csf.as_deref(), subscription.csf.map(|(_, read)| read), "{}", subscription.id);
        if let Some((_, read)) = subscription.csf {
            assert_eq!(read_as_stored(read)?.to_string(), read, "{}", subscription.id);
        }
        if subscription.kind == 1 {
            let read = store
                .get(target, &subscription.id.parse::<Identifier>()?)
                .await?
                .ok_or(UpgradeTestError::Missing { what: "a named subscription" })?
                .subscription;
            let expected = subscription.q.map(|(_, [q, _])| q);
            assert_eq!(read.filters.q.map(|q| q.to_string()).as_deref(), expected, "{}", subscription.id);
        }
    }
    Ok(())
}

/// v1.0.0 read `rel{urn:x:b}` with no type list, a lone word inside the braces being a name, and so does this build once the name is quoted.
#[test]
fn a_quoted_name_inside_a_hop_reads_as_the_name_v1_0_0_read() -> Result<(), UpgradeTestError> {
    let Filter::Term(QueryTerm::Equality {
        path: AttributePath::Linked { guard_types, inner, .. },
        ..
    }) = read_as_stored(r#""https://example.org/rel"{"urn:x:b"}==1"#)?
    else {
        return Err(UpgradeTestError::Missing { what: "a hop" });
    };
    assert!(guard_types.is_empty(), "{guard_types:?}");
    assert_eq!(inner.terminal().attribute.as_str(), "urn:x:b");
    Ok(())
}

#[tokio::test]
async fn the_names_holding_a_colon_of_a_live_subscription_read_back_quoted_as_the_filters_v1_0_0_read() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    for subscription in &SUBSCRIPTIONS {
        insert_subscription(database.pool(), subscription).await?;
    }

    database.migrate_to_current().await?;

    expect_quoted_subscriptions(database.pool(), database.pool(), &ContextTarget::Live).await?;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn the_names_holding_a_colon_of_a_snapshot_subscription_read_back_quoted_as_in_the_live_branch() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let key = insert_v1_0_0_snapshot(&database, SNAPSHOT, SnapshotBranch::Published).await?;
    let pool = database.branch_pool(key, SnapshotBranch::Published).await?;
    for subscription in &SUBSCRIPTIONS {
        insert_subscription(&pool, subscription).await?;
    }
    pool.close().await;

    database.migrate_to_current().await?;

    let target = SnapshotRepository::new(database.pool())
        .target(&SNAPSHOT.parse::<Identifier>()?)
        .await?
        .ok_or(UpgradeTestError::Missing {
            what: "the branch of the named Snapshot",
        })?;
    let branch = database.branch_pool(key, SnapshotBranch::Published).await?;
    expect_quoted_subscriptions(database.pool(), &branch, &ContextTarget::Snapshot(target)).await?;
    branch.close().await;
    database.drop_database().await?;
    Ok(())
}

#[tokio::test]
async fn a_subscription_holding_no_unquoted_name_with_a_colon_keeps_the_row_version_v1_0_0_wrote() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_subscription(database.pool(), &UNTOUCHED).await?;
    let before = read_row_versions(database.pool(), "subscriptions").await?;

    database.migrate_to_current().await?;

    assert_eq!(read_row_versions(database.pool(), "subscriptions").await?, before);
    database.drop_database().await?;
    Ok(())
}
