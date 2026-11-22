//! A Subscription `scopeQ` v1.0.0 stored with the bare `/#` beside other
//! Scopes, which clause 4.19 admits only as a whole scope query, reads back
//! after the upgrade as the expression it meant. Every other `scopeQ` reads
//! back as stored.

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

/// Each `scopeQ` v1.0.0 stored, beside the one this build reads after the
/// upgrade.
///
/// `/#` beside other alternatives selects what `/#` alone selects, and inside
/// a group it adds nothing to the Scopes beside it, each of which already
/// requires a Scope.
const STORED_AND_READ: [(&str, &str); 9] = [
    ("/#,/CompanyA", "/#"),
    ("/CompanyA,/#", "/#"),
    ("(/#;/CompanyA),/CompanyB", "/CompanyA,/CompanyB"),
    ("(/CompanyA;/#;/Madrid/#)", "(/CompanyA;/Madrid/#)"),
    ("/CompanyB,(/#;/#)", "/#"),
    ("/#", "/#"),
    ("/Madrid/#", "/Madrid/#"),
    ("(/Madrid/#;/CompanyA)", "(/Madrid/#;/CompanyA)"),
    ("/Madrid/+/#,(/A;/B)", "/Madrid/+/#,(/A;/B)"),
];

/// Inserts the entity subscription `id` with the `scopeQ` `scope_q`, as v1.0.0
/// wrote it.
async fn insert_scoped_subscription(pool: &PgPool, id: &str, scope_q: &str) -> Result<(), UpgradeTestError> {
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
            ("scope_q", Expr::val(scope_q)),
        ],
    )
    .await
}

/// The URI of the subscription storing the `index`th `scopeQ`.
fn subscription_id(index: usize) -> String {
    format!("urn:ngsi-ld:Subscription:scoped-{index}")
}

#[tokio::test]
async fn a_scope_query_holding_the_bare_any_scope_beside_other_scopes_reads_back_as_the_expression_it_meant() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    for (index, (stored, _)) in STORED_AND_READ.iter().enumerate() {
        insert_scoped_subscription(database.pool(), &subscription_id(index), stored).await?;
    }

    database.migrate_to_current().await?;

    let attribute_names = Arc::new(NameCache::<AttributeName>::new(database.pool(), &NameCacheConfig::default()));
    let store = SubscriptionRepository::new(database.pool(), attribute_names);
    for (index, (stored, read)) in STORED_AND_READ.iter().enumerate() {
        let id = subscription_id(index).parse::<Identifier>()?;
        let subscription = store
            .get(&ContextTarget::Live, &id)
            .await?
            .ok_or(UpgradeTestError::Missing { what: "a scoped subscription" })?
            .subscription;
        assert_eq!(
            subscription.filters.scope_q.map(|scope_q| scope_q.to_string()).as_deref(),
            Some(*read),
            "stored as {stored}"
        );
    }
    database.drop_database().await?;
    Ok(())
}
