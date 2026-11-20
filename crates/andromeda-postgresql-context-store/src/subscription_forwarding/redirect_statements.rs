//! The statements over the receiver the companions and copies notify, and the
//! companions and copies owed the current one.
//!
//! A companion owes the current receiver while
//! `subscription_companion_redirects` holds a row for it, a copy while
//! `forwarded_subscription_redirects` holds one, and each row holds the instant
//! its update is next due on the database clock, which every pass and every
//! obligation read compares against.

use crate::{store_fault::persistence_error, subscription_store::surrogate_key::subscription_key_of};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::{
    subscription_forwarding::{
        ForwardedSubscriptionClaims,
        ForwardedSubscriptionRedirects,
        ForwardedSubscriptions,
        SubscriptionCompanionRedirects,
        SubscriptionCompanions,
        SubscriptionReceiver,
    },
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::clock::{now, span_after_transaction_start};
use andromeda_subscription_forwarding::{error::SubscriptionForwardingError, receiver::ReceiverEndpoint};
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, InsertStatement, JoinType, LockType, OnConflict, Order, Query, SelectStatement, UpdateStatement};
use std::time::Duration;

/// The alias a read takes the owing rows under.
const OWING: &str = "o";

/// The alias the overdue copies read joins the copies under.
const COPY: &str = "f";

/// The alias a read joins `subscriptions` under for a subscription's URI.
const SUBSCRIPTION: &str = "s";

/// The receiver adopted last, locked for the adoption's transaction.
pub(crate) fn adopted_receiver_statement() -> SelectStatement {
    Query::select()
        .column(SubscriptionReceiver::Receiver)
        .from(SubscriptionReceiver::Table)
        .lock(LockType::Update)
        .to_owned()
}

/// Record `receiver` as the one adopted last.
pub(crate) fn record_receiver_statement(receiver: &ReceiverEndpoint) -> Result<InsertStatement, SubscriptionForwardingError> {
    let mut insert = Query::insert();
    insert
        .into_table(SubscriptionReceiver::Table)
        .columns([SubscriptionReceiver::Receiver])
        .values([Expr::val(receiver.url().as_str())])
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
        .on_conflict(
            OnConflict::column(SubscriptionReceiver::Singleton)
                .update_column(SubscriptionReceiver::Receiver)
                .to_owned(),
        );
    Ok(insert)
}

/// Owe the current receiver from now to every companion that owes nothing.
pub(crate) fn owe_companions_statement() -> Result<InsertStatement, SubscriptionForwardingError> {
    let companions = Query::select()
        .column(SubscriptionCompanions::LocalSubscriptionKey)
        .expr(now())
        .from(SubscriptionCompanions::Table)
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(SubscriptionCompanionRedirects::Table)
        .columns([
            SubscriptionCompanionRedirects::LocalSubscriptionKey,
            SubscriptionCompanionRedirects::RedirectDueAt,
        ])
        .select_from(companions)
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
        .on_conflict(OnConflict::column(SubscriptionCompanionRedirects::LocalSubscriptionKey).do_nothing().to_owned());
    Ok(insert)
}

/// Owe the current receiver from now to every copy whose claim is settled and
/// that owes nothing.
pub(crate) fn owe_settled_copies_statement() -> Result<InsertStatement, SubscriptionForwardingError> {
    let claim = Query::select()
        .expr(Expr::val(1))
        .from(ForwardedSubscriptionClaims::Table)
        .and_where(
            Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::LocalSubscriptionKey))
                .equals((ForwardedSubscriptions::Table, ForwardedSubscriptions::LocalSubscriptionKey)),
        )
        .and_where(
            Expr::col((ForwardedSubscriptionClaims::Table, ForwardedSubscriptionClaims::RegistrationId))
                .equals((ForwardedSubscriptions::Table, ForwardedSubscriptions::RegistrationId)),
        )
        .to_owned();
    let settled = Query::select()
        .column((ForwardedSubscriptions::Table, ForwardedSubscriptions::LocalSubscriptionKey))
        .expr(now())
        .column((ForwardedSubscriptions::Table, ForwardedSubscriptions::RegistrationId))
        .from(ForwardedSubscriptions::Table)
        .and_where(Expr::exists(claim).not())
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(ForwardedSubscriptionRedirects::Table)
        .columns([
            ForwardedSubscriptionRedirects::LocalSubscriptionKey,
            ForwardedSubscriptionRedirects::RedirectDueAt,
            ForwardedSubscriptionRedirects::RegistrationId,
        ])
        .select_from(settled)
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
        .on_conflict(
            OnConflict::columns([
                ForwardedSubscriptionRedirects::LocalSubscriptionKey,
                ForwardedSubscriptionRedirects::RegistrationId,
            ])
            .do_nothing()
            .to_owned(),
        );
    Ok(insert)
}

