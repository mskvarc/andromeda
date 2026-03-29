use super::subscription_store::SubscriptionFields;
use crate::error::AndromedaError;
use ngsi_ld_types::subscription::SubscriptionStatus;
use sqlx::postgres::PgPool;
use tracing::info;

pub struct CsourceSubscriptionRepository {
    pool: PgPool,
}

impl CsourceSubscriptionRepository {
    pub fn new(pool: &PgPool) -> Self {
        Self { pool: pool.clone() }
    }

    pub async fn create(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<(), AndromedaError> {
        sqlx::query(
            r#"
            INSERT INTO csource_subscriptions (
                id, type, subscription_name, description, entities,
                watched_attributes, notification_trigger, time_interval,
                q, geo_q, scope_q, csf, is_active, notification,
                expires_at, throttling, status, temporal_q, lang,
                jsonld_context, created_at, modified_at
            ) VALUES (
                $1, 'Subscription', $2, $3, $4,
                $5, $6, $7,
                $8, $9, $10, $11, $12, $13,
                $14, $15, $16, $17, $18,
                $19, NOW(), NOW()
            )
            "#,
        )
        .bind(id)
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.cloned())
        .bind(fields.watched_attributes)
        .bind(fields.notification_trigger)
        .bind(fields.time_interval)
        .bind(fields.q)
        .bind(fields.geo_q.cloned())
        .bind(fields.scope_q)
        .bind(fields.csf)
        .bind(fields.is_active)
        .bind(fields.notification.cloned())
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(status.to_string())
        .bind(fields.temporal_q.cloned())
        .bind(fields.lang)
        .bind(fields.jsonld_context)
        .execute(&self.pool)
        .await?;

        info!("Created csource subscription: {}", id);

        Ok(())
    }

    pub async fn update(&self, id: &str, fields: &SubscriptionFields<'_>, status: SubscriptionStatus) -> Result<bool, AndromedaError> {
        let result = sqlx::query(
            r#"
            UPDATE csource_subscriptions SET
                subscription_name = $2,
                description = $3,
                entities = $4,
                watched_attributes = $5,
                notification_trigger = $6,
                time_interval = $7,
                q = $8,
                geo_q = $9,
                scope_q = $10,
                csf = $11,
                is_active = $12,
                notification = $13,
                expires_at = $14,
                throttling = $15,
                status = $16,
                lang = $17,
                jsonld_context = $18,
                modified_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.cloned())
        .bind(fields.watched_attributes)
        .bind(fields.notification_trigger)
        .bind(fields.time_interval)
        .bind(fields.q)
        .bind(fields.geo_q.cloned())
        .bind(fields.scope_q)
        .bind(fields.csf)
        .bind(fields.is_active)
        .bind(fields.notification.cloned())
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(status.to_string())
        .bind(fields.lang)
        .bind(fields.jsonld_context)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected() > 0)
    }
}

crate::impl_subscription_store!(
    CsourceSubscriptionRepository,
    table: "csource_subscriptions",
    columns: "id, type, subscription_name, description, entities, \
              watched_attributes, notification_trigger, time_interval, \
              q, geo_q, scope_q, csf, is_active, notification, \
              expires_at, throttling, status, temporal_q, lang, \
              jsonld_context, times_sent, last_notification, \
              last_success, last_failure, created_at, modified_at",
    has_dataset_id: false
);
