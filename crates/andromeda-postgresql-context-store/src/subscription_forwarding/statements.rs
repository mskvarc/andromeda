//! The statements over `subscription_companions` and `forwarded_subscriptions`.
//!
//! Both tables hold a subscription by its `subscriptions` surrogate key while
//! the broker names it by its URI, so each statement resolves the key itself.
//! A write and a filter look it up by URI in a subquery, and a read that
//! returns a subscription joins `subscriptions` for its URI. No caller spends
//! a round trip on the lookup.

use crate::{store_fault::persistence_error, subscription_store::surrogate_key::subscription_key_of};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::{
    subscription_forwarding::{ForwardedSubscriptions, SubscriptionCompanions},
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::scalar_subquery::scalar_subquery;
use andromeda_subscription::forwarding_hook::ForwardedCopy;
use andromeda_subscription_forwarding::error::SubscriptionForwardingError;
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, Iden, InsertStatement, JoinType, OnConflict, Query, SelectStatement};

/// The alias a read takes the mapping table under.
const MAPPING: &str = "m";

/// The alias a read joins `subscriptions` under for a subscription's URI.
const SUBSCRIPTION: &str = "s";

/// A read of `table` under `m` whose first column is the URI of the
/// subscription `key` holds, joined from `subscriptions` under `s`. The caller
/// adds its own columns and filter.
fn reading_subscription_uri<T: Iden + Copy + 'static>(table: T, key: T) -> SelectStatement {
    let subscription = Alias::new(SUBSCRIPTION);
    let mapping = Alias::new(MAPPING);
    Query::select()
        .column((subscription.clone(), Subscriptions::Id))
        .from_as(table, mapping.clone())
        .join_as(
            JoinType::InnerJoin,
            Subscriptions::Table,
            subscription.clone(),
            Expr::col((subscription, Subscriptions::SubscriptionKey)).equals((mapping, key)),
        )
        .to_owned()
}

/// Record `companion` as the companion of `local`, replacing the one `local`
/// had.
///
/// A URI no subscription carries resolves to NULL, which the key columns
/// refuse, so a link naming a missing subscription fails.
pub(crate) fn link_companion_statement(local: &Identifier, companion: &Identifier) -> Result<InsertStatement, SubscriptionForwardingError> {
    let mut insert = Query::insert();
    insert
        .into_table(SubscriptionCompanions::Table)
        .columns([SubscriptionCompanions::LocalSubscriptionKey, SubscriptionCompanions::CompanionSubscriptionKey])
        .values([scalar_subquery(subscription_key_of(local)), scalar_subquery(subscription_key_of(companion))])
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
        .on_conflict(
            OnConflict::column(SubscriptionCompanions::LocalSubscriptionKey)
                .update_column(SubscriptionCompanions::CompanionSubscriptionKey)
                .to_owned(),
        );
    Ok(insert)
}

/// The URI of the companion of `local`.
pub(crate) fn companion_of_statement(local: &Identifier) -> SelectStatement {
    reading_subscription_uri(SubscriptionCompanions::Table, SubscriptionCompanions::CompanionSubscriptionKey)
        .and_where(Expr::col((Alias::new(MAPPING), SubscriptionCompanions::LocalSubscriptionKey)).in_subquery(subscription_key_of(local)))
        .to_owned()
}

/// The URI of the local subscription whose companion is `companion`.
pub(crate) fn local_of_companion_statement(companion: &Identifier) -> SelectStatement {
    reading_subscription_uri(SubscriptionCompanions::Table, SubscriptionCompanions::LocalSubscriptionKey)
        .and_where(Expr::col((Alias::new(MAPPING), SubscriptionCompanions::CompanionSubscriptionKey)).in_subquery(subscription_key_of(companion)))
        .to_owned()
}

