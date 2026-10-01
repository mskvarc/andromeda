//! The reads that let a deletion skip the payload it removes.
//!
//! The outline is read from the rows the write path keeps beside the payload:
//! the search row names the entity's types and scopes, and the current-state
//! projection holds one row per attribute instance with its `datasetId` and
//! family -- exactly what the payload's instances are, since every instance a
//! write admits is projected. The watcher probe asks the branch's subscription
//! relation, which every replica sharing the database reads alike.

use crate::{
    entity::{
        dictionary_join::{AttributeNameRow, join_attribute_name},
        entity_types::{Search, entity_types_statement},
        mapper::{stored_attribute_family, stored_attribute_name, stored_type_name, stored_uri},
        repo::EntityRepository,
    },
    error::ContextStoreError,
    subscription_store::trigger_code::stored_trigger,
};
use andromeda_context_commit::scope_presence::ScopePresence;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::deletion_outline::{DeletionOutline, DeletionWatchers};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    expanded::instance_key::AttributeInstanceKey,
    notification::{NotificationTrigger, effective_notification_triggers},
};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::{subscription_kind::SubscriptionKindCode, subscription_status::SubscriptionStatusCode};
use andromeda_postgresql_schema::{
    current::{CurrentAttributeValues, E, Entities, EntitySearch},
    name_dictionary::AttributeNames,
    subscriptions::Subscriptions,
};
use andromeda_postgresql_seaquery::{pg_array::has_elements, pg_operator::PgBinaryOperator};
use sea_query::{Expr, ExprTrait, Iden, JoinType, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::Row;
use strum::IntoEnumIterator;

/// Alias `cav` for the entity's projected instances.
#[derive(Iden, Clone, Copy)]
#[iden = "cav"]
struct Projected;

/// The column naming whether the entity held a Scope.
#[derive(Iden, Clone, Copy)]
#[iden = "scoped"]
struct Scoped;

/// The column answering the watcher probe.
#[derive(Iden, Clone, Copy)]
#[iden = "watched"]
struct Watched;

/// One row per type the entity carries, each naming whether it held a Scope:
/// the [`entity_types_statement`] with the column
/// `(cardinality(s.scope_paths) > 0) AS scoped` beside the name.
fn outline_types_statement(target: &ContextTarget, id: &str) -> SelectStatement {
    let mut statement = entity_types_statement(target, id);
    statement.expr_as(
        Expr::col((Search, EntitySearch::ScopePaths))
            .is_not_null()
            .and(has_elements(Expr::col((Search, EntitySearch::ScopePaths)))),
        Scoped,
    );
    statement
}

/// One row per attribute instance the entity holds:
///
/// ```sql
/// SELECT an.name, cav.dataset_id, cav.attribute_kind
/// FROM entities e JOIN current_attribute_values cav ON cav.entity_key = e.entity_key
/// JOIN attribute_names an ON an.attribute_name_key = cav.attribute_name_key
/// WHERE e.id = $1
/// ```
fn outline_instances_statement(target: &ContextTarget, id: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .column((AttributeNameRow, AttributeNames::Name))
        .column((Projected, CurrentAttributeValues::DatasetId))
        .column((Projected, CurrentAttributeValues::AttributeKind))
        .from_as(table_of(target, Entities::Table), E)
        .join_as(
            JoinType::InnerJoin,
            table_of(target, CurrentAttributeValues::Table),
            Projected,
            Expr::col((Projected, CurrentAttributeValues::EntityKey)).equals((E, Entities::EntityKey)),
        );
    join_attribute_name(
        &mut statement,
        JoinType::InnerJoin,
        Expr::col((Projected, CurrentAttributeValues::AttributeNameKey)),
    );
    statement.and_where(Expr::col((E, Entities::Id)).eq(id));
    statement
}

/// The stored codes of every trigger a subscription may list that a deletion
/// raises once clause 5.2.12 expands it: `entityDeleted`, `attributeDeleted`,
/// and `entityUpdated`, which Table 5.2.12-1 defines to include
/// `attributeDeleted`.
///
/// Read off [`effective_notification_triggers`], the expansion matching
/// itself applies, so the probe cannot drift from what a subscription is
/// matched by.
fn deletion_trigger_codes() -> Vec<i16> {
    NotificationTrigger::iter()
        .filter(|&listed| {
            effective_notification_triggers(Some(&[listed]))
                .iter()
                .any(|effective| matches!(effective, NotificationTrigger::EntityDeleted | NotificationTrigger::AttributeDeleted))
        })
        .map(|listed| stored_trigger(listed).code())
        .collect()
}

/// Whether any active, change-driven entity subscription listens for a
/// deletion:
///
/// ```sql
/// SELECT EXISTS (SELECT 1 FROM subscriptions WHERE kind = 1 AND status = 1
///   AND time_interval IS NULL AND notification_trigger && ARRAY[2, 3, 6]) AS watched
/// ```
///
/// A subscription with no explicit trigger selection listens for the clause
/// 5.2.12 default, `attributeCreated` and `attributeUpdated`, which a deletion
/// never raises; a periodic one is notified of current state, never of a
/// change. Types, identifiers and filters are not read: the answer may only be
/// coarser than the matching it guards.
fn deletion_watchers_statement(target: &ContextTarget) -> SelectStatement {
    let watchers = Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, Subscriptions::Table))
        .and_where(Expr::col(Subscriptions::Kind).eq(SubscriptionKindCode::Entity.code()))
        .and_where(Expr::col(Subscriptions::Status).eq(SubscriptionStatusCode::Active.code()))
        .and_where(Expr::col(Subscriptions::TimeInterval).is_null())
        .and_where(SimpleExpr::from(Expr::col(Subscriptions::NotificationTrigger)).array_overlaps(Expr::val(deletion_trigger_codes())))
        .take();
    Query::select().expr_as(Expr::exists(watchers), Watched).take()
}