/// Owe the current receiver from now to the copy of `local` at
/// `registration`, unless it owes it already.
///
/// A `local` no subscription carries resolves to NULL, which the key column
/// refuses.
pub(crate) fn owe_copy_statement(local: &Identifier, registration: &Identifier) -> Result<InsertStatement, SubscriptionForwardingError> {
    let copy = Query::select()
        .column(ForwardedSubscriptions::LocalSubscriptionKey)
        .expr(now())
        .column(ForwardedSubscriptions::RegistrationId)
        .from(ForwardedSubscriptions::Table)
        .and_where(Expr::col(ForwardedSubscriptions::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .and_where(Expr::col(ForwardedSubscriptions::RegistrationId).eq(registration.as_str()))
        .to_owned();
    let mut insert = Query::insert();
    insert
        .into_table(ForwardedSubscriptionRedirects::Table)
        .columns([
            ForwardedSubscriptionRedirects::LocalSubscriptionKey,
            ForwardedSubscriptionRedirects::RedirectDueAt,
            ForwardedSubscriptionRedirects::RegistrationId,
        ])
        .select_from(copy)
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
        .on_conflict(
            OnConflict::columns([
                ForwardedSubscriptionRedirects::LocalSubscriptionKey,
                ForwardedSubscriptionRedirects::RegistrationId,
            ])
            .do_nothing()
            .to_owned(),
        );
    Ok(insert)
}

/// The URI of up to `limit` local subscriptions whose companion's update is
/// due, the longest overdue first, served by
/// `idx_subscription_companion_redirects_redirect_due_at`.
pub(crate) fn overdue_companions_statement(limit: u32) -> SelectStatement {
    let owing = Alias::new(OWING);
    let subscription = Alias::new(SUBSCRIPTION);
    Query::select()
        .column((subscription.clone(), Subscriptions::Id))
        .from_as(SubscriptionCompanionRedirects::Table, owing.clone())
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((owing.clone(), SubscriptionCompanionRedirects::LocalSubscriptionKey)),
        )
        .and_where(Expr::col((owing.clone(), SubscriptionCompanionRedirects::RedirectDueAt)).lte(Expr::current_timestamp()))
        .order_by((owing, SubscriptionCompanionRedirects::RedirectDueAt), Order::Asc)
        .limit(u64::from(limit))
        .to_owned()
}

/// The local subscription URI, registration and remote identifier of up to
/// `limit` copies whose update is due, the longest overdue first, served by
/// `idx_forwarded_subscription_redirects_redirect_due_at`.
pub(crate) fn overdue_copies_statement(limit: u32) -> SelectStatement {
    let owing = Alias::new(OWING);
    let copy = Alias::new(COPY);
    let subscription = Alias::new(SUBSCRIPTION);
    Query::select()
        .column((subscription.clone(), Subscriptions::Id))
        .column((owing.clone(), ForwardedSubscriptionRedirects::RegistrationId))
        .column((copy.clone(), ForwardedSubscriptions::RemoteSubscriptionId))
        .from_as(ForwardedSubscriptionRedirects::Table, owing.clone())
        .join_as(
            JoinType::InnerJoin,
            ForwardedSubscriptions::Table,
            copy.clone(),
            Expr::col((copy.clone(), ForwardedSubscriptions::LocalSubscriptionKey))
                .equals((owing.clone(), ForwardedSubscriptionRedirects::LocalSubscriptionKey))
                .and(Expr::col((copy, ForwardedSubscriptions::RegistrationId)).equals((owing.clone(), ForwardedSubscriptionRedirects::RegistrationId))),
        )
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((owing.clone(), ForwardedSubscriptionRedirects::LocalSubscriptionKey)),
        )
        .and_where(Expr::col((owing.clone(), ForwardedSubscriptionRedirects::RedirectDueAt)).lte(Expr::current_timestamp()))
        .order_by((owing, ForwardedSubscriptionRedirects::RedirectDueAt), Order::Asc)
        .limit(u64::from(limit))
        .to_owned()
}

