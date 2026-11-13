//! The statements over `forwarded_subscription_claims`, the placements of
//! subscription copies whose source has not answered yet.
//!
//! A claim row is keyed like the copy it claims and goes with it. It holds
//! the instant the placement's lease ends, which the database clock sets, so
//! every placement and every pass compares leases against one clock.

use crate::{store_fault::persistence_error, subscription_store::surrogate_key::subscription_key_of};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::{
    subscription_forwarding::{ForwardedSubscriptionClaims, ForwardedSubscriptions},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{clock::span_after_transaction_start, scalar_subquery::scalar_subquery};
use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, InsertStatement, JoinType, Order, Query, SelectStatement, UpdateStatement};
use std::time::Duration;

/// The alias the stale read takes the claims under.
const CLAIM: &str = "c";

/// The alias the stale read joins the copies under.
const COPY: &str = "f";

/// The alias the stale read joins `subscriptions` under.
const SUBSCRIPTION: &str = "s";

/// Claim the copy of `local` just recorded at `registration` under a lease of
/// `lease`.
///
/// A `local` no subscription carries resolves to NULL, which the key column
/// refuses, so claiming a copy of a missing subscription fails.
pub(crate) fn lease_claim_statement(local: &Identifier, registration: &Identifier, lease: Duration) -> Result<InsertStatement, SubscriptionForwardingError> {
    let mut insert = Query::insert();
    insert
        .into_table(ForwardedSubscriptionClaims::Table)
        .columns([
            ForwardedSubscriptionClaims::LocalSubscriptionKey,
            ForwardedSubscriptionClaims::LeaseUntil,
            ForwardedSubscriptionClaims::RegistrationId,
        ])
        .values([
            scalar_subquery(subscription_key_of(local)),
            span_after_transaction_start(lease),
            Expr::val(registration.as_str()),
        ])
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?;
    Ok(insert)
}

/// Take over the claim of the copy of `local` at `registration` when its
/// lease has ended, starting a lease of `lease`, and return the identifier the
/// copy was recorded with.
///
/// Nothing is written, and nothing returned, when the copy holds no claim
/// (it is settled) or its claim is still under its lease.
pub(crate) fn take_over_claim_statement(local: &Identifier, registration: &Identifier, lease: Duration) -> UpdateStatement {
    Query::update()
        .table(ForwardedSubscriptionClaims::Table)
        .value(ForwardedSubscriptionClaims::LeaseUntil, span_after_transaction_start(lease))
        .from(ForwardedSubscriptions::Table)
        .and_where(
            Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::LocalSubscriptionKey))
                .equals((ForwardedSubscriptions::Table, ForwardedSubscriptions::LocalSubscriptionKey)),
        )
        .and_where(
            Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::RegistrationId))
                .equals((ForwardedSubscriptions::Table, ForwardedSubscriptions::RegistrationId)),
        )
        .and_where(Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::LocalSubscriptionKey)).in_subquery(subscription_key_of(local)))
        .and_where(Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::RegistrationId)).eq(registration.as_str()))
        .and_where(Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::LeaseUntil)).lt(Expr::current_timestamp()))
        .returning(Query::returning().column((ForwardedSubscriptions::Table, ForwardedSubscriptions::RemoteSubscriptionId)))
        .to_owned()
}

