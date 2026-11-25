//! The durable notification deliveries of one Subscription, read from the live branch of a tenant database over a
//! direct connection to the PostgreSQL server of the broker holding the tenant, with the server's administrative
//! login: the instants the queue records for each delivery, which no NGSI-LD resource reports.

use crate::{
    config::{BrokerRole, SuiteConfig},
    config_error::ConfigError,
    database_name::DatabaseName,
};
use andromeda_postgresql_schema::{notification::NotificationDeliveries, subscriptions::Subscriptions};
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};
use ngsi_ld::subscription::SubscriptionId;
use sea_query::{Expr, ExprTrait, Iden, Order, Query, SelectStatement};
use sqlx::{
    PgPool,
    Row,
    postgres::{PgPoolOptions, PgRow},
};
use std::time::Duration;
use url::Url;

/// The longest a reading waits for its one connection.
const ACQUIRE_TIMEOUT: Duration = Duration::from_secs(10);

/// Why the queue could not be read.
#[derive(Debug, thiserror::Error)]
pub enum DeliveryQueueError {
    /// The stack names no server for the broker.
    #[error(transparent)]
    Config(#[from] ConfigError),
    /// The server could not be reached, the statement failed or a column did not decode.
    #[error("the tenant database could not be read")]
    Server(#[from] sqlx::Error),
}

/// What the queue records of one delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueuedDelivery {
    /// The attempts made so far.
    pub attempt_count: i32,
    /// When the delivery is claimable once it is neither held nor leased: its first attempt, or the retry after its
    /// last failure.
    pub next_attempt_at: DateTime<Utc>,
    /// When the delivery was set aside behind its Subscription's probe; `None` while it is not.
    pub held_at: Option<DateTime<Utc>>,
    /// When the endpoint accepted it; `None` until then.
    pub delivered_at: Option<DateTime<Utc>>,
    /// When it ran out of attempts; `None` while it has some left.
    pub exhausted_at: Option<DateTime<Utc>>,
    /// The instant of the commit that made the change it notifies, as the delivery records it from its event.
    pub committed_at: DateTime<Utc>,
}

/// A direct connection to one tenant database on the server of one broker.
#[derive(Debug, Clone)]
pub struct DeliveryQueue {
    /// One connection, opened on first use.
    pool: PgPool,
}

impl DeliveryQueue {
    /// The tenant database `database` on the server of the broker of `role`.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryQueueError::Config`] when the stack names no such server.
    pub fn of(role: BrokerRole, database: &DatabaseName) -> Result<DeliveryQueue, DeliveryQueueError> {
        DeliveryQueue::at(&SuiteConfig::get().map_err(Clone::clone)?.servers().database(role, database)?)
    }

    /// The default tenant's database on the server of the broker of `role`: the platform database the server is
    /// named with, which is the only tenant database of a broker with multi-tenancy disabled.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryQueueError::Config`] when the stack names no such server.
    pub fn of_default_tenant(role: BrokerRole) -> Result<DeliveryQueue, DeliveryQueueError> {
        DeliveryQueue::at(SuiteConfig::get().map_err(Clone::clone)?.servers().server(role)?)
    }

    /// The database `database` names, reached over one connection opened on first use.
    fn at(database: &Url) -> Result<DeliveryQueue, DeliveryQueueError> {
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(ACQUIRE_TIMEOUT)
            .connect_lazy(database.as_str())?;
        Ok(DeliveryQueue { pool })
    }

    /// The deliveries queued for `subscription`, in the order of the changes they notify: by commit instant, then by
    /// position within the write, as the broker orders them.
    ///
    /// # Errors
    ///
    /// Returns [`DeliveryQueueError::Server`] when the database cannot be read.
    pub async fn deliveries_of(&self, subscription: &SubscriptionId) -> Result<Vec<QueuedDelivery>, DeliveryQueueError> {
        deliveries_statement(subscription)
            .to_query()
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(queued_delivery)
            .collect::<Result<_, sqlx::Error>>()
            .map_err(DeliveryQueueError::from)
    }
}

/// `SELECT d.attempt_count, d.next_attempt_at, d.held_at, d.delivered_at, d.exhausted_at, d.committed_at FROM
/// notification_deliveries d JOIN subscriptions s ... WHERE s.id = $1 ORDER BY d.committed_at, d.event_ordinal,
/// d.delivery_key`.
fn deliveries_statement(subscription: &SubscriptionId) -> SelectStatement {
    Query::select()
        .columns(
            [
                NotificationDeliveries::AttemptCount,
                NotificationDeliveries::NextAttemptAt,
                NotificationDeliveries::HeldAt,
                NotificationDeliveries::DeliveredAt,
                NotificationDeliveries::ExhaustedAt,
                NotificationDeliveries::CommittedAt,
            ]
            .map(|column| (NotificationDeliveries::Table, column)),
        )
        .from(NotificationDeliveries::Table)
        .inner_join(
            Subscriptions::Table,
            Expr::col((Subscriptions::Table, Subscriptions::SubscriptionKey)).equals((NotificationDeliveries::Table, NotificationDeliveries::SubscriptionKey)),
        )
        .and_where(Expr::col((Subscriptions::Table, Subscriptions::Id)).eq(subscription.as_str()))
        .order_by_columns(NotificationDeliveries::CHANGE_ORDER.map(|column| ((NotificationDeliveries::Table, column), Order::Asc)))
        .to_owned()
}

/// One row of [`deliveries_statement`], decoded.
fn queued_delivery(row: &PgRow) -> Result<QueuedDelivery, sqlx::Error> {
    Ok(QueuedDelivery {
        attempt_count: row.try_get(NotificationDeliveries::AttemptCount.unquoted())?,
        next_attempt_at: row.try_get(NotificationDeliveries::NextAttemptAt.unquoted())?,
        held_at: row.try_get(NotificationDeliveries::HeldAt.unquoted())?,
        delivered_at: row.try_get(NotificationDeliveries::DeliveredAt.unquoted())?,
        exhausted_at: row.try_get(NotificationDeliveries::ExhaustedAt.unquoted())?,
        committed_at: row.try_get(NotificationDeliveries::CommittedAt.unquoted())?,
    })
}

#[cfg(test)]
mod tests {
    use crate::delivery_queue::deliveries_statement;
    use googletest::prelude::*;
    use ngsi_ld::subscription::SubscriptionId;
    use sea_query::PostgresQueryBuilder;

    #[gtest]
    fn the_deliveries_of_a_subscription_are_read_in_the_order_of_the_changes_they_notify() -> Result<()> {
        let subscription = SubscriptionId::try_from("urn:ngsi-ld:Subscription:s")?;
        let (sql, _) = deliveries_statement(&subscription).build(PostgresQueryBuilder);
        verify_that!(
            sql,
            eq(concat!(
                r#"SELECT "notification_deliveries"."attempt_count", "notification_deliveries"."next_attempt_at", "#,
                r#""notification_deliveries"."held_at", "notification_deliveries"."delivered_at", "#,
                r#""notification_deliveries"."exhausted_at", "notification_deliveries"."committed_at" "#,
                r#"FROM "notification_deliveries" "#,
                r#"INNER JOIN "subscriptions" ON "subscriptions"."subscription_key" = "notification_deliveries"."subscription_key" "#,
                r#"WHERE "subscriptions"."id" = $1 ORDER BY "notification_deliveries"."committed_at" ASC, "#,
                r#""notification_deliveries"."event_ordinal" ASC, "notification_deliveries"."delivery_key" ASC"#
            ))
        )
    }
}