/// Clear what the companion of `local` owes.
pub(crate) fn companion_redirected_statement(local: &Identifier) -> DeleteStatement {
    Query::delete()
        .from_table(SubscriptionCompanionRedirects::Table)
        .and_where(Expr::col(SubscriptionCompanionRedirects::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .to_owned()
}

/// Clear what the copy of `local` at `registration` owes.
pub(crate) fn copy_redirected_statement(local: &Identifier, registration: &Identifier) -> DeleteStatement {
    Query::delete()
        .from_table(ForwardedSubscriptionRedirects::Table)
        .and_where(Expr::col(ForwardedSubscriptionRedirects::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .and_where(Expr::col(ForwardedSubscriptionRedirects::RegistrationId).eq(registration.as_str()))
        .to_owned()
}

/// Make the update the companion of `local` owes due `after` from now.
pub(crate) fn postpone_companion_statement(local: &Identifier, after: Duration) -> UpdateStatement {
    Query::update()
        .table(SubscriptionCompanionRedirects::Table)
        .value(SubscriptionCompanionRedirects::RedirectDueAt, span_after_transaction_start(after))
        .and_where(Expr::col(SubscriptionCompanionRedirects::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .to_owned()
}

/// Make the update the copy of `local` at `registration` owes due `after`
/// from now.
pub(crate) fn postpone_copy_statement(local: &Identifier, registration: &Identifier, after: Duration) -> UpdateStatement {
    Query::update()
        .table(ForwardedSubscriptionRedirects::Table)
        .value(ForwardedSubscriptionRedirects::RedirectDueAt, span_after_transaction_start(after))
        .and_where(Expr::col(ForwardedSubscriptionRedirects::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .and_where(Expr::col(ForwardedSubscriptionRedirects::RegistrationId).eq(registration.as_str()))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::subscription_forwarding::redirect_statements::{
        adopted_receiver_statement,
        companion_redirected_statement,
        copy_redirected_statement,
        overdue_companions_statement,
        overdue_copies_statement,
        owe_companions_statement,
        owe_copy_statement,
        owe_settled_copies_statement,
        postpone_companion_statement,
        postpone_copy_statement,
        record_receiver_statement,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_subscription_forwarding::receiver::ReceiverEndpoint;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;
    use std::time::Duration;
    use url::Url;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a URI")
    }

    /// The key lookup every statement embeds for the local subscription.
    const LOCAL_KEY: &str = r#"(SELECT "subscription_key" FROM "subscriptions" WHERE "id" = 'urn:ngsi-ld:Subscription:local')"#;

    #[test]
    fn the_adopted_receiver_is_read_locked_for_the_adoption() {
        assert_eq!(
            adopted_receiver_statement().to_string(PostgresQueryBuilder),
            r#"SELECT "receiver" FROM "subscription_receiver" FOR UPDATE"#
        );
    }

    #[test]
    fn the_receiver_adopted_replaces_the_one_row() {
        let receiver = ReceiverEndpoint::from_advertised(&Url::parse("http://broker-a:1056").expect("a URL")).expect("a receiver");
        assert_eq!(
            record_receiver_statement(&receiver)
                .expect("the statement builds")
                .to_string(PostgresQueryBuilder),
            concat!(
                r#"INSERT INTO "subscription_receiver" ("receiver") VALUES ('http://broker-a:1056/ngsi-ld/andromeda/v1/notifications') "#,
                r#"ON CONFLICT ("singleton") DO UPDATE SET "receiver" = "excluded"."receiver""#
            )
        );
    }

    #[test]
    fn every_companion_owing_nothing_owes_the_receiver_from_now() {
        assert_eq!(
            owe_companions_statement().expect("the statement builds").to_string(PostgresQueryBuilder),
            concat!(
                r#"INSERT INTO "subscription_companion_redirects" ("local_subscription_key", "redirect_due_at") "#,
                r#"SELECT "local_subscription_key", NOW() FROM "subscription_companions" "#,
                r#"ON CONFLICT ("local_subscription_key") DO NOTHING"#
            )
        );
    }

    /// A copy whose claim is open is left out, since its placement sends the
    /// current receiver and its settlement tells whether the source holds that
    /// send.
    #[test]
    fn every_settled_copy_owing_nothing_owes_the_receiver_from_now() {
        assert_eq!(
            owe_settled_copies_statement().expect("the statement builds").to_string(PostgresQueryBuilder),
            concat!(
                r#"INSERT INTO "forwarded_subscription_redirects" ("local_subscription_key", "redirect_due_at", "registration_id") "#,
                r#"SELECT "forwarded_subscriptions"."local_subscription_key", NOW(), "forwarded_subscriptions"."registration_id" FROM "forwarded_subscriptions" "#,
                r#"WHERE NOT EXISTS(SELECT 1 FROM "forwarded_subscription_claims" "#,
                r#"WHERE "forwarded_subscription_claims"."local_subscription_key" = "forwarded_subscriptions"."local_subscription_key" "#,
                r#"AND "forwarded_subscription_claims"."registration_id" = "forwarded_subscriptions"."registration_id") "#,
                r#"ON CONFLICT ("local_subscription_key", "registration_id") DO NOTHING"#
            )
        );
    }

    #[test]
    fn one_copy_owes_the_receiver_from_now_by_the_key_of_its_local_subscription_and_its_registration() {
        let sql = owe_copy_statement(&uri("urn:ngsi-ld:Subscription:local"), &uri("urn:ngsi-ld:ContextSourceRegistration:b"))
            .expect("the statement builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                concat!(
                    r#"INSERT INTO "forwarded_subscription_redirects" ("local_subscription_key", "redirect_due_at", "registration_id") "#,
                    r#"SELECT "local_subscription_key", NOW(), "registration_id" FROM "forwarded_subscriptions" "#,
                    r#"WHERE "local_subscription_key" IN {} AND "registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b' "#,
                    r#"ON CONFLICT ("local_subscription_key", "registration_id") DO NOTHING"#
                ),
                LOCAL_KEY
            )
        );
    }

    #[test]
    fn the_overdue_companions_are_read_with_their_subscription_uri_longest_overdue_first() {
        assert_eq!(
            overdue_companions_statement(100).to_string(PostgresQueryBuilder),
            concat!(
                r#"SELECT "s"."id" FROM "subscription_companion_redirects" AS "o" "#,
                r#"INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "o"."local_subscription_key" "#,
                r#"WHERE "o"."redirect_due_at" <= CURRENT_TIMESTAMP ORDER BY "o"."redirect_due_at" ASC LIMIT 100"#
            )
        );
    }

    #[test]
    fn the_overdue_copies_are_read_with_their_subscription_uri_and_identifier_longest_overdue_first() {
        assert_eq!(
            overdue_copies_statement(100).to_string(PostgresQueryBuilder),
            concat!(
                r#"SELECT "s"."id", "o"."registration_id", "f"."remote_subscription_id" FROM "forwarded_subscription_redirects" AS "o" "#,
                r#"INNER JOIN "forwarded_subscriptions" AS "f" ON "f"."local_subscription_key" = "o"."local_subscription_key" AND "f"."registration_id" = "o"."registration_id" "#,
                r#"INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "o"."local_subscription_key" "#,
                r#"WHERE "o"."redirect_due_at" <= CURRENT_TIMESTAMP ORDER BY "o"."redirect_due_at" ASC LIMIT 100"#
            )
        );
    }

    #[test]
    fn a_redirected_companion_loses_its_row_by_the_key_of_its_local_subscription() {
        assert_eq!(
            companion_redirected_statement(&uri("urn:ngsi-ld:Subscription:local")).to_string(PostgresQueryBuilder),
            format!(r#"DELETE FROM "subscription_companion_redirects" WHERE "local_subscription_key" IN {LOCAL_KEY}"#)
        );
    }

    #[test]
    fn a_redirected_copy_loses_its_row_by_the_key_of_its_local_subscription_and_its_registration() {
        assert_eq!(
            copy_redirected_statement(&uri("urn:ngsi-ld:Subscription:local"), &uri("urn:ngsi-ld:ContextSourceRegistration:b")).to_string(PostgresQueryBuilder),
            format!(
                r#"DELETE FROM "forwarded_subscription_redirects" WHERE "local_subscription_key" IN {LOCAL_KEY} AND "registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b'"#
            )
        );
    }

    #[test]
    fn a_postponed_update_falls_due_the_span_after_the_transaction_began() {
        assert_eq!(
            postpone_companion_statement(&uri("urn:ngsi-ld:Subscription:local"), Duration::from_secs(60)).to_string(PostgresQueryBuilder),
            format!(
                r#"UPDATE "subscription_companion_redirects" SET "redirect_due_at" = CURRENT_TIMESTAMP + (make_interval(secs => 60)) WHERE "local_subscription_key" IN {LOCAL_KEY}"#
            )
        );
        assert_eq!(
            postpone_copy_statement(
                &uri("urn:ngsi-ld:Subscription:local"),
                &uri("urn:ngsi-ld:ContextSourceRegistration:b"),
                Duration::from_secs(60)
            )
            .to_string(PostgresQueryBuilder),
            format!(
                r#"UPDATE "forwarded_subscription_redirects" SET "redirect_due_at" = CURRENT_TIMESTAMP + (make_interval(secs => 60)) WHERE "local_subscription_key" IN {LOCAL_KEY} AND "registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b'"#
            )
        );
    }
}