/// Record the copy of `local` placed at `copy.registration`, replacing the
/// identifier an earlier copy there carried.
///
/// A `local` no subscription carries resolves to NULL, which the key column
/// refuses, so recording a copy of a missing subscription fails.
pub(crate) fn record_forwarded_statement(local: &Identifier, copy: &ForwardedCopy) -> Result<InsertStatement, SubscriptionForwardingError> {
    let mut insert = Query::insert();
    insert
        .into_table(ForwardedSubscriptions::Table)
        .columns([
            ForwardedSubscriptions::LocalSubscriptionKey,
            ForwardedSubscriptions::RegistrationId,
            ForwardedSubscriptions::RemoteSubscriptionId,
        ])
        .values([
            scalar_subquery(subscription_key_of(local)),
            Expr::val(copy.registration.as_str()),
            Expr::val(copy.remote_subscription.as_str()),
        ])
        .map_err(|error| persistence_error::<SubscriptionForwardingError>(error))?
        .on_conflict(
            OnConflict::columns([ForwardedSubscriptions::LocalSubscriptionKey, ForwardedSubscriptions::RegistrationId])
                .update_column(ForwardedSubscriptions::RemoteSubscriptionId)
                .to_owned(),
        );
    Ok(insert)
}

/// The registration and remote identifier of every copy of `local`.
pub(crate) fn forwarded_of_statement(local: &Identifier) -> SelectStatement {
    Query::select()
        .column(ForwardedSubscriptions::RegistrationId)
        .column(ForwardedSubscriptions::RemoteSubscriptionId)
        .from(ForwardedSubscriptions::Table)
        .and_where(Expr::col(ForwardedSubscriptions::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .to_owned()
}

/// The local subscription URI and remote identifier of every copy held at
/// `registration`'s context source, served by
/// `idx_forwarded_subscriptions_registration`.
pub(crate) fn forwarded_for_registration_statement(registration: &Identifier) -> SelectStatement {
    reading_subscription_uri(ForwardedSubscriptions::Table, ForwardedSubscriptions::LocalSubscriptionKey)
        .column((Alias::new(MAPPING), ForwardedSubscriptions::RemoteSubscriptionId))
        .and_where(Expr::col((Alias::new(MAPPING), ForwardedSubscriptions::RegistrationId)).eq(registration.as_str()))
        .to_owned()
}

/// The local subscription URI and sending registration of the copy whose
/// remote identifier is `remote_subscription`.
pub(crate) fn local_of_forwarded_statement(remote_subscription: &Identifier) -> SelectStatement {
    reading_subscription_uri(ForwardedSubscriptions::Table, ForwardedSubscriptions::LocalSubscriptionKey)
        .column((Alias::new(MAPPING), ForwardedSubscriptions::RegistrationId))
        .and_where(Expr::col((Alias::new(MAPPING), ForwardedSubscriptions::RemoteSubscriptionId)).eq(remote_subscription.as_str()))
        .to_owned()
}

/// Forget the copy of `local` at `registration`.
pub(crate) fn remove_forwarded_statement(local: &Identifier, registration: &Identifier) -> DeleteStatement {
    Query::delete()
        .from_table(ForwardedSubscriptions::Table)
        .and_where(Expr::col(ForwardedSubscriptions::LocalSubscriptionKey).in_subquery(subscription_key_of(local)))
        .and_where(Expr::col(ForwardedSubscriptions::RegistrationId).eq(registration.as_str()))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::subscription_forwarding::statements::{
        companion_of_statement,
        forwarded_for_registration_statement,
        forwarded_of_statement,
        link_companion_statement,
        local_of_companion_statement,
        local_of_forwarded_statement,
        record_forwarded_statement,
        remove_forwarded_statement,
    };
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_subscription::forwarding_hook::ForwardedCopy;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn uri(raw: &str) -> Identifier {
        raw.parse().expect("a URI")
    }

    /// The key lookup every statement embeds for the subscription `name`.
    fn key_of(name: &str) -> String {
        format!(r#"(SELECT "subscription_key" FROM "subscriptions" WHERE "id" = 'urn:ngsi-ld:Subscription:{name}')"#)
    }

    #[test]
    fn a_companion_link_resolves_both_subscriptions_to_their_keys() {
        let sql = link_companion_statement(&uri("urn:ngsi-ld:Subscription:local"), &uri("urn:ngsi-ld:Subscription:companion"))
            .expect("the link builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"INSERT INTO "subscription_companions" ("local_subscription_key", "companion_subscription_key") VALUES ({}, {}) ON CONFLICT ("local_subscription_key") DO UPDATE SET "companion_subscription_key" = "excluded"."companion_subscription_key""#,
                key_of("local"),
                key_of("companion")
            )
        );
    }

    #[test]
    fn the_companion_of_a_local_subscription_is_read_back_as_its_uri() {
        let sql = companion_of_statement(&uri("urn:ngsi-ld:Subscription:local")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"SELECT "s"."id" FROM "subscription_companions" AS "m" INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "m"."companion_subscription_key" WHERE "m"."local_subscription_key" IN {}"#,
                key_of("local")
            )
        );
    }

    #[test]
    fn the_local_subscription_of_a_companion_is_read_back_as_its_uri() {
        let sql = local_of_companion_statement(&uri("urn:ngsi-ld:Subscription:companion")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"SELECT "s"."id" FROM "subscription_companions" AS "m" INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "m"."local_subscription_key" WHERE "m"."companion_subscription_key" IN {}"#,
                key_of("companion")
            )
        );
    }

    #[test]
    fn a_forwarded_copy_is_recorded_under_the_key_of_its_local_subscription() {
        let copy = ForwardedCopy {
            registration: uri("urn:ngsi-ld:ContextSourceRegistration:b"),
            remote_subscription: uri("urn:ngsi-ld:Subscription:remote"),
        };
        let sql = record_forwarded_statement(&uri("urn:ngsi-ld:Subscription:local"), &copy)
            .expect("the record builds")
            .to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"INSERT INTO "forwarded_subscriptions" ("local_subscription_key", "registration_id", "remote_subscription_id") VALUES ({}, 'urn:ngsi-ld:ContextSourceRegistration:b', 'urn:ngsi-ld:Subscription:remote') ON CONFLICT ("local_subscription_key", "registration_id") DO UPDATE SET "remote_subscription_id" = "excluded"."remote_subscription_id""#,
                key_of("local")
            )
        );
    }

    #[test]
    fn the_copies_of_a_local_subscription_are_read_by_its_key() {
        let sql = forwarded_of_statement(&uri("urn:ngsi-ld:Subscription:local")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"SELECT "registration_id", "remote_subscription_id" FROM "forwarded_subscriptions" WHERE "local_subscription_key" IN {}"#,
                key_of("local")
            )
        );
    }

    #[test]
    fn the_copies_held_at_a_registration_are_read_by_its_identifier_with_their_local_subscription_uri() {
        let sql = forwarded_for_registration_statement(&uri("urn:ngsi-ld:ContextSourceRegistration:b")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "s"."id", "m"."remote_subscription_id" FROM "forwarded_subscriptions" AS "m" INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "m"."local_subscription_key" WHERE "m"."registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b'"#
        );
    }

    #[test]
    fn an_inbound_notification_is_routed_by_the_remote_identifier_of_its_copy() {
        let sql = local_of_forwarded_statement(&uri("urn:ngsi-ld:Subscription:remote")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            r#"SELECT "s"."id", "m"."registration_id" FROM "forwarded_subscriptions" AS "m" INNER JOIN "subscriptions" AS "s" ON "s"."subscription_key" = "m"."local_subscription_key" WHERE "m"."remote_subscription_id" = 'urn:ngsi-ld:Subscription:remote'"#
        );
    }

    #[test]
    fn a_withdrawn_copy_is_deleted_by_the_key_of_its_local_subscription_and_its_registration() {
        let sql =
            remove_forwarded_statement(&uri("urn:ngsi-ld:Subscription:local"), &uri("urn:ngsi-ld:ContextSourceRegistration:b")).to_string(PostgresQueryBuilder);
        assert_eq!(
            sql,
            format!(
                r#"DELETE FROM "forwarded_subscriptions" WHERE "local_subscription_key" IN {} AND "registration_id" = 'urn:ngsi-ld:ContextSourceRegistration:b'"#,
                key_of("local")
            )
        );
    }
}
