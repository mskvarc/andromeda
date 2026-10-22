//! The read-side projection every subscription query shares: the column set the
//! mapper reads, the family pin, the total count, and the window of a listing
//! page (clause 5.5.9).

use crate::{
    store_fault::persistence_error,
    subscription_store::{
        delivery_state::join_delivery_state,
        stored_kind::{StoredKind, of_kind},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{page_window::PageWindow, read_order::ReadOrder};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_query::{dictionary::names_in_key_order::attribute_names_of, page_window::apply_identifier_window};
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{aggregate::count_all, statement::Statement};
use andromeda_subscription::error::SubscriptionError;
use sea_query::{CaseStatement, Expr, ExprTrait, Keyword, Query, SelectStatement, SimpleExpr};

/// The attribute IRIs behind one of a subscription's interned attribute lists,
/// in the order the subscriber wrote them; an absent list stays absent rather
/// than reading back as an empty one.
fn attribute_list(column: Subscriptions) -> SimpleExpr {
    let keys = Expr::col((Subscriptions::Table, column));
    CaseStatement::new()
        .case(keys.clone().is_null(), Keyword::Null) // clone: the column is named by the guard and by the resolution
        .finally(attribute_names_of(keys))
        .into()
}

/// Add the stored-subscription projection to `select`, in the column set the
/// mapper reads by name; `dataset_id` only for the family that stores it. The
/// delivery counters come from the joined state row, and the three attribute
/// lists are resolved from their dictionary keys under their own column names.
fn project_subscription(select: &mut SelectStatement, kind: StoredKind, target: &ContextTarget) {
    select
        .column((Subscriptions::Table, Subscriptions::Id))
        .column((Subscriptions::Table, Subscriptions::SubscriptionKey))
        .column((Subscriptions::Table, Subscriptions::SubscriptionName))
        .column((Subscriptions::Table, Subscriptions::Description))
        .column((Subscriptions::Table, Subscriptions::Entities))
        .expr_as(attribute_list(Subscriptions::WatchedAttributes), Subscriptions::WatchedAttributes)
        .column((Subscriptions::Table, Subscriptions::NotificationTrigger))
        .column((Subscriptions::Table, Subscriptions::TimeInterval))
        .column((Subscriptions::Table, Subscriptions::Q))
        .column((Subscriptions::Table, Subscriptions::QWrittenMembers))
        .column((Subscriptions::Table, Subscriptions::GeoQ))
        .column((Subscriptions::Table, Subscriptions::ScopeQ))
        .column((Subscriptions::Table, Subscriptions::Csf))
        .expr_as(attribute_list(Subscriptions::ExpandValues), Subscriptions::ExpandValues)
        .expr_as(attribute_list(Subscriptions::JsonKeys), Subscriptions::JsonKeys)
        .column((Subscriptions::Table, Subscriptions::Notification))
        .column((Subscriptions::Table, Subscriptions::ExpiresAt))
        .column((Subscriptions::Table, Subscriptions::Throttling))
        .column((Subscriptions::Table, Subscriptions::Status))
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
pub(super) fn one_of(id: &Identifier, kind: StoredKind) -> sea_query::Cond {
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
    let row: (i64,) = count
        .to_query_as()
        .fetch_one(pool)
        .await
        .map_err(|error| persistence_error::<SubscriptionError>(error))?;
    Ok(row.0)
}

/// The page `window` selects of the subscriptions of one family, in
/// identifier order, and the order its rows come back in (clauses 5.5.9 and
/// 6.3.10).
///
/// The listing is ordered by the identifier in either pagination mode: it is
/// unique, so an offset page is deterministic across executions, and a cursor
/// has a single key to continue from. A backward or from-end page is read
/// descending so one index scan serves it, and presented ascending by the
/// caller.
pub(super) fn listing_page(kind: StoredKind, target: &ContextTarget, window: &PageWindow) -> (SelectStatement, ReadOrder) {
    let mut select = projected_select(kind, target);
    select.and_where(of_kind(kind));
    let read_order = apply_identifier_window(&mut select, Expr::col((Subscriptions::Table, Subscriptions::Id)), window);
    (select, read_order)
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::{
        projection::{listing_page, projected_select},
        stored_kind::StoredKind,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        cursor::PageCursor,
        page_window::{PageStart, PageWindow},
        read_order::ReadOrder,
    };
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    /// The SQL of one listing page of entity subscriptions.
    fn listing_sql(window: &PageWindow) -> String {
        let (select, _) = listing_page(StoredKind::Entity, &ContextTarget::Live, window);
        select.to_string(PostgresQueryBuilder)
    }

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    /// Clause 5.5.9.2: an offset listing pages in the unique identifier order,
    /// so subscriptions created in any order page deterministically.
    #[test]
    fn an_offset_listing_orders_by_id() {
        let sql = listing_sql(&PageWindow::offset(5, 10));
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" ASC LIMIT 5 OFFSET 10"#), "{sql}");
        assert!(!sql.contains("created_at\" ASC"), "{sql}");
    }

    /// The first page of a cursor walk sorts by identifier ascending with no
    /// keyset bound and no offset (clause 5.5.9.1).
    #[test]
    fn a_first_page_orders_by_id_ascending_without_a_bound() {
        let sql = listing_sql(&PageWindow {
            limit: 5,
            start: PageStart::Cursor(None),
        });
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" ASC"#), "{sql}");
        assert!(!sql.contains(r#""id" >"#) && !sql.contains(r#""id" <"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// A forward cursor keeps the identifiers above the boundary, ascending.
    #[test]
    fn a_forward_cursor_bounds_above_and_orders_ascending() {
        let sql = listing_sql(&PageWindow::after(5, uri("urn:ngsi-ld:Subscription:001")));
        assert!(sql.contains(r#""subscriptions"."id" > 'urn:ngsi-ld:Subscription:001'"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" ASC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
    }

    /// A backward cursor keeps the identifiers below the boundary, descending,
    /// so a single index scan serves the page before it is reversed.
    #[test]
    fn a_backward_cursor_bounds_below_and_orders_descending() {
        let (select, read_order) = listing_page(
            StoredKind::Entity,
            &ContextTarget::Live,
            &PageWindow {
                limit: 5,
                start: PageStart::Cursor(Some(PageCursor::Before(uri("urn:ngsi-ld:Subscription:050")))),
            },
        );
        let sql = select.to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""subscriptions"."id" < 'urn:ngsi-ld:Subscription:050'"#), "{sql}");
        assert!(sql.contains(r#"ORDER BY "subscriptions"."id" DESC"#), "{sql}");
        assert!(!sql.contains("OFFSET"), "{sql}");
        assert_eq!(read_order, ReadOrder::Descending);
    }

    /// The from-end cursor takes the final identifiers descending with no bound.
    #[test]
    fn a_from_end_cursor_orders_descending_without_a_bound() {
        let sql = listing_sql(&PageWindow {
            limit: 5,
            start: PageStart::Cursor(Some(PageCursor::End)),
        });
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

    /// The three attribute lists are stored as dictionary keys and read back as
    /// the IRIs they name, in stored order, under their own column names, and
    /// an absent list reads back absent rather than empty.
    #[test]
    fn the_projection_resolves_the_interned_attribute_lists_in_order() {
        let sql = projected_select(StoredKind::Entity, &ContextTarget::Live).to_string(PostgresQueryBuilder);
        for column in ["watched_attributes", "expand_values", "json_keys"] {
            assert!(
                sql.contains(&format!(
                    r#"(CASE WHEN ("subscriptions"."{column}" IS NULL) THEN NULL ELSE ARRAY (SELECT "attribute_names"."name" FROM "attribute_names" WHERE "attribute_names"."attribute_name_key" = ANY("subscriptions"."{column}") ORDER BY array_position("subscriptions"."{column}", "attribute_names"."attribute_name_key") ASC) END) AS "{column}""#
                )),
                "{column}: {sql}"
            );
        }
    }

    #[test]
    fn the_projection_reads_the_status_code_without_a_cast() {
        let sql = projected_select(StoredKind::Entity, &ContextTarget::Live).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#""subscriptions"."status","#), "{sql}");
        assert!(!sql.contains("::text"), "{sql}");
    }
}
