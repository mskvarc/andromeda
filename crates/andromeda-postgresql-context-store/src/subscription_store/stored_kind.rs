//! Which subscription family a statement addresses.

use andromeda_postgresql_codes::subscription_kind::SubscriptionKindCode;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// Which subscription family a repository serves; both share the `subscriptions`
/// table and are told apart by `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StoredKind {
    /// An entity subscription (clause 5.8), which narrows by `datasetId`.
    Entity,
    /// A context-source subscription (clause 5.11), which does not.
    Csource,
}

impl StoredKind {
    /// The registry member a row of this family carries in `kind`.
    pub(crate) const fn code(self) -> SubscriptionKindCode {
        match self {
            StoredKind::Entity => SubscriptionKindCode::Entity,
            StoredKind::Csource => SubscriptionKindCode::Csource,
        }
    }

    /// Whether this family stores a `datasetId` column.
    pub(crate) const fn has_dataset_id(self) -> bool {
        matches!(self, StoredKind::Entity)
    }
}

/// The predicate confining a statement to one subscription family.
pub(crate) fn of_kind(kind: StoredKind) -> SimpleExpr {
    Expr::col((Subscriptions::Table, Subscriptions::Kind)).eq(kind.code().code())
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::stored_kind::{StoredKind, of_kind};
    use andromeda_postgresql_codes::subscription_kind::SubscriptionKindCode;
    use pretty_assertions::assert_eq;
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn the_kind_predicate_names_the_subscriptions_column_and_compares_the_code() {
        let sql = Query::select().expr(of_kind(StoredKind::Csource)).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "subscriptions"."kind" = 2"#);
    }

    #[test]
    fn each_family_is_stored_under_its_own_registry_member() {
        assert_eq!(StoredKind::Entity.code(), SubscriptionKindCode::Entity);
        assert_eq!(StoredKind::Csource.code(), SubscriptionKindCode::Csource);
    }
}
