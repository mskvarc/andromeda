//! The read-side projection every subscription query shares: the column set the
//! mapper reads, the family pin, the total count, and the keyset predicate and
//! ordering of a cursor walk (clause 5.5.9.1).

use crate::subscription_store::{
    delivery_state::join_delivery_state,
    failure::persistence_error,
    stored_kind::{StoredKind, of_kind},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_query::cursor::PageCursor;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{aggregate::count_all, cast::cast, pg_type::PostgresType};
use andromeda_subscription::error::SubscriptionError;
use sea_query::{Expr, ExprTrait, Order, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;

/// The `status::text AS status` read cast: a query decodes the enum as text.
fn status_as_text() -> SimpleExpr {
    cast(Expr::col((Subscriptions::Table, Subscriptions::Status)), PostgresType::Text)
}

/// Add the stored-subscription projection to `select`, in the column set the
/// mapper reads by name; `dataset_id` only for the family that stores it. The
/// delivery counters come from the joined state row.
fn project_subscription(select: &mut SelectStatement, kind: StoredKind, target: &ContextTarget) {
    select
        .column((Subscriptions::Table, Subscriptions::Id))
        .column((Subscriptions::Table, Subscriptions::SubscriptionKey))
        .column((Subscriptions::Table, Subscriptions::SubscriptionName))
        .column((Subscriptions::Table, Subscriptions::Description))
        .column((Subscriptions::Table, Subscriptions::Entities))
        .column((Subscriptions::Table, Subscriptions::WatchedAttributes))
        .column((Subscriptions::Table, Subscriptions::NotificationTrigger))
        .column((Subscriptions::Table, Subscriptions::TimeInterval))
        .column((Subscriptions::Table, Subscriptions::Q))
        .column((Subscriptions::Table, Subscriptions::GeoQ))
        .column((Subscriptions::Table, Subscriptions::ScopeQ))
        .column((Subscriptions::Table, Subscriptions::Csf))
        .column((Subscriptions::Table, Subscriptions::ExpandValues))
        .column((Subscriptions::Table, Subscriptions::JsonKeys))
        .column((Subscriptions::Table, Subscriptions::Notification))
        .column((Subscriptions::Table, Subscriptions::ExpiresAt))
        .column((Subscriptions::Table, Subscriptions::Throttling))
        .expr_as(status_as_text(), Subscriptions::Status)
        .column((Subscriptions::Table, Subscriptions::TemporalQ))
        .column((Subscriptions::Table, Subscriptions::Lang))
        .column((Subscriptions::Table, Subscriptions::JsonldContext))
        .column((Subscriptions::Table, Subscriptions::LocalOnly))
        .column((Subscriptions::Table, Subscriptions::SplitEntities))
        .column((Subscriptions::Table, Subscriptions::NgsildConformance))
        .column((Subscriptions::Table, Subscriptions::CreatedAt))
        .column((Subscriptions::Table, Subscriptions::ModifiedAt));
    if kind.has_dataset_id() {
        select.column((Subscriptions::Table, Subscriptions::DatasetId));
    }
    join_delivery_state(select, target);
}

/// A projecting select of one family with its state row joined, ready for a
/// caller's `WHERE`, in the target's branch schema.
pub(super) fn projected_select(kind: StoredKind, target: &ContextTarget) -> SelectStatement {
    let mut select = Query::select();
    select.from(table_of(target, Subscriptions::Table));
    project_subscription(&mut select, kind, target);
    select
}

/// The `WHERE id = $id AND kind` pin of one subscription. The branch is chosen
/// by the schema its relations are qualified with, not by a predicate.
pub(super) fn one_of(id: &Uri, kind: StoredKind) -> sea_query::Cond {
    sea_query::Cond::all()
        .add(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(id.as_str()))
        .add(of_kind(kind))
}

/// Count the subscriptions of one family, the total both the offset and the
/// keyset listing report (clause 6.3.13).
pub(super) async fn count_of_kind(pool: &sqlx::PgPool, target: &ContextTarget, kind: StoredKind) -> Result<i64, SubscriptionError> {
    let count = Query::select()
        .expr(count_all())
        .from(table_of(target, Subscriptions::Table))
        .and_where(of_kind(kind))
        .to_owned();
    let (sql, values) = count.build_sqlx(PostgresQueryBuilder);
    let row: (i64,) = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .fetch_one(pool)
        .await
        .map_err(persistence_error)?;
    Ok(row.0)
}

/// Add the keyset predicate and ordering for one cursor direction over the
/// subscription identifier (clause 5.5.9.1).
///
/// The keyset is over the identifier rather than the `createdAt` of the offset
/// listing, because a single-key keyset needs a unique key. The predicate and
/// the ordering read the same `id` column, so both stay driven by its index in
/// either direction; a backward or from-end page is ordered descending and
/// reversed to ascending by the caller.
pub(super) fn apply_subscription_keyset(select: &mut SelectStatement, cursor: Option<&PageCursor>) {
    match cursor {
        None => {
            select.order_by((Subscriptions::Table, Subscriptions::Id), Order::Asc);
        }
        Some(PageCursor::After(boundary)) => {
            select.and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).gt(Expr::val(boundary.as_str())));
            select.order_by((Subscriptions::Table, Subscriptions::Id), Order::Asc);
        }
        Some(PageCursor::Before(boundary)) => {
            select.and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).lt(Expr::val(boundary.as_str())));
            select.order_by((Subscriptions::Table, Subscriptions::Id), Order::Desc);
        }
        Some(PageCursor::End) => {
            select.order_by((Subscriptions::Table, Subscriptions::Id), Order::Desc);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::{
        projection::{apply_subscription_keyset, projected_select},
        stored_kind::StoredKind,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_query::cursor::PageCursor;
    use sea_query::PostgresQueryBuilder;

    /// The keyset SQL for one cursor direction over the subscription identifier
    /// (clause 5.5.9.1).
    fn keyset_sql(cursor: Option<&PageCursor>) -> String {
        let mut select = projected_select(StoredKind::Entity, &ContextTarget::Live);
        apply_subscription_keyset(&mut select, cursor);
        select.to_string(PostgresQueryBuilder)
    }

    fn uri(value: &str) -> Uri {
        value.parse().expect("valid URI")
    }

    /// The first page of a cursor walk sorts by identifier ascending with no
    /// keyset bound and no offset (clause 5.5.9.1).
    #[test]
    fn a_first_page_orders_by_id_ascending_without_a_bound() {
        let sql = keyset_sql(None);
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" ASC"#), "{sql}");
        assert!(!sql.contains(r#""id" >"#) && !sql.contains(r#""id" <"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// A forward cursor keeps the identifiers above the boundary, ascending.
    #[test]
    fn a_forward_cursor_bounds_above_and_orders_ascending() {
        let sql = keyset_sql(Some(&PageCursor::After(uri("urn:ngsi-ld:Subscription:001"))));
        assert!(sql.contains(r#""subscriptions"."id" > 'urn:ngsi-ld:Subscription:001'"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" ASC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// A backward cursor keeps the identifiers below the boundary, descending,
    /// so a single index scan serves the page before it is reversed.
    #[test]
    fn a_backward_cursor_bounds_below_and_orders_descending() {
        let sql = keyset_sql(Some(&PageCursor::Before(uri("urn:ngsi-ld:Subscription:050"))));
        assert!(sql.contains(r#""subscriptions"."id" < 'urn:ngsi-ld:Subscription:050'"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" DESC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// The from-end cursor takes the final identifiers descending with no bound.
    #[test]
    fn a_from_end_cursor_orders_descending_without_a_bound() {
        let sql = keyset_sql(Some(&PageCursor::End));
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" DESC"#), "{sql}");
        assert!(!sql.contains(r#""id" >"#) && !sql.contains(r#""id" <"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    #[test]
    fn every_projection_reads_the_delivery_counters_from_the_joined_state_row() {
        let sql = projected_select(StoredKind::Entity, &ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"LEFT JOIN "subscription_delivery_state""#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."dataset_id""#), "{sql}");
        for counter in ["times_sent", "last_notification", "last_success", "last_failure"] {
            assert!(
                !sql.contains(&format!(r#""subscriptions"."{counter}""#)),
                "the subscription row carries no {counter}: {sql}"
            );
        }
        let csource = projected_select(StoredKind::Csource, &ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(!csource.contains("dataset_id"), "{csource}");
    }
}
