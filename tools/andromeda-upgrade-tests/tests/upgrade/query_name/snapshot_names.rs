//! The `q`, the `csf` and the `orderBy` terms of the clause 5.2.23 Queries a
//! v1.0.0 Snapshot was captured by, stored in the spelling their creator
//! wrote, read back after the upgrade with each name holding a colon quoted,
//! as the filters v1.0.0 read.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    snapshot_records::{insert_v1_0_0_snapshot, store_v1_0_0_snapshot_queries},
    test_error::UpgradeTestError,
};
use andromeda_context_space::snapshot_branch::SnapshotBranch;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::query::body::Query;
use andromeda_ngsi_ld_query::filter::{Filter, attribute_path::AttributePath, query_term::QueryTerm};
use andromeda_postgresql_context_store::snapshot::repo::SnapshotRepository;
use andromeda_snapshot::store::SnapshotStore;
use pretty_assertions::assert_eq;
use serde_json::{Value, json};

/// The Snapshot every rehearsal of this file stores.
const SNAPSHOT: &str = "urn:ngsi-ld:Snapshot:named-queries";

/// Each `q` v1.0.0 stored beside the one this build reads after the upgrade.
///
/// v1.0.0 opened a type list only where a comma followed the first word, so a
/// lone `X:y` inside the braces was a prefixed attribute name.
const QUERIES: [(&str, &str); 3] = [
    ("sensor{ex:humidity}==40", r#"sensor{"ex:humidity"}==40"#),
    (r#"isParkedIn{Building,Store:category}=="x""#, r#"isParkedIn{Building,Store:category}=="x""#),
    (r#"rel{Building,"ex:Store":ex:category}=="x""#, r#"rel{Building,"ex:Store":"ex:category"}=="x""#),
];

/// The `orderBy` terms v1.0.0 stored beside the ones this build reads.
const ORDER_BY: [(&str, &str); 3] = [
    ("ex:speed;desc", r#""ex:speed";desc"#),
    ("ex:a.observedAt", r#""ex:a".observedAt"#),
    ("name", "name"),
];

/// The `csf` v1.0.0 stored beside the one this build reads, an unquoted
/// absolute IRI and a prefixed name, one in each Query term.
const CSF: (&str, &str) = (r#"urn:x:tier=="gold";ex:region=="north""#, r#""urn:x:tier"=="gold";"ex:region"=="north""#);

/// The Queries v1.0.0 stored for the Snapshot, one for each stored `q`, each
/// with the stored `csf` and ordered by every stored term.
fn stored_queries() -> Value {
    let order_by: Vec<&str> = ORDER_BY.iter().map(|(stored, _)| *stored).collect();
    let (csf, _) = CSF;
    Value::Array(
        QUERIES
            .iter()
            .map(|(stored, _)| json!({"type": "Query", "q": stored, "csf": csf, "ordering": {"orderBy": order_by}}))
            .collect(),
    )
}

/// Expects `queries` to hold one Query for each stored `q`, in order, each with the `q`, the `csf` and the `orderBy` terms this build reads.
fn expect_queries_read_back(queries: Option<&[Query]>, member: &str) -> Result<(), UpgradeTestError> {
    let queries = queries.ok_or(UpgradeTestError::Missing {
        what: "the queries of a Snapshot",
    })?;
    assert_eq!(queries.len(), QUERIES.len(), "{member}");
    let (_, csf) = CSF;
    for (query, (stored, read)) in queries.iter().zip(QUERIES) {
        assert_eq!(query.q.as_ref().map(ToString::to_string).as_deref(), Some(read), "{member} stored as {stored}");
        assert_eq!(query.csf.as_ref().map(ToString::to_string).as_deref(), Some(csf), "{member} stored as {stored}");
        let terms: Vec<String> = query
            .ordering
            .as_ref()
            .map_or_default(|ordering| ordering.order_by.iter().map(ToString::to_string).collect());
        let expected: Vec<&str> = ORDER_BY.iter().map(|(_, read)| *read).collect();
        assert_eq!(terms, expected, "{member} stored as {stored}");
    }
    let Some(Filter::Term(QueryTerm::Equality {
        path: AttributePath::Linked { guard_types, inner, .. },
        ..
    })) = queries.first().and_then(|query| query.q.as_ref())
    else {
        return Err(UpgradeTestError::Missing { what: "a hop" });
    };
    assert!(guard_types.is_empty(), "{member}: {guard_types:?}");
    assert_eq!(inner.terminal().attribute.as_str(), "ex:humidity", "{member}");
    Ok(())
}

#[tokio::test]
async fn the_names_holding_a_colon_of_the_queries_a_snapshot_was_captured_by_read_back_quoted_as_v1_0_0_read_them() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    insert_v1_0_0_snapshot(&database, SNAPSHOT, SnapshotBranch::Published).await?;
    let stored = stored_queries();
    store_v1_0_0_snapshot_queries(database.pool(), SNAPSHOT, &stored, &stored).await?;

    database.migrate_to_current().await?;

    let snapshot = SnapshotRepository::new(database.pool())
        .get(&SNAPSHOT.parse::<Identifier>()?)
        .await?
        .ok_or(UpgradeTestError::Missing { what: "the named Snapshot" })?;
    expect_queries_read_back(snapshot.snapshot_queries.as_deref(), "snapshotQueries")?;
    expect_queries_read_back(snapshot.snapshot_temporal_queries.as_deref(), "snapshotTemporalQueries")?;
    database.drop_database().await?;
    Ok(())
}
