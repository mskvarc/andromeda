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
//! numbering. `kind` and `status` are registry codes too, and the three
//! attribute lists are the dictionary keys [`InternedAttributeLists`] carries.

use crate::subscription_store::{
    failure::persistence_error,
    interned_attributes::InternedAttributeLists,
    status_code::stored_status,
    stored_kind::{StoredKind, of_kind},
    trigger_code::stored_trigger,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{clock::now, placeholder::external_placeholder};
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

/// The stored `major.minor` text of a subscription's `ngsildConformance`, or
/// `None` when it declares no version.
///
/// Table 5.2.12-1 defines the member as text of that form, which is what a
/// retrieve has to echo, so the column holds the rendering of the parsed
/// version rather than a decomposed encoding no query compares.
fn stored_conformance(fields: &SubscriptionFields<'_>) -> Option<String> {
    fields.ngsild_conformance.map(|version| version.to_string())
}

/// Bind an entity-subscription create command in statement order.
pub fn bind_entity_create<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Identifier,
    fields: &'q SubscriptionFields<'q>,
    interned: &'q InternedAttributeLists,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(interned.watched_attributes.as_deref())
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(fields.dataset_id.map(|ids| ids.iter().map(Identifier::as_str).collect::<Vec<_>>()))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(fields.local_only)
        .bind(fields.split_entities)
        .bind(stored_conformance(fields))
}

/// Bind an entity-subscription update command in statement order.
pub fn bind_entity_update<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Identifier,
    fields: &'q SubscriptionFields<'q>,
    interned: &'q InternedAttributeLists,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(interned.watched_attributes.as_deref())
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(fields.dataset_id.map(|ids| ids.iter().map(Identifier::as_str).collect::<Vec<_>>()))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(fields.local_only)
        .bind(fields.split_entities)
        .bind(stored_conformance(fields))
}

/// Bind a context-source-subscription create command in statement order.
pub fn bind_csource_create<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Identifier,
    fields: &'q SubscriptionFields<'q>,
    interned: &'q InternedAttributeLists,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(interned.watched_attributes.as_deref())
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(stored_conformance(fields))
}

/// Bind a context-source-subscription update command in statement order.
pub fn bind_csource_update<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Identifier,
    fields: &'q SubscriptionFields<'q>,
    interned: &'q InternedAttributeLists,
    status: SubscriptionStatus,
) -> SqlxQuery<'q, sqlx::Postgres, PgArguments> {
    query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(fields.entities.map(Json))
        .bind(interned.watched_attributes.as_deref())
        .bind(stored_triggers(fields))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(Url::as_str))
        .bind(stored_conformance(fields))
}

