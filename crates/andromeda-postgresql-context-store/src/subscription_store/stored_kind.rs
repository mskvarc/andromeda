//! Which subscription family a statement addresses.

use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::literal::text_literal;
use sea_query::{Expr, ExprTrait, SimpleExpr};

/// Which subscription family a repository serves; both share the `subscriptions`
/// table and are told apart by `kind`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoredKind {
    /// An entity subscription (clause 5.8), which narrows by `datasetId`.
    Entity,
    /// A context-source subscription (clause 5.11), which does not.
    Csource,
}

impl StoredKind {
    /// The `kind` token a row of this family carries.
    pub const fn wire(self) -> &'static str {
        match self {
            StoredKind::Entity => "entity",
            StoredKind::Csource => "csource",
        }
    }

    /// Whether this family stores a `datasetId` column.
    pub const fn has_dataset_id(self) -> bool {
        matches!(self, StoredKind::Entity)
    }
}

/// The predicate confining a statement to one subscription family, inline like
/// the fixed token it is rather than bound.
pub fn of_kind(kind: StoredKind) -> SimpleExpr {
    Expr::col((Subscriptions::Table, Subscriptions::Kind)).eq(text_literal(kind.wire()))
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::stored_kind::{StoredKind, of_kind};
    use sea_query::{PostgresQueryBuilder, Query};

    #[test]
    fn the_kind_predicate_names_the_subscriptions_column_and_inlines_the_token() {
        let sql = Query::select().expr(of_kind(StoredKind::Csource)).to_string(PostgresQueryBuilder);
        assert_eq!(sql, r#"SELECT "subscriptions"."kind" = ('csource')"#);
    }
}
