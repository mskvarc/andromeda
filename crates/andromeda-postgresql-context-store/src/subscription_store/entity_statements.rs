//! The create and replace statements of an entity subscription's
//! `subscriptions` row and the binders that fill them.
//!
//! The marker order a statement emits is the order its binder pushes values,
//! so the two are kept side by side. A create returns the surrogate key it
//! minted, because the delivery-state row that completes the subscription is
//! inserted under it in the same transaction.
//!
//! `notification_trigger` is stored as the clause-5.3.3 registry codes the
//! notification queue is numbered from, not as their camel-case labels, so a
//! subscription's trigger set and a queued notification's trigger share one
//! numbering. `kind` and `status` are registry codes too, and the three
//! attribute lists are the dictionary keys [`InternedAttributeLists`] carries.

use crate::subscription_store::{
    conformance_text::stored_conformance,
    failure::persistence_error,
    interned_attributes::InternedAttributeLists,
    status_code::stored_status,
    stored_kind::{StoredKind, of_kind},
    trigger_code::stored_trigger_list,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_ngsi_ld_query::filter::serialize::serialize_written_members;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{clock::now, placeholder::external_placeholder};
use andromeda_subscription::{error::SubscriptionError, store::SubscriptionFields};
use iri_rs::IriBuf;
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query};
use sqlx::{postgres::PgArguments, query::Query as SqlxQuery, types::Json};

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
        .bind(stored_trigger_list(fields.notification_trigger))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(fields.dataset_id.map(|selectors| selectors.iter().map(ToString::to_string).collect::<Vec<_>>()))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.temporal_q.map(Json))
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(IriBuf::as_str))
        .bind(fields.local_only)
        .bind(fields.split_entities)
        .bind(stored_conformance(fields.ngsild_conformance))
        .bind(fields.q.map(serialize_written_members))
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
        .bind(stored_trigger_list(fields.notification_trigger))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(fields.geo_q.map(Json))
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(fields.dataset_id.map(|selectors| selectors.iter().map(ToString::to_string).collect::<Vec<_>>()))
        .bind(fields.notification.map(Json))
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(IriBuf::as_str))
        .bind(fields.local_only)
        .bind(fields.split_entities)
        .bind(stored_conformance(fields.ngsild_conformance))
        .bind(fields.q.map(serialize_written_members))
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
        Subscriptions::QWrittenMembers,
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
            external_placeholder(25),
            now(),
            now(),
        ])
        .map_err(|error| persistence_error(error))?
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
            (Subscriptions::QWrittenMembers, external_placeholder(24)),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(of_kind(StoredKind::Entity));
    update.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::entity_statements::{entity_create_sql, entity_update_sql};
    use andromeda_context_space::context_target::ContextTarget;

    #[test]
    fn the_entity_create_carries_its_kind_code_and_binds_its_status_code_with_contiguous_markers() {
        let sql = entity_create_sql(&ContextTarget::Live).expect("the fixed entity insert builds");
        assert!(sql.contains("VALUES ($1, 1, $2"), "the entity kind code: {sql}");
        assert!(!sql.contains("'Subscription'"), "the constant type token is no longer a column: {sql}");
        assert!(!sql.contains("::subscription_status"), "no enum cast remains: {sql}");
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        for marker in 1..=25 {
            assert!(sql.contains(&format!("${marker}")), "missing ${marker}: {sql}");
        }
        assert!(!sql.contains("$26"), "the create binds exactly twenty-five values: {sql}");
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
    }

    #[test]
    fn the_create_returns_the_surrogate_key_the_delivery_state_row_is_inserted_under() {
        let sql = entity_create_sql(&ContextTarget::Live).expect("builds");
        assert!(sql.ends_with(r#"RETURNING "subscription_key""#), "{sql}");
    }

    /// Table 5.2.12-1 opt-in lists, `ngsildConformance`, and the written
    /// members of `q` that clause 4.9 reads a target language and a raw JSON
    /// key in, carried by both statements.
    #[test]
    fn both_statements_carry_the_clause_5_2_12_members_and_the_written_members_of_q() {
        for sql in [
            entity_create_sql(&ContextTarget::Live).expect("builds"),
            entity_update_sql(&ContextTarget::Live),
        ] {
            for column in ["expand_values", "json_keys", "ngsild_conformance", "q_written_members"] {
                assert!(sql.contains(column), "{column}: {sql}");
            }
        }
    }

    /// An entity subscription is no context source change, so neither
    /// statement takes the context source change clock.
    #[test]
    fn neither_statement_advances_the_context_source_change_clock() {
        for sql in [
            entity_create_sql(&ContextTarget::Live).expect("builds"),
            entity_update_sql(&ContextTarget::Live),
        ] {
            assert!(!sql.contains("context_source_change_clock"), "{sql}");
            assert!(!sql.contains("context_source_revision"), "{sql}");
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
    fn the_replacement_touches_no_delivery_counter_and_no_dropped_column() {
        let sql = entity_update_sql(&ContextTarget::Live);
        for counter in ["times_sent", "times_failed", "last_notification", "last_success", "last_failure"] {
            assert!(!sql.contains(counter), "a client update must not move {counter}: {sql}");
        }
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
        assert!(!sql.contains(r#""type""#), "the constant type column is gone: {sql}");
    }
}
