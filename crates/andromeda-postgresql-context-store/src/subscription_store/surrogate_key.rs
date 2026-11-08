//! The `subscriptions` surrogate key of a subscription the broker names by its
//! URI.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use sea_query::{Expr, ExprTrait, Query, SelectStatement};

/// The surrogate key of `subscription` (`SELECT subscription_key FROM
/// subscriptions WHERE id = $1`), as a subquery a relation keyed by it filters
/// or writes with. It returns no row for a subscription that does not exist.
#[must_use]
pub(crate) fn subscription_key_of(subscription: &Identifier) -> SelectStatement {
    Query::select()
        .column(Subscriptions::SubscriptionKey)
        .from(Subscriptions::Table)
        .and_where(Expr::col(Subscriptions::Id).eq(subscription.as_str()))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::surrogate_key::subscription_key_of;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn the_surrogate_key_is_looked_up_by_the_subscription_uri() {
        let subscription: Identifier = "urn:ngsi-ld:Subscription:s".parse().expect("a URI");
        assert_eq!(
            subscription_key_of(&subscription).to_string(PostgresQueryBuilder),
            r#"SELECT "subscription_key" FROM "subscriptions" WHERE "id" = 'urn:ngsi-ld:Subscription:s'"#
        );
    }
}
