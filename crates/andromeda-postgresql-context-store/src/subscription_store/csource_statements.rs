//! The create and replace statements of a context-source subscription's
//! `subscriptions` row and the binders that fill them.
//!
//! The marker order a statement emits is the order its binder pushes values,
//! so the two are kept side by side. A create returns the surrogate key it
//! minted, because the delivery-state row that completes the subscription is
//! inserted under it in the same transaction. A context-source subscription
//! stores no `dataset_id` and no `local_only`.
//!
//! Both statements advance the tenant's context source change clock in a
//! `WITH` clause and stamp the revision drawn as `context_source_revision`:
//! that orders the write against every registration write (clauses 5.11.2.4
//! and 5.11.3.4). It is the statement's last lock, taken after the attribute
//! lists were interned.

use crate::{
    context_source_change_clock::{advance_clause, advanced_revision},
    json_text::optional_json_text,
    store_fault::persistence_error,
    subscription_store::{
        conformance_text::stored_conformance,
        interned_attributes::InternedAttributeLists,
        status_code::stored_status,
        stored_kind::{StoredKind, of_kind},
        trigger_code::stored_trigger_list,
    },
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
use andromeda_ngsi_ld_query::filter::serialize::serialize_written_members;
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_schema::subscriptions::Subscriptions;
use andromeda_postgresql_seaquery::{
    clock::now,
    pg_type::PostgresType,
    placeholder::{external_placeholder, external_placeholder_as},
};
use andromeda_subscription::{error::SubscriptionError, store::SubscriptionFields};
use iri_rs::IriBuf;
use sea_query::{Expr, ExprTrait, PostgresQueryBuilder, Query};
use sqlx::{postgres::PgArguments, query::Query as SqlxQuery};

/// Bind a context-source-subscription create command in statement order.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON.
pub fn bind_csource_create<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Identifier,
    fields: &'q SubscriptionFields<'q>,
    interned: &'q InternedAttributeLists,
    status: SubscriptionStatus,
) -> Result<SqlxQuery<'q, sqlx::Postgres, PgArguments>, SubscriptionError> {
    Ok(query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(optional_json_text(fields.entities).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(interned.watched_attributes.as_deref())
        .bind(stored_trigger_list(fields.notification_trigger))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(optional_json_text(fields.geo_q).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(optional_json_text(fields.notification).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(optional_json_text(fields.temporal_q).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(IriBuf::as_str))
        .bind(stored_conformance(fields.ngsild_conformance))
        .bind(fields.q.map(serialize_written_members)))
}

/// Bind a context-source-subscription update command in statement order.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON.
pub fn bind_csource_update<'q>(
    query: SqlxQuery<'q, sqlx::Postgres, PgArguments>,
    id: &'q Identifier,
    fields: &'q SubscriptionFields<'q>,
    interned: &'q InternedAttributeLists,
    status: SubscriptionStatus,
) -> Result<SqlxQuery<'q, sqlx::Postgres, PgArguments>, SubscriptionError> {
    Ok(query
        .bind(id.as_str())
        .bind(fields.subscription_name)
        .bind(fields.description)
        .bind(optional_json_text(fields.entities).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(interned.watched_attributes.as_deref())
        .bind(stored_trigger_list(fields.notification_trigger))
        .bind(fields.time_interval)
        .bind(fields.q.map(ToString::to_string))
        .bind(optional_json_text(fields.geo_q).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(fields.scope_q.map(ToString::to_string))
        .bind(fields.csf.map(ToString::to_string))
        .bind(interned.expand_values.as_deref())
        .bind(interned.json_keys.as_deref())
        .bind(optional_json_text(fields.notification).map_err(|error| persistence_error::<SubscriptionError>(error))?)
        .bind(fields.expires_at)
        .bind(fields.throttling)
        .bind(stored_status(status).code())
        .bind(fields.lang.map(ToString::to_string))
        .bind(fields.jsonld_context.map(IriBuf::as_str))
        .bind(stored_conformance(fields.ngsild_conformance))
        .bind(fields.q.map(serialize_written_members)))
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
        Subscriptions::QWrittenMembers,
        Subscriptions::ContextSourceRevision,
        Subscriptions::CreatedAt,
        Subscriptions::ModifiedAt,
    ]);
    insert
        .with_cte(advance_clause())
        .values([
            external_placeholder(1),
            Expr::val(StoredKind::Csource.code().code()),
            external_placeholder(2),
            external_placeholder(3),
            external_placeholder_as(4, PostgresType::Json),
            external_placeholder(5),
            external_placeholder(6),
            external_placeholder(7),
            external_placeholder(8),
            external_placeholder_as(9, PostgresType::Json),
            external_placeholder(10),
            external_placeholder(11),
            external_placeholder(12),
            external_placeholder(13),
            external_placeholder_as(14, PostgresType::Json),
            external_placeholder(15),
            external_placeholder(16),
            external_placeholder(17),
            external_placeholder_as(18, PostgresType::Json),
            external_placeholder(19),
            external_placeholder(20),
            external_placeholder(21),
            external_placeholder(22),
            advanced_revision(),
            now(),
            now(),
        ])
        .map_err(|error| persistence_error::<SubscriptionError>(error))?
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
            (Subscriptions::Entities, external_placeholder_as(4, PostgresType::Json)),
            (Subscriptions::WatchedAttributes, external_placeholder(5)),
            (Subscriptions::NotificationTrigger, external_placeholder(6)),
            (Subscriptions::TimeInterval, external_placeholder(7)),
            (Subscriptions::Q, external_placeholder(8)),
            (Subscriptions::GeoQ, external_placeholder_as(9, PostgresType::Json)),
            (Subscriptions::ScopeQ, external_placeholder(10)),
            (Subscriptions::Csf, external_placeholder(11)),
            (Subscriptions::ExpandValues, external_placeholder(12)),
            (Subscriptions::JsonKeys, external_placeholder(13)),
            (Subscriptions::Notification, external_placeholder_as(14, PostgresType::Json)),
            (Subscriptions::ExpiresAt, external_placeholder(15)),
            (Subscriptions::Throttling, external_placeholder(16)),
            (Subscriptions::Status, external_placeholder(17)),
            (Subscriptions::Lang, external_placeholder(18)),
            (Subscriptions::JsonldContext, external_placeholder(19)),
            (Subscriptions::NgsildConformance, external_placeholder(20)),
            (Subscriptions::QWrittenMembers, external_placeholder(21)),
            (Subscriptions::ContextSourceRevision, advanced_revision()),
            (Subscriptions::ModifiedAt, now()),
        ])
        .and_where(Expr::col(Subscriptions::Id).eq(external_placeholder(1)))
        .and_where(of_kind(StoredKind::Csource))
        .with_cte(advance_clause());
    update.to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::csource_statements::{csource_create_sql, csource_update_sql};
    use andromeda_context_space::context_target::ContextTarget;
    use pretty_assertions::assert_eq;

    #[test]
    fn the_create_carries_its_kind_code_and_omits_the_dataset_and_local_only_columns() {
        let sql = csource_create_sql(&ContextTarget::Live).expect("the fixed context-source insert builds");
        assert!(sql.contains("VALUES ($1, 2, $2"), "the context-source kind code: {sql}");
        assert!(!sql.contains("::subscription_status"), "no enum cast remains: {sql}");
        assert!(sql.contains("$22"), "the context-source create binds twenty-two values: {sql}");
        assert!(!sql.contains("$23"), "the context-source create binds twenty-two values: {sql}");
        assert!(!sql.contains("dataset_id"), "a context-source subscription stores no datasetId: {sql}");
        assert!(!sql.contains("local_only"), "a context-source subscription stores no localOnly: {sql}");
    }

    #[test]
    fn the_create_returns_the_surrogate_key_the_delivery_state_row_is_inserted_under() {
        let sql = csource_create_sql(&ContextTarget::Live).expect("builds");
        assert!(sql.ends_with(r#"RETURNING "subscription_key""#), "{sql}");
    }

    /// Clause 5.11.2.3 makes the Subscription datatype the payload of a
    /// context-source subscription too, so both statements carry its opt-in
    /// lists, `ngsildConformance` and the written members of `q`.
    #[test]
    fn both_statements_carry_the_clause_5_2_12_members_and_the_written_members_of_q() {
        for sql in [
            csource_create_sql(&ContextTarget::Live).expect("builds"),
            csource_update_sql(&ContextTarget::Live),
        ] {
            for column in ["expand_values", "json_keys", "ngsild_conformance", "q_written_members"] {
                assert!(sql.contains(column), "{column}: {sql}");
            }
        }
    }

    /// Clauses 5.11.2.4 and 5.11.3.4: a context-source write is ordered
    /// against every registration write by the clock revision it draws in its
    /// own statement.
    #[test]
    fn both_statements_advance_the_change_clock_and_stamp_the_revision_drawn() {
        for sql in [
            csource_create_sql(&ContextTarget::Live).expect("builds"),
            csource_update_sql(&ContextTarget::Live),
        ] {
            assert!(
                sql.starts_with(r#"WITH "advanced_clock" AS (UPDATE "context_source_change_clock" SET "revision" = "revision" + 1 RETURNING "revision")"#),
                "{sql}"
            );
            assert!(sql.contains(r#""context_source_revision""#), "{sql}");
            assert!(sql.contains(r#"(SELECT "revision" FROM "advanced_clock")"#), "{sql}");
        }
    }

    #[test]
    fn every_csource_subscription_document_is_bound_as_json() {
        let create = csource_create_sql(&ContextTarget::Live).expect("builds");
        let update = csource_update_sql(&ContextTarget::Live);
        for marker in ["$4::json", "$9::json", "$14::json", "$18::json"] {
            assert!(create.contains(marker), "{marker}: {create}");
        }
        for marker in ["$4::json", "$9::json", "$14::json"] {
            assert!(update.contains(marker), "{marker}: {update}");
        }
        assert_eq!(create.matches("::json").count(), 4, "{create}");
        assert_eq!(update.matches("::json").count(), 3, "{update}");
        assert!(!create.contains("::jsonb") && !update.contains("::jsonb"), "{create} {update}");
    }

    #[test]
    fn the_update_pins_the_row_by_id_and_kind() {
        let sql = csource_update_sql(&ContextTarget::Live);
        assert!(sql.contains(r#""id" = ($1)"#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."kind" = 2"#), "{sql}");
        assert!(sql.contains(r#""status" = $17"#), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
    }

    #[test]
    fn the_replacement_touches_no_delivery_counter_and_no_dropped_column() {
        let sql = csource_update_sql(&ContextTarget::Live);
        for counter in ["times_sent", "times_failed", "last_notification", "last_success", "last_failure"] {
            assert!(!sql.contains(counter), "a client update must not move {counter}: {sql}");
        }
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
        assert!(!sql.contains(r#""type""#), "the constant type column is gone: {sql}");
    }
}
