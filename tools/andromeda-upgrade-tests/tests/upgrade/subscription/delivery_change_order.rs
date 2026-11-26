//! A delivery v1.0.0 queued carries the commit instant and the write position
//! of the change it notifies once the tenant is upgraded, taken from its
//! event, so a Subscription's queue is ordered by its changes rather than by
//! the keys v1.0.0 drew. A delivery queued after the upgrade names them
//! itself.

use crate::fixtures::{
    deliveries::{insert_delivery, insert_event, insert_subscription},
    rehearsal::at_v1_0_0,
    statements::{instant, name},
    test_error::UpgradeTestError,
};
use andromeda_postgresql_seaquery::statement::Statement;
use pretty_assertions::assert_eq;
use sea_query::{Alias, Expr, ExprTrait, JoinType, Order, Query, SelectStatement};
use sqlx::{PgPool, error::ErrorKind, postgres::PgDatabaseError};

/// The column a write refused for leaving it NULL names, if the write was
/// refused for that.
fn column_left_null(written: &Result<i64, UpgradeTestError>) -> Option<&str> {
    let Err(UpgradeTestError::Query(sqlx::Error::Database(error))) = written else {
        return None;
    };
    if error.kind() != ErrorKind::NotNullViolation {
        return None;
    }
    error.try_downcast_ref::<PgDatabaseError>().and_then(PgDatabaseError::column)
}

/// `SELECT d.event_key FROM notification_deliveries AS d INNER JOIN
/// notification_events AS e ON e.event_key = d.event_key WHERE d.committed_at =
/// e.committed_at AND d.event_ordinal = e.event_ordinal`: the deliveries that
/// carry their event's commit instant and write position.
fn deliveries_matching_their_events() -> SelectStatement {
    let (delivery, event) = (name("d"), name("e"));
    let column = |table: &Alias, column: &str| Expr::col((table.clone(), name(column)));
    Query::select()
        .column((delivery.clone(), name("event_key")))
        .from_as(name("notification_deliveries"), delivery.clone())
        .join_as(
            JoinType::InnerJoin,
            name("notification_events"),
            event.clone(),
            column(&event, "event_key").equals((delivery.clone(), name("event_key"))),
        )
        .and_where(column(&delivery, "committed_at").equals((event.clone(), name("committed_at"))))
        .and_where(column(&delivery, "event_ordinal").equals((event.clone(), name("event_ordinal"))))
        .order_by((delivery, name("delivery_key")), Order::Asc)
        .to_owned()
}

/// The events of the queued deliveries, in the order of the changes they
/// notify.
async fn events_in_change_order(pool: &PgPool) -> Result<Vec<i64>, UpgradeTestError> {
    let select = Query::select()
        .column(name("event_key"))
        .from(name("notification_deliveries"))
        .order_by(name("committed_at"), Order::Asc)
        .order_by(name("event_ordinal"), Order::Asc)
        .order_by(name("delivery_key"), Order::Asc)
        .to_owned();
    Ok(select.to_query_scalar::<i64>().fetch_all(pool).await?)
}

#[tokio::test]
async fn a_queued_v1_0_0_delivery_takes_the_commit_instant_and_write_position_of_its_change() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let subscription = insert_subscription(database.pool()).await?;
    let first_of_later_write = insert_event(database.pool(), "00000000-0000-4000-8000-0000000000d1", 0, "2025-01-27T21:00:02Z").await?;
    let earlier_write = insert_event(database.pool(), "00000000-0000-4000-8000-0000000000d2", 3, "2025-01-27T21:00:01Z").await?;
    let second_of_later_write = insert_event(database.pool(), "00000000-0000-4000-8000-0000000000d1", 1, "2025-01-27T21:00:02Z").await?;
    for event in [first_of_later_write, earlier_write, second_of_later_write] {
        insert_delivery(database.pool(), event, subscription, Vec::new()).await?;
    }

    database.migrate_to_current().await?;

    assert_eq!(
        deliveries_matching_their_events().to_query_scalar::<i64>().fetch_all(database.pool()).await?,
        [first_of_later_write, earlier_write, second_of_later_write]
    );
    assert_eq!(
        events_in_change_order(database.pool()).await?,
        [earlier_write, first_of_later_write, second_of_later_write]
    );
    Ok(())
}

#[tokio::test]
async fn a_delivery_queued_after_the_upgrade_without_its_change_order_is_refused() -> Result<(), UpgradeTestError> {
    let (mut database, _) = at_v1_0_0().await?;
    let subscription = insert_subscription(database.pool()).await?;
    let event = insert_event(database.pool(), "00000000-0000-4000-8000-0000000000d3", 0, "2025-01-27T21:00:01Z").await?;

    database.migrate_to_current().await?;

    let without_instant = insert_delivery(database.pool(), event, subscription, vec![("event_ordinal", Expr::val(0))]).await;
    let without_position = insert_delivery(database.pool(), event, subscription, vec![("committed_at", instant("2025-01-27T21:00:01Z"))]).await;
    assert_eq!(
        [column_left_null(&without_instant), column_left_null(&without_position)],
        [Some("committed_at"), Some("event_ordinal")],
        "{without_instant:?} {without_position:?}"
    );
    Ok(())
}
