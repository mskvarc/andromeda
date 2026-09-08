//! The create and replace statements of the `subscriptions` row and the
//! binders that fill them.
//!
//! Each family has its own statement because the column set differs by
//! `dataset_id` and `local_only`; the marker order a statement emits is the
//! order its binder pushes values, so the two are kept side by side. A create
//! returns the surrogate key it minted, because the delivery-state row that
//! completes the subscription is inserted under it in the same transaction.
//!
//! `notification_trigger` is stored as the clause-5.3.3 registry codes the
//! notification queue is numbered from, not as their camel-case labels, so a
//! subscription's trigger set and a queued notification's trigger share one
//! numbering.

use crate::{
    notification_delivery::trigger_code::stored_trigger,
    subscription_store::{
        failure::persistence_error,
        stored_kind::{StoredKind, of_kind},
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{cast::cast, clock::now, literal::text_literal, pg_type::PostgresType, placeholder::external_placeholder};
use andromeda_subscription::{error::SubscriptionError, store::SubscriptionFields};
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query};
use sqlx::{postgres::PgArguments, query::Query as SqlxQuery, types::Json};
use url::Url;

/// The stored `SMALLINT[]` of a subscription's trigger set, or `None` when the
/// subscription carries no explicit set (matching every trigger).
fn stored_triggers(fields: &SubscriptionFields<'_>) -> Option<Vec<i16>> {
    fields
        .notification_trigger
        .map(|triggers| triggers.iter().map(|trigger| stored_trigger(*trigger).code()).collect())
}

/// Bind an entity-subscription create command in statement order.
pub fn bind_entity_create<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    target: &'q ContextTarget,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(andromeda_ngsi_ld_lexicon::attribute_name::AttributeName::as_str).collect::<Vec<_>>()),
        )
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.dataset_id.map(|ids| ids.iter().map(Uri::as_str).collect::<Vec<_>>()))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(fields.local_only)
        .bind(fields.split_entities)
        .bind(target.key().as_i64())
}

/// Bind an entity-subscription update command in statement order.
pub fn bind_entity_update<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    target: &'q ContextTarget,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(andromeda_ngsi_ld_lexicon::attribute_name::AttributeName::as_str).collect::<Vec<_>>()),
        )
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.dataset_id.map(|ids| ids.iter().map(Uri::as_str).collect::<Vec<_>>()))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(fields.local_only)
        .bind(fields.split_entities)
        .bind(target.key().as_i64())
}

/// Bind a context-source-subscription create command in statement order.
pub fn bind_csource_create<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    target: &'q ContextTarget,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(andromeda_ngsi_ld_lexicon::attribute_name::AttributeName::as_str).collect::<Vec<_>>()),
        )
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(target.key().as_i64())
}

/// Bind a context-source-subscription update command in statement order.
pub fn bind_csource_update<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    target: &'q ContextTarget,
    id: &'q Uri,
    fields: &'q SubscriptionFields<'q>,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(
            fields
                .watched_attributes
                .map(|names| names.iter().map(andromeda_ngsi_ld_lexicon::attribute_name::AttributeName::as_str).collect::<Vec<_>>()),
        )
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(<&str>::from(status))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(target.key().as_i64())
}

