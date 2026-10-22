//! The create and replace statements of an entity subscription's
//! `subscriptions` row.
//!
//! A create returns the surrogate key it minted, because the delivery-state
//! row that completes the subscription is inserted under it in the same
//! transaction.
//!
//! `notification_trigger` is stored as the clause-5.3.3 registry codes the
//! notification queue is numbered from, not as their camel-case labels, so a
//! subscription's trigger set and a queued notification's trigger share one
//! numbering. `kind` and `status` are registry codes too, and the three
//! attribute lists are the dictionary keys [`InternedAttributeLists`] carries.

use crate::{
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
        (
            Subscriptions::DatasetId,
            Expr::val(fields.dataset_id.map(|selectors| selectors.iter().map(ToString::to_string).collect::<Vec<_>>())),
        ),
        (Subscriptions::Notification, json_document(fields.notification)?),
        (Subscriptions::ExpiresAt, Expr::val(fields.expires_at)),
        (Subscriptions::Throttling, Expr::val(fields.throttling)),
        (Subscriptions::Status, Expr::val(stored_status(status).code())),
        (Subscriptions::Lang, Expr::val(fields.lang.map(ToString::to_string))),
        (Subscriptions::JsonldContext, Expr::val(fields.jsonld_context.map(IriBuf::as_str))),
        (Subscriptions::LocalOnly, Expr::val(fields.local_only)),
        (Subscriptions::SplitEntities, Expr::val(fields.split_entities)),
        (Subscriptions::NgsildConformance, Expr::val(stored_conformance(fields.ngsild_conformance))),
        (Subscriptions::QWrittenMembers, Expr::val(fields.q.map(serialize_written_members))),
    ])
}

/// The entity-subscription insert of `id` with the values `fields` carries.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON or of an insert that cannot be assembled.
pub fn entity_create(
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
        .values(
            [Expr::val(id.as_str()), Expr::val(StoredKind::Entity.code().code())]
                .into_iter()
                .chain(values)
                .chain([now(), now()]),
        )
        .map_err(|error| persistence_error::<SubscriptionError>(error))?
        .returning(Query::returning().column(Subscriptions::SubscriptionKey));
    Ok(insert)
}

/// The entity-subscription replacement of `id` with the values `fields`
/// carries.
///
/// # Errors
///
/// Returns the persistence failure of a document that cannot be written as
/// JSON.
pub fn entity_update(
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
        .and_where(of_kind(StoredKind::Entity))
        .to_owned())
}

#[cfg(test)]
mod tests {
    use crate::subscription_store::{
        bare_fields_fixture::bare_fields,
        entity_statements::{entity_create, entity_update},
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
        let (sql, values) = entity_create(
            &ContextTarget::Live,
            &id(),
            &bare_fields(),
            InternedAttributeLists::default(),
            SubscriptionStatus::Active,
        )
        .expect("the entity insert builds")
        .build(PostgresQueryBuilder);
        (sql, values.0.len())
    }

    /// The replacement of a subscription carrying nothing optional, rendered.
    fn update_sql() -> (String, usize) {
        let (sql, values) = entity_update(
            &ContextTarget::Live,
            &id(),
            &bare_fields(),
            InternedAttributeLists::default(),
            SubscriptionStatus::Active,
        )
        .expect("the entity update builds")
        .build(PostgresQueryBuilder);
        (sql, values.0.len())
    }

    #[test]
    fn the_entity_create_carries_its_kind_code_and_binds_its_status_code() {
        let (sql, values) = create_sql();
        assert!(sql.contains("VALUES ($1, $2, $3"), "the identifier then the entity kind code: {sql}");
        assert!(!sql.contains("'Subscription'"), "the constant type token is no longer a column: {sql}");
        assert!(!sql.contains("::subscription_status"), "no enum cast remains: {sql}");
        assert!(sql.contains("NOW()"), "created_at and modified_at stamp the transaction clock: {sql}");
        assert_eq!(values, 26, "the create binds every column but the two timestamps: {sql}");
        assert!(!sql.contains("is_active"), "the redundant is_active column is gone: {sql}");
    }

    #[test]
    fn the_create_returns_the_surrogate_key_the_delivery_state_row_is_inserted_under() {
        let (sql, _) = create_sql();
        assert!(sql.ends_with(r#"RETURNING "subscription_key""#), "{sql}");
    }

    /// Table 5.2.12-1 opt-in lists, `ngsildConformance`, and the written
    /// members of `q` that clause 4.9 reads a target language and a raw JSON
    /// key in, carried by both statements.
    #[test]
    fn both_statements_carry_the_clause_5_2_12_members_and_the_written_members_of_q() {
        for (sql, _) in [create_sql(), update_sql()] {
            for column in ["expand_values", "json_keys", "ngsild_conformance", "q_written_members"] {
                assert!(sql.contains(column), "{column}: {sql}");
            }
        }
    }

    /// An entity subscription is no context source change, so neither
    /// statement takes the context source change clock.
    #[test]
    fn neither_statement_advances_the_context_source_change_clock() {
        for (sql, _) in [create_sql(), update_sql()] {
            assert!(!sql.contains("context_source_change_clock"), "{sql}");
            assert!(!sql.contains("context_source_revision"), "{sql}");
        }
    }

    #[test]
    fn every_subscription_document_is_bound_as_json() {
        let (create, _) = create_sql();
        let (update, _) = update_sql();
        assert_eq!(create.matches("::json").count(), 4, "{create}");
        assert_eq!(update.matches("::json").count(), 3, "{update}");
        assert!(!create.contains("::jsonb") && !update.contains("::jsonb"), "{create} {update}");
    }

    #[test]
    fn the_entity_update_pins_the_row_by_id_and_kind() {
        let (sql, _) = update_sql();
        assert!(sql.contains(r#""id" = $24"#), "{sql}");
        assert!(sql.contains(r#""subscriptions"."kind" = $25"#), "{sql}");
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
