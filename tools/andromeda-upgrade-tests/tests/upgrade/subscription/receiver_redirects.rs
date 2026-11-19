//! The companion and the copies a v1.0.0 tenant placed notify the receiver
//! v1.0.0 served, below its path for the notifications of context sources.
//! After the upgrade the tenant is due at once while it has not adopted the
//! receiver this build serves, and adopting it owes that receiver to the
//! companion and to every copy whose claim is settled, never to a copy whose
//! claim is open. A companion or copy updated owes nothing, and adopting the
//! same receiver again owes nothing anew.

use crate::fixtures::{
    registrations::insert_registration,
    rehearsal::at_v1_0_0,
    statements::{insert, insert_returning, instant, json_document, smallint},
    test_error::UpgradeTestError,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_context_store::{pending_work::earliest_obligation, subscription_forwarding::repo::SubscriptionForwardingRepository};
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_subscription_forwarding::{
    receiver::ReceiverEndpoint,
    redirect_ledger::{OwedRedirect, RedirectLedger, RedirectTarget},
};
use pretty_assertions::assert_eq;
use sea_query::Expr;
use serde_json::json;
use sqlx::PgPool;
use std::time::SystemTime;
use url::Url;

/// The origin the hub advertises before and after the upgrade.
const ADVERTISED: &str = "http://hub.example:1056";

/// The receiver v1.0.0 placed its companions and copies with.
const V1_0_0_RECEIVER: &str = "http://hub.example:1056/ngsi-ld/v1/notifications";

/// The local subscription.
const LOCAL: &str = "urn:ngsi-ld:Subscription:local";

/// The companion mirroring the local subscription.
const COMPANION: &str = "urn:ngsi-ld:Subscription:companion";

/// The registration of the source holding the copy v1.0.0 placed.
const SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:placed";

/// The copy v1.0.0 placed.
const COPY: &str = "urn:ngsi-ld:Subscription:copy";

/// The registration of the source a copy is being placed at after the upgrade.
const CLAIMED_SOURCE: &str = "urn:ngsi-ld:ContextSourceRegistration:claimed";

/// The copy being placed after the upgrade, its claim still open.
const CLAIMED_COPY: &str = "urn:ngsi-ld:Subscription:claimed";

/// Inserts the subscription `id` of `kind` (1 an entity subscription, 2 a
/// context source registration subscription) notifying `endpoint`, as v1.0.0
/// wrote it, and returns its key.
async fn insert_subscription(pool: &PgPool, id: &str, kind: i16, endpoint: &str) -> Result<i64, UpgradeTestError> {
    let mut row = vec![
        ("id", Expr::val(id)),
        ("kind", smallint(kind)),
        ("status", smallint(1)),
        ("created_at", instant("2025-01-27T21:00:00Z")),
        ("modified_at", instant("2025-01-27T21:00:00Z")),
        ("active_since", instant("2025-01-27T21:00:00Z")),
        ("selected_since", instant("2025-01-27T21:00:00Z")),
        ("notification", json_document(&json!({"endpoint": {"uri": endpoint}}))),
    ];
    if kind == 2 {
        row.push(("context_source_revision", Expr::val(0_i64)));
    }
    insert_returning(pool, "subscriptions", "subscription_key", row).await
}

/// Inserts the copy `remote` of the subscription `local` at `registration`'s
/// source.
async fn insert_copy(pool: &PgPool, local: i64, registration: &str, remote: &str) -> Result<(), UpgradeTestError> {
    insert(
        pool,
        "forwarded_subscriptions",
        vec![
            ("local_subscription_key", Expr::val(local)),
            ("registration_id", Expr::val(registration)),
            ("remote_subscription_id", Expr::val(remote)),
        ],
    )
    .await
}

/// The identifier `raw` names.
fn uri(raw: &str) -> Result<Identifier, UpgradeTestError> {
    Ok(raw.parse()?)
}

/// The receiver this build serves below [`ADVERTISED`].
fn current_receiver() -> Result<ReceiverEndpoint, UpgradeTestError> {
    ReceiverEndpoint::from_advertised(&Url::parse(ADVERTISED)?).ok_or(UpgradeTestError::Missing { what: "the receiver" })
}

#[tokio::test]
async fn a_v1_0_0_companion_and_settled_copy_owe_the_receiver_this_build_serves_once_it_is_adopted() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let local = insert_subscription(database.pool(), LOCAL, 1, "https://subscriber.example.com/notify").await?;
    let companion = insert_subscription(database.pool(), COMPANION, 2, V1_0_0_RECEIVER).await?;
    insert(
        database.pool(),
        "subscription_companions",
        vec![
            ("local_subscription_key", Expr::val(local)),
            ("companion_subscription_key", Expr::val(companion)),
        ],
    )
    .await?;
    insert_registration(database.pool(), SOURCE, &json!({})).await?;
    insert_copy(database.pool(), local, SOURCE, COPY).await?;

    database.migrate_to_current().await?;

    insert_registration(database.pool(), CLAIMED_SOURCE, &json!({})).await?;
    insert_copy(database.pool(), local, CLAIMED_SOURCE, CLAIMED_COPY).await?;
    insert(
        database.pool(),
        "forwarded_subscription_claims",
        vec![
            ("local_subscription_key", Expr::val(local)),
            ("lease_until", instant("2999-01-01T00:00:00Z")),
            ("registration_id", Expr::val(CLAIMED_SOURCE)),
        ],
    )
    .await?;
    let receiver = current_receiver()?;
    let due = earliest_obligation(database.pool(), Some(&receiver))
        .await
        .map_err(UpgradeTestError::Obligation)?;
    assert!(
        due.is_some_and(|due| due <= SystemTime::now()),
        "a receiver not yet adopted is due at once: {due:?}"
    );

    let ledger = SubscriptionForwardingRepository::new(database.pool());
    ledger.adopt_receiver(&receiver).await?;
    let copy = RedirectTarget::Copy(ForwardedCopy {
        registration: uri(SOURCE)?,
        remote_subscription: uri(COPY)?,
    });
    assert_eq!(
        ledger.overdue_redirects(100).await?,
        [
            OwedRedirect {
                local: uri(LOCAL)?,
                target: RedirectTarget::Companion,
            },
            OwedRedirect {
                local: uri(LOCAL)?,
                target: copy.clone(),
            },
        ]
    );

    ledger.redirected(&uri(LOCAL)?, &RedirectTarget::Companion).await?;
    ledger.redirected(&uri(LOCAL)?, &copy).await?;
    ledger.adopt_receiver(&receiver).await?;
    assert_eq!(ledger.overdue_redirects(100).await?, []);
    let due = earliest_obligation(database.pool(), Some(&receiver))
        .await
        .map_err(UpgradeTestError::Obligation)?;
    assert!(due.is_some_and(|due| due > SystemTime::now()), "only the open claim's lease is left: {due:?}");
    database.drop_database().await?;
    Ok(())
}