impl EntityRepository {
    /// The outline of the entity `id` names, or `None` when there is none.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a read fails or a stored name, family
    /// or `datasetId` does not read back as its domain value.
    pub async fn deletion_outline(&self, target: &ContextTarget, id: &Identifier) -> Result<Option<DeletionOutline>, ContextStoreError> {
        let (sql, values) = outline_types_statement(target, id.as_str()).build_sqlx(PostgresQueryBuilder);
        let type_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let Some(first) = type_rows.first() else {
            return Ok(None);
        };
        let scope = if first.try_get::<bool, _>(Scoped.to_string().as_str())? {
            ScopePresence::Held
        } else {
            ScopePresence::Absent
        };
        let types = type_rows
            .iter()
            .map(|row| stored_type_name(row.try_get::<&str, _>(0)?))
            .collect::<Result<Vec<_>, ContextStoreError>>()?;

        let (sql, values) = outline_instances_statement(target, id.as_str()).build_sqlx(PostgresQueryBuilder);
        let instance_rows = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_all(&self.pool).await?;
        let instances = instance_rows
            .iter()
            .map(|row| {
                let key = AttributeInstanceKey {
                    attr: stored_attribute_name(row.try_get::<&str, _>(0)?)?,
                    dataset_id: row.try_get::<Option<&str>, _>(1)?.map(stored_uri).transpose()?,
                };
                Ok((key, stored_attribute_family(row.try_get::<i16, _>(2)?)?))
            })
            .collect::<Result<Vec<_>, ContextStoreError>>()?;
        Ok(Some(DeletionOutline { types, instances, scope }))
    }

    /// Whether any subscription a deletion in `target` can notify exists.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when the probe fails.
    pub async fn deletion_watchers(&self, target: &ContextTarget) -> Result<DeletionWatchers, ContextStoreError> {
        let (sql, values) = deletion_watchers_statement(target).build_sqlx(PostgresQueryBuilder);
        let watched: bool = sqlx::query_scalar_with(sqlx::AssertSqlSafe(sql.as_str()), values).fetch_one(&self.pool).await?;
        Ok(if watched { DeletionWatchers::Possible } else { DeletionWatchers::None })
    }
}

#[cfg(test)]
mod tests {
    use crate::entity::deletion_outline::{deletion_watchers_statement, outline_instances_statement, outline_types_statement};
    use andromeda_context_space::{context_target::ContextTarget, snapshot_key::SnapshotKey, snapshot_target::SnapshotTarget};
    use sea_query::PostgresQueryBuilder;

    /// The rendered text of the watcher probe, values inlined.
    fn probe(target: &ContextTarget) -> String {
        deletion_watchers_statement(target).to_string(PostgresQueryBuilder)
    }

    #[test]
    fn the_deletion_watcher_probe_renders_the_indexed_exists() {
        let sql = probe(&ContextTarget::Live);
        assert!(sql.starts_with(r#"SELECT EXISTS(SELECT 1 FROM "subscriptions""#), "{sql}");
        for clause in [
            r#""kind" = 1"#,
            r#""status" = 1"#,
            r#""time_interval" IS NULL"#,
            r#""notification_trigger" && ARRAY [2,3,6]"#,
        ] {
            assert!(sql.contains(clause), "{clause}: {sql}");
        }
    }

    /// A default-trigger subscription stores NULL triggers, which overlap
    /// nothing; a periodic one carries a `time_interval`; a paused one another
    /// status. The probe's three conditions exclude each.
    #[test]
    fn default_periodic_and_paused_subscriptions_are_no_deletion_watchers() {
        let sql = probe(&ContextTarget::Live);
        assert!(sql.contains(r#""notification_trigger" &&"#), "{sql}");
        assert!(sql.contains(r#""time_interval" IS NULL"#), "{sql}");
        assert!(sql.contains(r#""status" = 1"#), "{sql}");
    }

    #[test]
    fn the_probe_reads_the_branch_it_is_given() {
        let key = SnapshotKey::try_from(5).expect("a positive snapshot key");
        let target = ContextTarget::Snapshot(SnapshotTarget::published(key, "urn:ngsi-ld:Snapshot:five".parse().expect("a snapshot URI")));
        assert!(probe(&target).contains(r#""snap_5"."subscriptions""#), "{}", probe(&target));
    }

    #[test]
    fn the_outline_reads_types_scope_and_projected_instances_by_identifier() {
        let types = outline_types_statement(&ContextTarget::Live, "urn:ngsi-ld:T:1").to_string(PostgresQueryBuilder);
        assert!(types.contains(r#""n"."entity_type_name_key" = ANY("s"."entity_type_name_keys")"#), "{types}");
        assert!(types.contains(r#""e"."id" = 'urn:ngsi-ld:T:1'"#), "{types}");
        let instances = outline_instances_statement(&ContextTarget::Live, "urn:ngsi-ld:T:1").to_string(PostgresQueryBuilder);
        assert!(instances.contains(r#""current_attribute_values" AS "cav""#), "{instances}");
        assert!(instances.contains(r#""an"."attribute_name_key" = "cav"."attribute_name_key""#), "{instances}");
    }
}