/// Settle the copy of `local` at `registration` by removing its claim.
pub(crate) fn settle_claim_statement(local: &Identifier, registration: &Identifier) -> DeleteStatement {
    Query::delete()
        .from_table(ForwardedSubscriptionClaims::Table)
        .and_where(Expr::col(ForwardedSubscriptionClaims::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .and_where(Expr::col(ForwardedSubscriptionClaims::RegistrationId).eq(registration.as_str()))
        .to_owned()
}

/// The local subscription URI, registration and recorded identifier of up to
/// `limit` claims whose lease has ended, the longest overdue first, served by
/// `idx_forwarded_subscription_claims_claim`.
pub(crate) fn stale_claims_statement(limit: u32) -> SelectStatement {
    let claim = Alias::new(CLAIM);
    let copy = Alias::new(COPY);
    let subscription = Alias::new(SUBSCRIPTION);
    Query::select()
        .column((subscription.clone(), Subscriptions::Id))
        .column((claim.clone(), ForwardedSubscriptionClaims::RegistrationId))
        .column((copy.clone(), ForwardedSubscriptions::RemoteSubscriptionId))
        .from_as(ForwardedSubscriptionClaims::Table, claim.clone())
        .join_as(
            JoinType::InnerJoin,
            ForwardedSubscriptions::Table,
            copy.clone(),
            Expr::col((copy.clone(), ForwardedSubscriptions::LocalSubscriptionKey))
                .equals((claim.clone(), ForwardedSubscriptionClaims::LocalSubscriptionKey))
                .and(Expr::col((copy, ForwardedSubscriptions::RegistrationId)).equals((claim.clone(), ForwardedSubscriptionClaims::RegistrationId))),
        )
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((claim.clone(), ForwardedSubscriptionClaims::LocalSubscriptionKey)),
        )
        .and_where(Expr::col((claim.clone(), ForwardedSubscriptionClaims::LeaseUntil)).lt(Expr::current_timestamp()))
        .order_by((claim, ForwardedSubscriptionClaims::LeaseUntil), Order::Asc)
        .limit(u64::from(limit))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::subscription_forwarding::claim_statements::{lease_claim_statement, settle_claim_statement, stale_claims_statement, take_over_claim_statement};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Value};
    use std::time::Duration;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a URI")
    }

    /// The key lookup every statement embeds for the local subscription.
    const LOCAL_KEY: &str = r#"(SELECT "subscription_key" FROM "subscriptions" WHERE "id" = 'urn:ngsi-ld:Subscription:local')"#;

    #[test]
    fn a_claim_holds_a_lease_ending_the_lease_after_now() {
        let sql = lease_claim_statement(
            &uri("urn:ngsi-ld:Subscription:local"),
            &uri("urn:ngsi-ld:ContextSourceRegistration:b"),
            Duration::from_secs(60),
        )
        .expect("the claim builds")
        .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"INSERT INTO "forwarded_subscription_claims" ("local_subscription_key", "lease_until", "registration_id") VALUES ({LOCAL_KEY}, CURRENT_TIMESTAMP + (make_interval(secs => 60)), 'urn:ngsi-ld:ContextSourceRegistration:b')"#
            )
        );
    }

    /// The lease length reaches the server as a bound value.
    #[test]
    fn the_lease_of_a_claim_is_bound_as_seconds() {
        let (sql, values) = lease_claim_statement(
            &uri("urn:ngsi-ld:Subscription:local"),
            &uri("urn:ngsi-ld:ContextSourceRegistration:b"),
            Duration::from_millis(1500),
        )
        .expect("the claim builds")
        .build(PostgresQueryBuilder);
        assert!(sql.contains("CURRENT_TIMESTAMP + (make_interval(secs => $2))"), "{sql}");
        assert!(values.iter().any(|value| *value == Value::Double(Some(1.5))), "{values:?}");
    }

    /// Only a claim whose lease has ended is taken over, and the copy is then
    /// sent under the identifier recorded with it. A settled copy holds no
    /// claim, so the update finds nothing to take.
    #[test]
    fn only_a_claim_whose_lease_has_ended_is_taken_over_returning_the_identifier_recorded() {
        let sql = take_over_claim_statement(
            &uri("urn:ngsi-ld:Subscription:local"),
            &uri("urn:ngsi-ld:ContextSourceRegistration:b"),
            Duration::from_secs(60),
        )
        .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                concat!(
                    r#"UPDATE "forwarded_subscription_claims" SET "lease_until" = CURRENT_TIMESTAMP + (make_interval(secs => 60)) FROM "forwarded_subscriptions" "#,
                    r#"WHERE "forwarded_subscription_claims"."local_subscription_key" = "forwarded_subscriptions"."local_subscription_key" "#,
                    r#"AND "forwarded_subscription_claims"."registration_id" = "forwarded_subscriptions"."registration_id" "#,
                    r#"AND "forwarded_subscription_claims"."local_subscription_key" IN {} "#,
                    r#"AND "forwarded_subscription_claims"."registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b' "#,
                    r#"AND "forwarded_subscription_claims"."lease_until" < CURRENT_TIMESTAMP "#,
                    r#"RETURNING "forwarded_subscriptions"."remote_subscription_id""#
                ),
                LOCAL_KEY
            )
        );
    }

    #[test]
    fn a_settled_copy_loses_its_claim_by_the_key_of_its_local_subscription_and_its_registration() {
        let sql =
            settle_claim_statement(&uri("urn:ngsi-ld:Subscription:local"), &uri("urn:ngsi-ld:ContextSourceRegistration:b")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"DELETE FROM "forwarded_subscription_claims" WHERE "local_subscription_key" IN {LOCAL_KEY} AND "registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b'"#
            )
        );
    }

    #[test]
    fn the_stale_claims_are_read_with_their_subscription_uri_and_copy_identifier_longest_overdue_first() {
        let sql = stale_claims_statement(100).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            concat!(
                r#"SELECT "s"."id", "c"."registration_id", "f"."remote_subscription_id" FROM "forwarded_subscription_claims" AS "c" "#,
                r#"INNER JOIN "forwarded_subscriptions" AS "f" ON "f"."local_subscription_key" = "c"."local_subscription_key" AND "f"."registration_id" = "c"."registration_id" "#,
                r#"INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "c"."local_subscription_key" "#,
                r#"WHERE "c"."lease_until" < CURRENT_TIMESTAMP ORDER BY "c"."lease_until" ASC LIMIT 100"#
            )
        );
    }
}
