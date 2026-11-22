//! A Subscription `q` v1.0.0 stored with its patterns in quotes reads back
//! after the upgrade with each pattern bare, as clause 4.9 writes a `RegExp`,
//! and a pattern the bare form would end early or trim keeps its quotes. The
//! stored text reads as the same filter, so the upgrade rewrites no row.

use crate::fixtures::{
    rehearsal::at_v1_0_0,
    statements::{insert, instant, json_document, smallint},
    test_error::UpgradeTestError,
};
use andromeda_configuration::schema::database::name_cache::NameCacheConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_postgresql_context_store::subscription::repo::SubscriptionRepository;
use andromeda_postgresql_name_dictionary::name_cache::NameCache;
use andromeda_subscription::store::SubscriptionStore;
use pretty_assertions::assert_eq;
use sea_query::Expr;
use serde_json::json;
use sqlx::PgPool;
use std::sync::Arc;

/// Each `q` v1.0.0 stored, which v1.0.0 also stored as the `q` with its
/// members as written, beside the one this build reads after the upgrade.
const STORED_AND_READ: [(&str, &str); 5] = [
    (
        r#""https://example.com/brandName"~="^Mer(c|z)edes$""#,
        r#""https://example.com/brandName"~=^Mer(c|z)edes$"#,
    ),
    (
        r#""https://example.com/brandName"!~="^Merc";"https://example.com/speed">50"#,
        r#""https://example.com/brandName"!~=^Merc;"https://example.com/speed">50"#,
    ),
    (
        r#""https://example.com/brandName"~="^example\.com$""#,
        r#""https://example.com/brandName"~=^example\.com$"#,
    ),
    (r#""https://example.com/brandName"~="a|b""#, r#""https://example.com/brandName"~="a|b""#),
    (r#""https://example.com/brandName"~=" Merc""#, r#""https://example.com/brandName"~=" Merc""#),
];

/// Inserts the entity subscription `id` with the `q` `q`, as v1.0.0 wrote it.
async fn insert_filtered_subscription(pool: &PgPool, id: &str, q: &str) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "subscriptions",
        vec![
            ("id", Expr::val(id)),
            ("kind", smallint(1)),
            ("status", smallint(1)),
            ("created_at", instant("2025-01-27T21:00:00Z")),
            ("modified_at", instant("2025-01-27T21:00:00Z")),
            ("active_since", instant("2025-01-27T21:00:00Z")),
            ("selected_since", instant("2025-01-27T21:00:00Z")),
            (
                "notification",
                json_document(&json!({"endpoint": {"uri": "https://receiver.example.com/notify"}})),
            ),
            ("q", Expr::val(q)),
            ("q_written_members", Expr::val(q)),
        ],
    )
    .await
}

/// The URI of the subscription storing the `index`th `q`.
fn subscription_id(index: usize) -> String {
    format!("urn:ngsi-ld:Subscription:patterned-{index}")
}

#[tokio::test]
async fn a_q_holding_quoted_patterns_reads_back_with_each_pattern_bare_unless_the_bare_form_would_change_it() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    for (index, (stored, _)) in STORED_AND_READ.iter().enumerate() {
        insert_filtered_subscription(database.pool(), &subscription_id(index), stored).await?;
    }

    database.migrate_to_current().await?;

    let attribute_names = Arc::new(NameCache::<AttributeName>::new(database.pool(), &NameCacheConfig::default()));
    let store = SubscriptionRepository::new(database.pool(), attribute_names);
    for (index, (stored, read)) in STORED_AND_READ.iter().enumerate() {
        let id = subscription_id(index).parse::<Identifier>()?;
        let subscription = store
            .get(&ContextTarget::Live, &id)
            .await?
            .ok_or(UpgradeTestError::Missing {
                what: "a filtered subscription",
            })?
            .subscription;
        assert_eq!(subscription.filters.q.map(|q| q.to_string()).as_deref(), Some(*read), "stored as {stored}");
    }
    database.drop_database().await?;
    Ok(())
}
