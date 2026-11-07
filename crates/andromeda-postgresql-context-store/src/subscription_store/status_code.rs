//! The number the `subscriptions.status` column stores a lifecycle state as,
//! and the predicates that select by it.
//!
//! Written as exhaustive matches in both directions, so a state a later
//! revision of clause 5.2.12 adds fails to compile here, where its persisted
//! number is decided.

use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_postgresql_codes::subscription_status::SubscriptionStatusCode;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// The registry member the store persists for a lifecycle state.
#[must_use]
pub(crate) const fn stored_status(status: SubscriptionStatus) -> SubscriptionStatusCode {
    match status {
        SubscriptionStatus::Active => SubscriptionStatusCode::Active,
        SubscriptionStatus::Paused => SubscriptionStatusCode::Paused,
        SubscriptionStatus::Expired => SubscriptionStatusCode::Expired,
    }
}

/// The lifecycle state a stored registry member names.
#[must_use]
pub(crate) const fn status_of(code: SubscriptionStatusCode) -> SubscriptionStatus {
    match code {
        SubscriptionStatusCode::Active => SubscriptionStatus::Active,
        SubscriptionStatusCode::Paused => SubscriptionStatus::Paused,
        SubscriptionStatusCode::Expired => SubscriptionStatus::Expired,
    }
}

/// `subscriptions.status = <status>`.
pub(crate) fn status_is(status: SubscriptionStatus) -> SimpleExpr {
    Expr::col((Subscriptions::Table, Subscriptions::Status)).eq(stored_status(status).code())
}

/// `subscriptions.status <> <status>`.
pub(crate) fn status_is_not(status: SubscriptionStatus) -> SimpleExpr {
    Expr::col((Subscriptions::Table, Subscriptions::Status)).ne(stored_status(status).code())
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::status_code::{status_is, status_is_not, status_of, stored_status};
    use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
    use andromeda_postgresql_codes::subscription_status::SubscriptionStatusCode;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};
    use strum::IntoEnumIterator;

    #[test]
    fn every_subscription_status_survives_a_round_trip_through_its_stored_number() {
        for status in SubscriptionStatus::iter() {
            assert_eq!(status_of(stored_status(status)), status);
        }
    }

    /// The registry's label is the clause 5.2.12 token the domain enum renders.
    #[test]
    fn the_stored_number_of_a_status_is_the_one_its_token_is_registered_under() {
        for code in SubscriptionStatusCode::ALL {
            assert_eq!(code.label(), status_of(*code).to_string());
        }
    }

    #[test]
    fn the_status_predicates_compare_the_stored_code() {
        let sql = Query::select().expr(status_is(SubscriptionStatus::Active)).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "subscriptions"."status" = 1"#);
        let sql = Query::select().expr(status_is_not(SubscriptionStatus::Expired)).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "subscriptions"."status" <> 3"#);
    }
}
