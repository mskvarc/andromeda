//! Subscription store trait implementations for both entity and context-source
//! subscriptions.
//!
//! The `SubscriptionStore` trait and the `SubscriptionFields` type it
//! references belong to the policy crate; the statements a subscription's row
//! is created and replaced with live in `entity_statements` and
//! `csource_statements`, the delivery
//! counters and their statements in `delivery_state`, and the projection every
//! read shares in `projection`. This module owns the port implementation
//! itself.

use crate::{
    csource_subscription::repo::CsourceSubscriptionRepository,
    store_fault::persistence_error,
    subscription::repo::SubscriptionRepository,
    subscription_store::{
        csource_statements::{csource_create, csource_update},
        delivery_state::{SlotClaimStatement, outcome_statement, record_interval_tick_statement, slot_claim_statement, state_insert},
        entity_statements::{entity_create, entity_update},
        failure::create_error,
        interned_attributes::intern_attribute_lists,
        mapper::{row_from_pg, row_to_stored_subscription, rows_to_stored_subscriptions, stored_subscription_uri},
        params::DatasetIdPresence,
        periodic_due::{due_interval_select, interval_anchors_select},
        projection::{count_of_kind, listing_page, one_of, projected_select},
        status_code::{status_is, status_is_not, stored_status},
        stored_kind::{StoredKind, of_kind},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::{Subscription, SubscriptionStatus};
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{literal::int_literal, statement::Statement};
use andromeda_subscription::{
    error::SubscriptionError,
    interval_schedule::{interval_duration, next_interval_due as next_interval_grid_due},
    notification_slot::{NotificationOutcome, NotificationSlot, NotificationSlotGate, OutcomeRecord},
    store::{SubscriptionFields, SubscriptionStore},
    stored_subscription::StoredSubscription,
};
use sea_query::{Expr, ExprTrait, Iden, Query};
use sqlx::Row;

/// Generates a `SubscriptionStore` implementation backed by the `subscriptions`
/// table and its `subscription_delivery_state` row, its statements built
/// through sea-query.
macro_rules! impl_subscription_store {
    ($type:ty, kind: $kind:expr, has_dataset_id: $hdi:expr, create: $create:path, update: $update:path) => {
        #[async_trait::async_trait]
        impl SubscriptionStore for $type {
            async fn get(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<StoredSubscription>, SubscriptionError> {
                let mut select = projected_select($kind, target);
                select.cond_where(one_of(id, $kind));
                let pg_row = select
                    .to_query()
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                match pg_row {
                    Some(pg) => {
                        let row = row_from_pg(&pg, $hdi).map_err(|error| persistence_error::<SubscriptionError>(error))?;
                        Ok(Some(row_to_stored_subscription(row)))
                    }
                    None => Ok(None),
                }
            }

            async fn list(&self, target: &ContextTarget, window: &PageWindow) -> Result<(Vec<Subscription>, i64), SubscriptionError> {
                let total = count_of_kind(&self.pool, target, $kind).await?;

                let (select, read_order) = listing_page($kind, target, window);
                let pg_rows = select
                    .to_query()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                // A listing renders resources, so the surrogate key each row
                // carries is dropped rather than threaded into the response.
                let mut subscriptions: Vec<Subscription> = rows_to_stored_subscriptions(&pg_rows, $hdi)
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?
                    .into_iter()
                    .map(|stored| stored.subscription)
                    .collect();
                // Clause 5.5.9.1: a backward or from-end page was read descending
                // so a single index scan could serve it; present it ascending.
                read_order.present(&mut subscriptions);
                Ok((subscriptions, total))
            }

            async fn delete(&self, target: &ContextTarget, id: &Identifier) -> Result<u64, SubscriptionError> {
                let statement = Query::delete()
                    .from_table(table_of(target, Subscriptions::Table))
                    .cond_where(one_of(id, $kind))
                    .to_owned();
                let result = statement
                    .to_query()
                    .execute(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                Ok(result.rows_affected())
            }

            async fn exists(&self, target: &ContextTarget, id: &Identifier) -> Result<bool, SubscriptionError> {
                let inner = Query::select()
                    .expr(int_literal(1))
                    .from(table_of(target, Subscriptions::Table))
                    .cond_where(one_of(id, $kind))
                    .to_owned();
                let statement = Query::select().expr(Expr::exists(inner)).to_owned();
                let row: (bool,) = statement
                    .to_query_as()
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                Ok(row.0)
            }

            async fn list_active(&self, target: &ContextTarget) -> Result<Vec<StoredSubscription>, SubscriptionError> {
                let mut select = projected_select($kind, target);
                select.and_where(status_is(SubscriptionStatus::Active)).and_where(of_kind($kind));
                let pg_rows = select
                    .to_query()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                rows_to_stored_subscriptions(&pg_rows, $hdi).map_err(|error| persistence_error::<SubscriptionError>(error))
            }

            async fn claim_notification_slot(
                &self,
                target: &ContextTarget,
                id: &Identifier,
                gate: NotificationSlotGate,
            ) -> Result<NotificationSlot, SubscriptionError> {
                // The gate is the statement's own `WHERE`, so a state row it
                // did not match is a slot the ceilings refused.
                match slot_claim_statement(target, id, $kind, gate) {
                    SlotClaimStatement::Stamping(statement) => {
                        let result = statement
                            .to_query()
                            .execute(&self.pool)
                            .await
                            .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                        Ok(if result.rows_affected() > 0 {
                            NotificationSlot::Claimed(OutcomeRecord::OutcomeOnly)
                        } else {
                            NotificationSlot::Suppressed
                        })
                    }
                    SlotClaimStatement::Checking(statement) => {
                        let claimed: Option<chrono::DateTime<chrono::Utc>> = statement
                            .to_query_scalar()
                            .fetch_optional(&self.pool)
                            .await
                            .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                        Ok(claimed.map_or(NotificationSlot::Suppressed, |notified_at| {
                            NotificationSlot::Claimed(OutcomeRecord::CountingNotification { notified_at })
                        }))
                    }
                }
            }

            async fn record_outcome(
                &self,
                target: &ContextTarget,
                id: &Identifier,
                outcome: NotificationOutcome,
                record: OutcomeRecord,
            ) -> Result<(), SubscriptionError> {
                outcome_statement(outcome, record, target, id, $kind)
                    .to_query()
                    .execute(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                Ok(())
            }

            async fn expire_due(&self, target: &ContextTarget, cutoff: chrono::DateTime<chrono::Utc>) -> Result<Vec<Identifier>, SubscriptionError> {
                let statement = Query::update()
                    .table(table_of(target, Subscriptions::Table))
                    .value(Subscriptions::Status, stored_status(SubscriptionStatus::Expired).code())
                    .value(Subscriptions::ModifiedAt, Expr::val(cutoff))
                    .and_where(status_is_not(SubscriptionStatus::Expired))
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::ExpiresAt)).is_not_null())
                    .and_where(Expr::col((Subscriptions::Table, Subscriptions::ExpiresAt)).lte(cutoff))
                    .and_where(of_kind($kind))
                    .returning(Query::returning().column(Subscriptions::Id))
                    .to_owned();
                let rows = statement
                    .to_query()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                rows.iter()
                    .map(|row| {
                        row.try_get::<String, _>("id")
                            .map_err(|error| persistence_error::<SubscriptionError>(error))
                            .and_then(|raw| stored_subscription_uri(&raw).map_err(|error| persistence_error::<SubscriptionError>(error)))
                    })
                    .collect()
            }

            /// The subscription row, its delivery-state row and the dictionary
            /// keys of its attribute lists land in one transaction, so a
            /// subscription never exists without the row its counters live on
            /// nor references a key that was rolled back.
            async fn create_subscription(
                &self,
                target: &ContextTarget,
                id: &Identifier,
                fields: &SubscriptionFields<'_>,
                status: SubscriptionStatus,
            ) -> Result<(), SubscriptionError> {
                let mut transaction = self.pool.begin().await.map_err(|error| persistence_error::<SubscriptionError>(error))?;
                let interned = intern_attribute_lists(self.attribute_names.as_ref(), &mut transaction, fields).await?;
                let created = $create(target, id, fields, interned, status)?
                    .to_query()
                    .fetch_one(&mut *transaction)
                    .await
                    .map_err(|error| create_error(error, id))?;
                let subscription_key: i64 = created
                    .try_get("subscription_key")
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                state_insert(target, subscription_key)?
                    .to_query()
                    .execute(&mut *transaction)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                transaction
                    .commit()
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                tracing::info!("Created {} subscription: {}", $kind.code().label(), id);
                Ok(())
            }

            async fn update_subscription(
                &self,
                target: &ContextTarget,
                id: &Identifier,
                fields: &SubscriptionFields<'_>,
                status: SubscriptionStatus,
            ) -> Result<u64, SubscriptionError> {
                let mut transaction = self.pool.begin().await.map_err(|error| persistence_error::<SubscriptionError>(error))?;
                let interned = intern_attribute_lists(self.attribute_names.as_ref(), &mut transaction, fields).await?;
                let result = $update(target, id, fields, interned, status)?
                    .to_query()
                    .execute(&mut *transaction)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                transaction
                    .commit()
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                Ok(result.rows_affected())
            }

            async fn find_due_interval_subscriptions(
                &self,
                target: &ContextTarget,
                cutoff: chrono::DateTime<chrono::Utc>,
            ) -> Result<Vec<StoredSubscription>, SubscriptionError> {
                let pg_rows = due_interval_select($kind, target, cutoff)
                    .to_query()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                rows_to_stored_subscriptions(&pg_rows, $hdi).map_err(|error| persistence_error::<SubscriptionError>(error))
            }

            async fn next_interval_due(
                &self,
                target: &ContextTarget,
                cutoff: chrono::DateTime<chrono::Utc>,
            ) -> Result<Option<chrono::DateTime<chrono::Utc>>, SubscriptionError> {
                let rows = interval_anchors_select($kind, target, cutoff)
                    .to_query()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                let mut earliest: Option<chrono::DateTime<chrono::Utc>> = None;
                for row in &rows {
                    let anchor: chrono::DateTime<chrono::Utc> = row.try_get("anchor").map_err(|error| persistence_error::<SubscriptionError>(error))?;
                    let interval_seconds: f64 = row
                        .try_get(Subscriptions::TimeInterval.unquoted())
                        .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                    // A stored interval that no longer describes a valid grid is
                    // skipped rather than scheduling an impossible wake.
                    let Some(interval) = interval_duration(interval_seconds) else {
                        continue;
                    };
                    let Some(due) = next_interval_grid_due(anchor, interval, cutoff) else {
                        continue;
                    };
                    earliest = Some(earliest.map_or(due, |current| if due < current { due } else { current }));
                }
                Ok(earliest)
            }

            async fn record_interval_tick(
                &self,
                target: &ContextTarget,
                id: &Identifier,
                cutoff: chrono::DateTime<chrono::Utc>,
            ) -> Result<(), SubscriptionError> {
                record_interval_tick_statement(target, id, $kind, cutoff)
                    .to_query()
                    .execute(&self.pool)
                    .await
                    .map_err(|error| persistence_error::<SubscriptionError>(error))?;
                Ok(())
            }
        }
    };
}

impl_subscription_store!(
    SubscriptionRepository,
    kind: StoredKind::Entity,
    has_dataset_id: DatasetIdPresence::Present,
    create: entity_create,
    update: entity_update
);

impl_subscription_store!(
    CsourceSubscriptionRepository,
    kind: StoredKind::Csource,
    has_dataset_id: DatasetIdPresence::Absent,
    create: csource_create,
    update: csource_update
);

#[cfg(test)]
mod tests {
    use crate::{csource_subscription::repo::CsourceSubscriptionRepository, subscription::repo::SubscriptionRepository};
    use andromeda_subscription::store::SubscriptionStore;

    fn assert_subscription_store<T: SubscriptionStore>() {}

    #[test]
    fn entity_subscription_repository_implements_the_policy_port_directly() {
        assert_subscription_store::<SubscriptionRepository>();
    }

    #[test]
    fn csource_subscription_repository_implements_the_policy_port_directly() {
        assert_subscription_store::<CsourceSubscriptionRepository>();
    }
}