/// The entity-subscription insert, its field values bound by
/// [`bind_entity_create`] in the marker order this emits.
pub fn entity_create_sql(target: &ContextTarget) -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert.into_table(table_of(target, Subscriptions::Table)).columns([
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
        Subscriptions::ExpandValues,
        Subscriptions::JsonKeys,
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
        Subscriptions::NgsildConformance,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            Expr::val(StoredKind::Entity.code().code()),
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
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            external_placeholder(22),
            external_placeholder(23),
            external_placeholder(24),
            now(),
            now(),
        ])
        .map_err(persistence_error)?
        .returning(Query::returning().column(Subscriptions::SubscriptionKey));
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The entity-subscription replacement, bound by [`bind_entity_update`].
pub fn entity_update_sql(target: &ContextTarget) -> String {
    let mut update = Query::update();
    update
        .table(table_of(target, Subscriptions::Table))
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
            (Subscriptions::ExpandValues, external_placeholder(12)),
            (Subscriptions::JsonKeys, external_placeholder(13)),
            (Subscriptions::DatasetId, external_placeholder(14)),
            (Subscriptions::Notification, external_placeholder(15)),
            (Subscriptions::ExpiresAt, external_placeholder(16)),
            (Subscriptions::Throttling, external_placeholder(17)),
            (Subscriptions::Status, external_placeholder(18)),
            (Subscriptions::Lang, external_placeholder(19)),
            (Subscriptions::JsonldContext, external_placeholder(20)),
            (Subscriptions::LocalOnly, external_placeholder(21)),
            (Subscriptions::SplitEntities, external_placeholder(22)),
            (Subscriptions::NgsildConformance, external_placeholder(23)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(of_kind(StoredKind::Entity));
    update.to_string(PostgresQueryBuilder)
}

/// The context-source-subscription insert, bound by [`bind_csource_create`].
pub fn csource_create_sql(target: &ContextTarget) -> Result<String, SubscriptionError> {
    let mut insert = Query::insert();
    insert.into_table(table_of(target, Subscriptions::Table)).columns([
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
        Subscriptions::ExpandValues,
        Subscriptions::JsonKeys,
        Subscriptions::Notification,
        Subscriptions::ExpiresAt,
        Subscriptions::Throttling,
        Subscriptions::Status,
        Subscriptions::TemporalQ,
        Subscriptions::Lang,
        Subscriptions::JsonldContext,
        Subscriptions::NgsildConformance,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .values([
            external_placeholder(1),
            Expr::val(StoredKind::Csource.code().code()),
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
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder(18),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            now(),
            now(),
        ])
        .map_err(persistence_error)?
        .returning(Query::returning().column(Subscriptions::SubscriptionKey));
    Ok(insert.to_string(PostgresQueryBuilder))
}

/// The context-source-subscription replacement, bound by [`bind_csource_update`].
pub fn csource_update_sql(target: &ContextTarget) -> String {
    let mut update = Query::update();
    update
        .table(table_of(target, Subscriptions::Table))
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
            (Subscriptions::ExpandValues, external_placeholder(12)),
            (Subscriptions::JsonKeys, external_placeholder(13)),
            (Subscriptions::Notification, external_placeholder(14)),
            (Subscriptions::ExpiresAt, external_placeholder(15)),
            (Subscriptions::Throttling, external_placeholder(16)),
            (Subscriptions::Status, external_placeholder(17)),
            (Subscriptions::Lang, external_placeholder(18)),
            (Subscriptions::JsonldContext, external_placeholder(19)),
            (Subscriptions::NgsildConformance, external_placeholder(20)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(of_kind(StoredKind::Csource));
    update.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::create_update_sql::{csource_create_sql, csource_update_sql, entity_create_sql, entity_update_sql};
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn the_entity_create_carries_its_kind_code_and_binds_its_status_code_with_contiguous_markers() {
        let sql = entity_create_sql(&ContextTarget::Live).expect("the fixed entity insert builds");
        assert!(sql.contains("VALUES ($1, 1, $2"), "the entity kind code: {sql}");
        assert!(!sql.contains("'Subscription'"), "the constant type token is no longer a column: {sql}");
        assert!(!sql.contains("::subscription_status"), "no enum cast remains: {sql}");
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        for marker in 1..=24 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$25"), "the create binds exactly twenty-four values: {sql}");
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
    }

    #[test]
    fn every_create_returns_the_surrogate_key_the_delivery_state_row_is_inserted_under() {
        for sql in [
            entity_create_sql(&ContextTarget::Live).expect("builds"),
            csource_create_sql(&ContextTarget::Live).expect("builds"),
        ] {
            assert!(sql.ends_with(r#"RETURNING "subscription_key""#), "{sql}");
        }
    }

    #[test]
    fn every_statement_carries_the_clause_5_2_12_opt_in_lists() {
        for sql in [
            entity_create_sql(&ContextTarget::Live).expect("builds"),
            csource_create_sql(&ContextTarget::Live).expect("builds"),
            entity_update_sql(&ContextTarget::Live),
            csource_update_sql(&ContextTarget::Live),
        ] {
            assert!(sql.contains("expand_values") && sql.contains("json_keys"), "{sql}");
        }
    }

    #[test]
    fn the_csource_create_omits_the_dataset_and_local_only_columns() {
        let sql = csource_create_sql(&ContextTarget::Live).expect("the fixed context-source insert builds");
        assert!(sql.contains("VALUES ($1, 2, $2"), "the context-source kind code: {sql}");
        assert!(!sql.contains("::subscription_status"), "no enum cast remains: {sql}");
        assert!(!sql.contains("$22"), "the context-source create binds twenty-one values: {sql}");
        assert!(!sql.contains("dataset_id"), "a context-source subscription stores no datasetId: {sql}");
    }

    /// Table 5.2.12-1 gives `ngsildConformance` to the Subscription datatype,
    /// which clause 5.11.2.3 makes the payload of a context-source
    /// subscription too, so every statement carries the column and a retrieve
    /// of either family echoes what its client wrote.
    #[test]
    fn every_statement_carries_the_clause_5_2_12_conformance_version() {
        for sql in [
            entity_create_sql(&ContextTarget::Live).expect("builds"),
            csource_create_sql(&ContextTarget::Live).expect("builds"),
            entity_update_sql(&ContextTarget::Live),
            csource_update_sql(&ContextTarget::Live),
        ] {
            assert!(sql.contains("ngsild_conformance"), "{sql}");
        }
    }

    #[test]
    fn the_entity_update_pins_the_row_by_id_and_kind() {
        let sql = entity_update_sql(&ContextTarget::Live);
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."kind" = 1"#), "{sql}");
        assert!(sql.contains(r#""status" = $18"#), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
    }

    #[test]
    fn no_replacement_touches_a_delivery_counter_or_the_dropped_columns() {
        for sql in [entity_update_sql(&ContextTarget::Live), csource_update_sql(&ContextTarget::Live)] {
            for counter in ["times_sent", "times_failed", "last_notification", "last_success", "last_failure"] {
                assert!(!sql.contains(counter), "a client update must not move {counter}: {sql}");
            }
            assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
            assert!(!sql.contains(r#""type""#), "the constant type column is gone: {sql}");
        }
    }
}