/// The entity-subscription insert, its field values bound by
/// [`bind_entity_create`] in the marker order this emits.
pub fn entity_create_sql() -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert.into_table(Subscriptions::Table).columns([
        Subscriptions::Id,
        Subscriptions::Kind,
        Subscriptions::SubscriptionName,
        Subscriptions::Description,
        Subscriptions::Entities,
        Subscriptions::WatchedAttributes,
        Subscriptions::NotificationTrigger,
        Subscriptions::TimeInterval,
        Subscriptions::Q,
        Subscriptions::GeoQ,
        Subscriptions::ScopeQ,
        Subscriptions::Csf,
        Subscriptions::DatasetId,
        Subscriptions::Notification,
        Subscriptions::ExpiresAt,
        Subscriptions::Throttling,
        Subscriptions::Status,
        Subscriptions::TemporalQ,
        Subscriptions::Lang,
        Subscriptions::JsonldContext,
        Subscriptions::LocalOnly,
        Subscriptions::SplitEntities,
        Subscriptions::ContextSpaceKey,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            text_literal("entity"),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            external_placeholder(9),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            external_placeholder(15),
            cast(external_placeholder(16), PostgresType::SubscriptionStatus),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            external_placeholder(22),
            now(),
            now(),
        ])
        .map_err(persistence_error)?
        .returning(Query::returning().column(Subscriptions::SubscriptionKey));
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The entity-subscription replacement, bound by [`bind_entity_update`].
pub fn entity_update_sql() -> String {
    let mut update = Query::update();
    update
        .table(Subscriptions::Table)
        .values([
            (Subscriptions::SubscriptionName, external_placeholder(2)),
            (Subscriptions::Description, external_placeholder(3)),
            (Subscriptions::Entities, external_placeholder(4)),
            (Subscriptions::WatchedAttributes, external_placeholder(5)),
            (Subscriptions::NotificationTrigger, external_placeholder(6)),
            (Subscriptions::TimeInterval, external_placeholder(7)),
            (Subscriptions::Q, external_placeholder(8)),
            (Subscriptions::GeoQ, external_placeholder(9)),
            (Subscriptions::ScopeQ, external_placeholder(10)),
            (Subscriptions::Csf, external_placeholder(11)),
            (Subscriptions::DatasetId, external_placeholder(12)),
            (Subscriptions::Notification, external_placeholder(13)),
            (Subscriptions::ExpiresAt, external_placeholder(14)),
            (Subscriptions::Throttling, external_placeholder(15)),
            (Subscriptions::Status, cast(external_placeholder(16), PostgresType::SubscriptionStatus)),
            (Subscriptions::Lang, external_placeholder(17)),
            (Subscriptions::JsonldContext, external_placeholder(18)),
            (Subscriptions::LocalOnly, external_placeholder(19)),
            (Subscriptions::SplitEntities, external_placeholder(20)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(Expr::col(Subscriptions::ContextSpaceKey).eq(external_placeholder(21)))
        .and_where(of_kind(StoredKind::Entity));
    update.to_string(PostgresQueryBuilder)
}

/// The context-source-subscription insert, bound by [`bind_csource_create`].
pub fn csource_create_sql() -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert.into_table(Subscriptions::Table).columns([
        Subscriptions::Id,
        Subscriptions::Kind,
        Subscriptions::SubscriptionName,
        Subscriptions::Description,
        Subscriptions::Entities,
        Subscriptions::WatchedAttributes,
        Subscriptions::NotificationTrigger,
        Subscriptions::TimeInterval,
        Subscriptions::Q,
        Subscriptions::GeoQ,
        Subscriptions::ScopeQ,
        Subscriptions::Csf,
        Subscriptions::Notification,
        Subscriptions::ExpiresAt,
        Subscriptions::Throttling,
        Subscriptions::Status,
        Subscriptions::TemporalQ,
        Subscriptions::Lang,
        Subscriptions::JsonldContext,
        Subscriptions::ContextSpaceKey,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            text_literal("csource"),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder(4),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            external_placeholder(9),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder(14),
            cast(external_placeholder(15), PostgresType::SubscriptionStatus),
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            now(),
            now(),
        ])
        .map_err(persistence_error)?
        .returning(Query::returning().column(Subscriptions::SubscriptionKey));
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The context-source-subscription replacement, bound by [`bind_csource_update`].
pub fn csource_update_sql() -> String {
    let mut update = Query::update();
    update
        .table(Subscriptions::Table)
        .values([
            (Subscriptions::SubscriptionName, external_placeholder(2)),
            (Subscriptions::Description, external_placeholder(3)),
            (Subscriptions::Entities, external_placeholder(4)),
            (Subscriptions::WatchedAttributes, external_placeholder(5)),
            (Subscriptions::NotificationTrigger, external_placeholder(6)),
            (Subscriptions::TimeInterval, external_placeholder(7)),
            (Subscriptions::Q, external_placeholder(8)),
            (Subscriptions::GeoQ, external_placeholder(9)),
            (Subscriptions::ScopeQ, external_placeholder(10)),
            (Subscriptions::Csf, external_placeholder(11)),
            (Subscriptions::Notification, external_placeholder(12)),
            (Subscriptions::ExpiresAt, external_placeholder(13)),
            (Subscriptions::Throttling, external_placeholder(14)),
            (Subscriptions::Status, cast(external_placeholder(15), PostgresType::SubscriptionStatus)),
            (Subscriptions::Lang, external_placeholder(16)),
            (Subscriptions::JsonldContext, external_placeholder(17)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(Expr::col(Subscriptions::ContextSpaceKey).eq(external_placeholder(18)))
        .and_where(of_kind(StoredKind::Csource));
    update.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::create_update_sql::{csource_create_sql, csource_update_sql, entity_create_sql, entity_update_sql};

    #[test]
    fn the_entity_create_carries_its_kind_and_status_cast_with_contiguous_markers() {
        let sql = entity_create_sql().expect("the fixed entity insert builds");
        assert!(sql.contains("'entity'"), "{sql}");
        assert!(!sql.contains("'Subscription'"), "the constant type token is no longer a column: {sql}");
        assert!(sql.contains("$16::subscription_status"), "{sql}");
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        for marker in 1..=22 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$23"), "the create binds exactly twenty-two values: {sql}");
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
    }

    #[test]
    fn every_create_returns_the_surrogate_key_the_delivery_state_row_is_inserted_under() {
        for sql in [entity_create_sql().expect("builds"), csource_create_sql().expect("builds")] {
            assert!(sql.ends_with(r#"RETURNING "subscription_key""#), "{sql}");
        }
    }

    #[test]
    fn the_csource_create_omits_the_dataset_and_local_only_columns() {
        let sql = csource_create_sql().expect("the fixed context-source insert builds");
        assert!(sql.contains("'csource'"), "{sql}");
        assert!(sql.contains("$15::subscription_status"), "{sql}");
        assert!(!sql.contains("$20"), "the context-source create binds nineteen values: {sql}");
        assert!(!sql.contains("dataset_id"), "a context-source subscription stores no datasetId: {sql}");
    }

    #[test]
    fn the_entity_update_pins_the_row_by_id_and_kind() {
        let sql = entity_update_sql();
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains("'entity'"), "{sql}");
        assert!(sql.contains("$16::subscription_status"), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
    }

    #[test]
    fn no_replacement_touches_a_delivery_counter_or_the_dropped_columns() {
        for sql in [entity_update_sql(), csource_update_sql()] {
            for counter in ["times_sent", "last_notification", "last_success", "last_failure"] {
                assert!(!sql.contains(counter), "a client update must not move {counter}: {sql}");
            }
            assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
            assert!(!sql.contains(r#""type""#), "the constant type column is gone: {sql}");
        }
    }
}
