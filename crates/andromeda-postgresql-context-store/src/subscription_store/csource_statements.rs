//! The create and replace statements of a context-source subscription's
//! `subscriptions` row.
//!
//! A create returns the surrogate key it minted, because the delivery-state
//! row that completes the subscription is inserted under it in the same
//! transaction. A context-source subscription stores no `dataset_id`, no
//! `local_only` and no `split_entities`.
//!
//! Both statements advance the tenant's context source change clock in a
//! `WITH` clause and stamp the revision drawn as `context_source_revision`:
//! that orders the write against every registration write (clauses 5.11.2.4
//! and 5.11.3.4). It is the statement's last lock, taken after the attribute
//! lists were interned.

use crate::{
    context_source_change_clock::{advance_clause, advanced_revision},
    store_fault::persistence_error,
    subscription_store::{
        conformance_text::stored_conformance,
        interned_attributes::InternedAttributeLists,
        json_document::json_document,
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
use andromeda_postgresql_seaquery::clock::now;
use andromeda_subscription::{error::SubscriptionError, store::SubscriptionFields};
use iri_rs::IriBuf;
use sea_query::{Expr, ExprTrait, InsertStatement, Query, SimpleExpr, UpdateStatement};

/// The columns a create and a replacement both write, each with the value it
/// takes.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON.
fn written_columns(
    fields: &SubscriptionFields<'_>,
    interned: InternedAttributeLists,
    status: SubscriptionStatus,
) -> Result<Vec<(Subscriptions, SimpleExpr)>, SubscriptionError> {
    Ok(vec![
        (Subscriptions::SubscriptionName, Expr::val(fields.subscription_name)),
        (Subscriptions::Description, Expr::val(fields.description)),
        (Subscriptions::Entities, json_document(fields.entities)?),
        (Subscriptions::WatchedAttributes, Expr::val(interned.watched_attributes)),
        (Subscriptions::NotificationTrigger, Expr::val(stored_trigger_list(fields.notification_trigger))),
        (Subscriptions::TimeInterval, Expr::val(fields.time_interval)),
        (Subscriptions::Q, Expr::val(fields.q.map(ToString::to_string))),
        (Subscriptions::GeoQ, json_document(fields.geo_q)?),
        (Subscriptions::ScopeQ, Expr::val(fields.scope_q.map(ToString::to_string))),
        (Subscriptions::Csf, Expr::val(fields.csf.map(ToString::to_string))),
        (Subscriptions::ExpandValues, Expr::val(interned.expand_values)),
        (Subscriptions::JsonKeys, Expr::val(interned.json_keys)),
        (Subscriptions::Notification, json_document(fields.notification)?),
        (Subscriptions::ExpiresAt, Expr::val(fields.expires_at)),
        (Subscriptions::Throttling, Expr::val(fields.throttling)),
        (Subscriptions::Status, Expr::val(stored_status(status).code())),
        (Subscriptions::Lang, Expr::val(fields.lang.map(ToString::to_string))),
        (Subscriptions::JsonldContext, Expr::val(fields.jsonld_context.map(IriBuf::as_str))),
        (Subscriptions::NgsildConformance, Expr::val(stored_conformance(fields.ngsild_conformance))),
        (Subscriptions::QWrittenMembers, Expr::val(fields.q.map(serialize_written_members))),
        (Subscriptions::ContextSourceRevision, advanced_revision()),
    ])
}

/// The context-source-subscription insert of `id` with the values `fields`
/// carries.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON or of an insert that cannot be assembled.
pub fn csource_create(
    target: &ContextTarget,
    id: &Identifier,
    fields: &SubscriptionFields<'_>,
    interned: InternedAttributeLists,
    status: SubscriptionStatus,
) -> Result<InsertStatement, SubscriptionError> {
    let mut columns = written_columns(fields, interned, status)?;
    columns.push((Subscriptions::TemporalQ, json_document(fields.temporal_q)?));
    let (names, values): (Vec<Subscriptions>, Vec<SimpleExpr>) = columns.into_iter().unzip();
    let mut insert = Query::insert();
    insert
        .into_table(table_of(target, Subscriptions::Table))
        .columns(
            [Subscriptions::Id, Subscriptions::Kind]
                .into_iter()
                .chain(names)
                .chain([Subscriptions::CreatedAt, Subscriptions::ModifiedAt]),
        )
        .with_cte(advance_clause())
        .values(
            [Expr::val(id.as_str()), Expr::val(StoredKind::Csource.code().code())]
                .into_iter()
                .chain(values)
                .chain([now(), now()]),
        )
        .map_err(|error| persistence_error::<SubscriptionError>(error))?
        .returning(Query::returning().column(Subscriptions::SubscriptionKey));
    Ok(insert)
}

/// The context-source-subscription replacement of `id` with the values
/// `fields` carries.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON.
pub fn csource_update(
    target: &ContextTarget,
    id: &Identifier,
    fields: &SubscriptionFields<'_>,
    interned: InternedAttributeLists,
    status: SubscriptionStatus,
) -> Result<UpdateStatement, SubscriptionError> {
    Ok(Query::update()
        .table(table_of(target, Subscriptions::Table))
        .values(
            written_columns(fields, interned, status)?
                .into_iter()
                .chain([(Subscriptions::ModifiedAt, now())]),
        )
        .and_where(Expr::col(Subscriptions::Id).eq(id.as_str()))
        .and_where(of_kind(StoredKind::Csource))
        .with_cte(advance_clause())
        .to_owned())
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::subscription_store::{
        bare_fields_fixture::bare_fields,
        csource_statements::{csource_create, csource_update},
        interned_attributes::InternedAttributeLists,
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_model::subscription::SubscriptionStatus;
    use pretty_assertions::assert_eq;
    use sea_query::PostgresQueryBuilder;

    fn id() -> Identifier {
        "urn:ngsi-ld:Subscription:s".parse().expect("a valid identifier")
    }

    /// The create of a subscription carrying nothing optional, rendered.
    fn create_sql() -> (String, usize) {
        let (sql, values) = csource_create(
            &ContextTarget::Live,
            &id(),
            &bare_fields(),
            InternedAttributeLists::default(),
            SubscriptionStatus::Active,
        )
        .expect("the context-source insert builds")
        .build(PostgresQueryBuilder);
        (sql, values.0.len())
    }

    /// The replacement of a subscription carrying nothing optional, rendered.
    fn update_sql() -> (String, usize) {
        let (sql, values) = csource_update(
            &ContextTarget::Live,
            &id(),
            &bare_fields(),
            InternedAttributeLists::default(),
            SubscriptionStatus::Active,
        )
        .expect("the context-source update builds")
        .build(PostgresQueryBuilder);
        (sql, values.0.len())
    }

    #[test]
    fn the_create_carries_its_kind_code_and_omits_the_dataset_and_local_only_columns() {
        let (sql, values) = create_sql();
        assert!(sql.contains("VALUES ($1, $2, $3"), "the identifier then the context-source kind code: {sql}");
        assert!(!sql.contains("::subscription_status"), "no enum cast remains: {sql}");
        assert_eq!(values, 23, "the create binds every column but the two timestamps and the drawn revision: {sql}");
        assert!(!sql.contains("dataset_id"), "a context-source subscription stores no datasetId: {sql}");
        assert!(!sql.contains("local_only"), "a context-source subscription stores no localOnly: {sql}");
    }

    #[test]
    fn the_create_returns_the_surrogate_key_the_delivery_state_row_is_inserted_under() {
        let (sql, _) = create_sql();
        assert!(sql.ends_with(r#"RETURNING "subscription_key""#), "{sql}");
    }

    /// Clause 5.11.2.3 makes the Subscription datatype the payload of a
    /// context-source subscription too, so both statements carry its opt-in
    /// lists, `ngsildConformance` and the written members of `q`.
    #[test]
    fn both_statements_carry_the_clause_5_2_12_members_and_the_written_members_of_q() {
        for (sql, _) in [create_sql(), update_sql()] {
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
        for (sql, _) in [create_sql(), update_sql()] {
            assert!(
                sql.starts_with(r#"WITH "advanced_clock" AS (UPDATE "context_source_change_clock" SET "revision" = "revision" + (1) RETURNING "revision")"#),
                "{sql}"
            );
            assert!(sql.contains(r#""context_source_revision""#), "{sql}");
            assert!(sql.contains(r#"(SELECT "revision" FROM "advanced_clock")"#), "{sql}");
        }
    }

    #[test]
    fn every_csource_subscription_document_is_bound_as_json() {
        let (create, _) = create_sql();
        let (update, _) = update_sql();
        assert_eq!(create.matches("::json").count(), 4, "{create}");
        assert_eq!(update.matches("::json").count(), 3, "{update}");
        assert!(!create.contains("::jsonb") && !update.contains("::jsonb"), "{create} {update}");
    }

    #[test]
    fn the_update_pins_the_row_by_id_and_kind() {
        let (sql, _) = update_sql();
        assert!(sql.contains(r#""id" = $21"#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."kind" = $22"#), "{sql}");
        assert!(sql.contains(r#""modified_at" = NOW()"#), "{sql}");
    }

    #[test]
    fn the_replacement_touches_no_delivery_counter_and_no_dropped_column() {
        let (sql, _) = update_sql();
        for counter in ["times_sent", "times_failed", "last_notification", "last_success", "last_failure"] {
            assert!(!sql.contains(counter), "a client update must not move {counter}: {sql}");
        }
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
        assert!(!sql.contains(r#""type""#), "the constant type column is gone: {sql}");
    }
}
