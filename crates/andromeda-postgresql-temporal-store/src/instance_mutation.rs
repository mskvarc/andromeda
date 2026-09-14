//! Mutation and retrieval of individual temporal attribute instances.
//!
//! Clause 5.6.15 addresses one instance of one temporal attribute by its
//! `instanceId`, and that identifier is stored in one of two columns: the bare
//! UUID of a broker-minted identifier, or the verbatim text of one a client
//! supplied. Every statement here is therefore written twice, once per column,
//! so each reads the partial index that covers it instead of an `OR` the
//! planner has to resolve with a scan.

use crate::{
    attribute_instance::{collect_metadata, detect_attribute_type, extract_observed_at},
    attribute_kind_code::stored_attribute_kind,
    attribute_value::{AttributeValue, HistoryValueColumns, extract_attribute_value},
    error::TemporalStoreError,
    repo::TemporalRepository,
    unit_code_metadata::take_unit_code,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{
    attribute_name::AttributeName,
    attribute_type::AttributeType,
    instance_family::InstanceFamily,
    instance_identity::InstanceIdentity,
    uri::Uri,
};
use andromeda_postgresql_branch::table_of;
use andromeda_postgresql_codes::value_kind::ValueKind;
use andromeda_postgresql_encoding::ewkb_binding::geometry_bytes;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, AttributeHistoryInstanceClaims, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{clock::now, placeholder::external_placeholder, postgis::validated_geom_from_ewkb};
use andromeda_unit_catalog::unit_code_id::UnitCodeId;
use chrono::{DateTime, Utc};
use sea_query::{CaseStatement, Expr, ExprTrait, Iden, JoinType, Keyword, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use serde_json::Value;
use sqlx::{AssertSqlSafe, postgres::PgRow};

/// The `temporal_attribute_streams` alias in the stream-scope subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "s"]
struct S;

/// The `temporal_entities` alias in the stream-scope subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "te"]
struct Te;

/// The `attribute_names` alias in the stream-scope subquery.
#[derive(Iden, Clone, Copy)]
#[iden = "an"]
struct An;

/// The `attribute_history` alias the single-instance select reads under.
#[derive(Iden, Clone, Copy)]
#[iden = "ah"]
struct Ah;

impl TemporalRepository {
    /// Replace one expanded attribute instance through the typed PostgreSQL update path.
    pub(crate) async fn update_attribute_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        expanded: &Value,
    ) -> Result<(), TemporalStoreError> {
        let state = build_update_instance_state(expanded)?;
        self.update_instance(target, entity_id, attribute_name, instance, &state).await
    }

    /// Physically remove one named attribute instance from history.
    ///
    /// Clause 5.6.15 deletes the addressed instance itself rather than
    /// recording that it ended, so the row is removed and no tombstone is
    /// written: a tombstone would keep answering temporal queries for an
    /// instance the client asked to have removed.
    ///
    /// Deliberate retention asymmetry: unlike whole-entity temporal deletion,
    /// this single-instance path does not read the removed row's `write_id`
    /// and does not release the write record it belongs to. Every other row
    /// of that write (other instances, other attributes, current state,
    /// notifications, side effects) may still exist, so this statement cannot
    /// know from a single deleted row whether the write record it belonged to
    /// has become unreferenced. Leaving the write record retained is the safe
    /// direction: retention's own periodic sweep is what actually knows the
    /// full set of referents and reaps a write record once none of them
    /// remain, so retaining here costs one write record's worth of storage
    /// until that sweep runs, whereas guessing wrong and releasing early would
    /// corrupt replay identity for every sibling row still pointing at it.
    pub(crate) async fn delete_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
    ) -> Result<(), TemporalStoreError> {
        let mut tx = self.pool.begin().await?;
        let generated_sql = delete_instance_sql(InstanceColumn::Generated, target);
        let verbatim_sql = delete_instance_sql(InstanceColumn::Verbatim, target);
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(AssertSqlSafe(generated_sql)).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(AssertSqlSafe(verbatim_sql)).bind(identifier.as_str()),
        };
        let deleted = statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .execute(&mut *tx)
            .await?
            .rows_affected();
        if deleted == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        // A broker-minted identifier never claims a row in the first place
        // (`duplicate_instance_claim`), so only a verbatim one has a claim to
        // release. Releasing it here is what lets a client reuse the
        // `instanceId` of an instance it has just deleted.
        if let InstanceIdentity::Verbatim(identifier) = instance {
            let release_sql = release_verbatim_instance_claim_sql(target);
            sqlx::query(AssertSqlSafe(release_sql))
                .bind(identifier.as_str())
                .bind(entity_id.as_str())
                .bind(attribute_name.as_str())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Modify one named attribute instance in place.
    async fn update_instance(
        &self,
        target: &ContextTarget,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        instance: &InstanceIdentity,
        state: &UpdateInstanceState,
    ) -> Result<(), TemporalStoreError> {
        let generated_sql = update_instance_sql(InstanceColumn::Generated, target);
        let verbatim_sql = update_instance_sql(InstanceColumn::Verbatim, target);
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(AssertSqlSafe(generated_sql)).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(AssertSqlSafe(verbatim_sql)).bind(identifier.as_str()),
        };
        let updated = statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .bind(state.value_kind.code())
            .bind(state.columns.value_number.as_ref())
            .bind(state.columns.value_string.as_deref())
            .bind(state.columns.value_bool)
            .bind(geometry_bytes(state.columns.value_geo.as_ref())?)
            .bind(state.columns.value_object.as_ref())
            .bind(state.columns.value_datetime)
            .bind(state.columns.value_uri.as_ref().map(Uri::as_str))
            .bind(state.observed_at)
            .bind(state.metadata.as_ref())
            .bind(state.unit_code_id.map(UnitCodeId::get))
            .bind(stored_attribute_kind(state.attribute_type).code())
            .execute(&self.pool)
            .await?
            .rows_affected();
        if updated == 0 {
            return Err(TemporalStoreError::NotFound(instance_not_found(instance)));
        }
        Ok(())
    }

    /// Fetch a single attribute instance for policy validation.
    pub(crate) async fn get_instance(
        &self,
        target: &ContextTarget,
        instance: &InstanceIdentity,
        entity_id: &Uri,
        attribute_name: &AttributeName,
    ) -> Result<Option<PgRow>, TemporalStoreError> {
        let generated_sql = select_instance_sql(InstanceColumn::Generated, target);
        let verbatim_sql = select_instance_sql(InstanceColumn::Verbatim, target);
        let statement = match instance {
            InstanceIdentity::Generated(uuid) => sqlx::query(AssertSqlSafe(generated_sql)).bind(uuid),
            InstanceIdentity::Verbatim(identifier) => sqlx::query(AssertSqlSafe(verbatim_sql)).bind(identifier.as_str()),
        };
        Ok(statement
            .bind(entity_id.as_str())
            .bind(attribute_name.as_str())
            .fetch_optional(&self.pool)
            .await?)
    }
}

/// Report an instance the addressed attribute does not hold.
fn instance_not_found(instance: &InstanceIdentity) -> String {
    format!("Attribute instance {} not found", instance.rendered(InstanceFamily::Attribute))
}

/// Owned update values kept alive while PostgreSQL binds borrow them.
struct UpdateInstanceState {
    /// Typed history columns.
    columns: HistoryValueColumns,
    /// Expanded instance metadata, without the `unitCode` member.
    metadata: Option<Value>,
    /// Catalog identity of the instance's `unitCode`, when it declares one.
    unit_code_id: Option<UnitCodeId>,
    /// The observation instant the modified instance declares, if any.
    observed_at: Option<DateTime<Utc>>,
    /// The value shape the stored row declares, which the history table
    /// constrains to agree with the column the value lands in.
    value_kind: ValueKind,
    /// The attribute family detected from the replacement instance.
    ///
    /// Clause 5.6.14 defines instance PATCH as replacement by the supplied
    /// complete fragment, so a replacement that changes family must move the
    /// stored `attribute_kind` along with the value columns: retaining it
    /// here is what lets the update statement keep the two in agreement
    /// rather than rendering the old family over the new value shape.
    attribute_type: AttributeType,
}

/// Decode one expanded instance into owned PostgreSQL update state.
fn build_update_instance_state(expanded: &Value) -> Result<UpdateInstanceState, TemporalStoreError> {
    let attribute_type = detect_attribute_type(expanded);
    let attribute_value = extract_attribute_value(expanded, attribute_type)?;
    let value_kind = value_kind_of(&attribute_value);
    let observed_at = extract_observed_at(expanded)?;
    let mut metadata = collect_metadata(expanded);
    let unit_code_id = take_unit_code(&mut metadata)?;
    Ok(UpdateInstanceState {
        columns: attribute_value.into_columns(),
        metadata,
        unit_code_id,
        observed_at,
        value_kind,
        attribute_type,
    })
}

/// The stored value shape of a decoded instance value.
///
/// A modified instance always carries a value, so no input to this mapping is a
/// [`ValueKind::Tombstone`]: that member is reserved for a row recording a
/// deletion (clauses 4.5.7 and 4.5.8), which clause 5.6.15 cannot produce.
const fn value_kind_of(value: &AttributeValue) -> ValueKind {
    match value {
        AttributeValue::Number(_) => ValueKind::Number,
        AttributeValue::String(_) => ValueKind::String,
        AttributeValue::Boolean(_) => ValueKind::Boolean,
        AttributeValue::Geometry(_) => ValueKind::Geometry,
        AttributeValue::Object(_) => ValueKind::Object,
        AttributeValue::DateTime(_) => ValueKind::DateTime,
        AttributeValue::Uri(_) => ValueKind::Uri,
    }
}

/// Which stored column carries the addressed `instanceId`.
///
/// Clause 5.6.15's identifier lives in exactly one of two columns, and every
/// statement addresses the one that matches so it reads the partial index that
/// covers it rather than resolving an `OR` with a scan.
#[derive(Clone, Copy)]
enum InstanceColumn {
    /// A broker-minted `instanceId`, stored as a bare UUID.
    Generated,
    /// A client-supplied `instanceId`, stored verbatim.
    Verbatim,
}

impl InstanceColumn {
    /// The `attribute_history` column this identity is addressed through.
    const fn column(self) -> AttributeHistory {
        match self {
            InstanceColumn::Generated => AttributeHistory::GeneratedInstanceUuid,
            InstanceColumn::Verbatim => AttributeHistory::VerbatimInstanceId,
        }
    }
}

/// The streams of one entity's attribute, whatever `datasetId` they carry.
///
/// An `instanceId` is unique across the attribute, so the dataset dimension is
/// not part of the address clause 5.6.15 defines; every stream of the named
/// attribute is a candidate and the instance predicate selects within them. The
/// entity id (`$2`) and attribute name (`$3`) are bound by the caller's chain.
fn attribute_streams_subquery(target: &ContextTarget) -> SelectStatement {
    Query::select()
        .column((S, TemporalAttributeStreams::StreamKey))
        .from(table_of(target, TemporalAttributeStreams::Table).alias(S))
        .join_as(
            JoinType::InnerJoin,
            table_of(target, TemporalEntities::Table),
            Te,
            Expr::col((Te, TemporalEntities::EntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .join_as(
            JoinType::InnerJoin,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::Id)).equals((S, TemporalAttributeStreams::AttributeNameId)),
        )
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(external_placeholder(2)))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(external_placeholder(3)))
        .to_owned()
}

/// The instance delete, removing the addressed row rather than tombstoning it.
fn delete_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::delete()
        .from_table(table_of(target, AttributeHistory::Table))
        .and_where(Expr::col(column.column()).eq(external_placeholder(1)))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(attribute_streams_subquery(target)))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// Free the `(entity, attribute, verbatim instanceId)` claim a deleted
/// instance held, so a future write may reuse the identifier (clause 4.5.7
/// does not reserve a deleted instance's identity forever).
///
/// `DELETE … USING` is expressed as two correlated `EXISTS`, since sea-query has
/// no `USING`: each resolves one of the scope tables the join did.
fn release_verbatim_instance_claim_sql(target: &ContextTarget) -> String {
    let entity_scope = Query::select()
        .expr(Expr::val(1))
        .from(table_of(target, TemporalEntities::Table).alias(Te))
        .and_where(
            Expr::col((Te, TemporalEntities::EntityKey)).equals((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::TemporalEntityKey)),
        )
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(external_placeholder(2)))
        .to_owned();
    let attribute_scope = Query::select()
        .expr(Expr::val(1))
        .from_as(AttributeNames::Table, An)
        .and_where(Expr::col((An, AttributeNames::Id)).equals((AttributeHistoryInstanceClaims::Table, AttributeHistoryInstanceClaims::AttributeNameId)))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(external_placeholder(3)))
        .to_owned();
    Query::delete()
        .from_table(table_of(target, AttributeHistoryInstanceClaims::Table))
        .and_where(Expr::col(AttributeHistoryInstanceClaims::VerbatimInstanceId).eq(external_placeholder(1)))
        .and_where(Expr::exists(entity_scope))
        .and_where(Expr::exists(attribute_scope))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// The geometry column's `CASE WHEN $8 IS NOT NULL THEN ST_MakeValid(ST_GeomFromEWKB($8)) ELSE NULL END`.
fn geometry_write(marker: usize) -> SimpleExpr {
    CaseStatement::new()
        .case(
            Expr::expr(external_placeholder(marker)).is_not_null(),
            validated_geom_from_ewkb(external_placeholder(marker)),
        )
        .finally(Keyword::Null)
        .into()
}

/// The instance modification (clause 5.6.14 replacement).
///
/// `$5` binds directly as the column's arbitrary-precision type, so a number
/// with no exact double is stored exactly as a creation stores it. `modified_at`
/// advances the NGSI-LD axis while `recorded_at` is left alone.
/// `attribute_kind` moves with the value columns so a replacement that changes
/// family keeps the row's declared family in agreement with its value shape.
fn update_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::update()
        .table(table_of(target, AttributeHistory::Table))
        .values([
            (AttributeHistory::ValueKind, external_placeholder(4)),
            (AttributeHistory::ValueNumber, external_placeholder(5)),
            (AttributeHistory::ValueString, external_placeholder(6)),
            (AttributeHistory::ValueBool, external_placeholder(7)),
            (AttributeHistory::ValueGeo, geometry_write(8)),
            (AttributeHistory::ValueObject, external_placeholder(9)),
            (AttributeHistory::ValueDatetime, external_placeholder(10)),
            (AttributeHistory::ValueUri, external_placeholder(11)),
            (AttributeHistory::ObservedAt, external_placeholder(12)),
            (AttributeHistory::Metadata, external_placeholder(13)),
            (AttributeHistory::UnitCodeId, external_placeholder(14)),
            (AttributeHistory::AttributeKind, external_placeholder(15)),
            (AttributeHistory::ModifiedAt, now()),
        ])
        .and_where(Expr::col(column.column()).eq(external_placeholder(1)))
        .and_where(Expr::col(AttributeHistory::StreamKey).in_subquery(attribute_streams_subquery(target)))
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

/// The single-instance select used for policy validation.
fn select_instance_sql(column: InstanceColumn, target: &ContextTarget) -> String {
    Query::select()
        .column((Ah, AttributeHistory::RecordedAt))
        .column((Ah, AttributeHistory::WriteId))
        .column((Ah, AttributeHistory::WriteOrdinal))
        .column((Ah, AttributeHistory::StreamKey))
        .column((Ah, AttributeHistory::AttributeKind))
        .column((Ah, AttributeHistory::ValueKind))
        .column((Ah, AttributeHistory::ObservedAt))
        .column((Ah, AttributeHistory::DeletedAt))
        .from(table_of(target, AttributeHistory::Table).alias(Ah))
        .and_where(Expr::col((Ah, column.column())).eq(external_placeholder(1)))
        .and_where(Expr::col((Ah, AttributeHistory::StreamKey)).in_subquery(attribute_streams_subquery(target)))
        .limit(1)
        .to_owned()
        .to_string(PostgresQueryBuilder)
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_value::AttributeValue,
        instance_mutation::{
            InstanceColumn,
            delete_instance_sql,
            release_verbatim_instance_claim_sql,
            select_instance_sql,
            update_instance_sql,
            value_kind_of,
        },
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_geometry::geometry::Geometry;
    use andromeda_ngsi_ld_number::JsonNumber;
    use andromeda_postgresql_codes::value_kind::ValueKind;
    use pretty_assertions::assert_eq;

    /// The six identity-column statements, rendered.
    fn all_statements() -> Vec<String> {
        [InstanceColumn::Generated, InstanceColumn::Verbatim]
            .into_iter()
            .flat_map(|column| {
                [
                    delete_instance_sql(column, &ContextTarget::Live),
                    update_instance_sql(column, &ContextTarget::Live),
                    select_instance_sql(column, &ContextTarget::Live),
                ]
            })
            .collect()
    }

    #[test]
    fn each_statement_addresses_exactly_one_instance_identity_column() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            for statement in [
                delete_instance_sql(column, &ContextTarget::Live),
                update_instance_sql(column, &ContextTarget::Live),
                select_instance_sql(column, &ContextTarget::Live),
            ] {
                match column {
                    InstanceColumn::Generated => {
                        assert!(statement.contains(r#""generated_instance_uuid" = ($1)"#), "{statement}");
                        assert!(!statement.contains("verbatim_instance_id"), "{statement}");
                    }
                    InstanceColumn::Verbatim => {
                        assert!(statement.contains(r#""verbatim_instance_id" = ($1)"#), "{statement}");
                        assert!(!statement.contains("generated_instance_uuid"), "{statement}");
                    }
                }
            }
        }
    }

    #[test]
    fn every_statement_is_confined_to_the_named_entity_and_attribute() {
        for statement in all_statements() {
            assert!(statement.contains(r#"FROM "temporal_attribute_streams" AS "s""#), "{statement}");
            assert!(statement.contains(r#""te"."id" = ($2)"#), "{statement}");
            assert!(statement.contains(r#""an"."name" = ($3)"#), "{statement}");
        }
    }

    #[test]
    fn a_modification_advances_the_modification_axis_and_leaves_the_write_clock_alone() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = update_instance_sql(column, &ContextTarget::Live);
            assert!(statement.contains(r#""modified_at" = NOW()"#), "{statement}");
            assert!(!statement.contains(r#""recorded_at" ="#), "{statement}");
            assert!(statement.contains(r#""value_kind" = $4"#), "{statement}");
            assert!(statement.contains(r#""attribute_kind" = $15"#), "{statement}");
        }
    }

    #[test]
    fn a_deletion_removes_the_row_rather_than_recording_a_tombstone() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = delete_instance_sql(column, &ContextTarget::Live);
            assert!(statement.starts_with(r#"DELETE FROM "attribute_history""#), "{statement}");
            assert!(!statement.contains("deleted_at"), "{statement}");
        }
    }

    #[test]
    fn a_verbatim_deletion_releases_the_instance_claim_it_held() {
        let statement = release_verbatim_instance_claim_sql(&ContextTarget::Live);
        assert!(statement.starts_with(r#"DELETE FROM "attribute_history_instance_claims""#), "{statement}");
        assert!(statement.contains(r#""verbatim_instance_id" = ($1)"#), "{statement}");
        assert!(statement.contains(r#""te"."id" = ($2)"#), "{statement}");
        assert!(statement.contains(r#""an"."name" = ($3)"#), "{statement}");
        assert_eq!(statement.matches("EXISTS").count(), 2, "{statement}");
    }

    #[test]
    fn the_update_statement_binds_numeric_without_a_double_hop() {
        for column in [InstanceColumn::Generated, InstanceColumn::Verbatim] {
            let statement = update_instance_sql(column, &ContextTarget::Live);
            assert!(statement.contains(r#""value_number" = $5"#), "{statement}");
            assert!(!statement.contains("double precision"), "{statement}");
        }
    }

    #[test]
    fn every_decoded_value_shape_maps_to_its_own_stored_kind() {
        let geometry = Geometry::from_geojson_value(&serde_json::json!({"type": "Point", "coordinates": [1.0, 2.0]}))
            .map(|geometry| value_kind_of(&AttributeValue::Geometry(geometry)))
            .map_err(|error| error.to_string());
        assert_eq!(geometry, Ok(ValueKind::Geometry));
        let kinds = [
            value_kind_of(&AttributeValue::Number(JsonNumber::from(1))),
            value_kind_of(&AttributeValue::String("text".to_owned())),
            value_kind_of(&AttributeValue::Boolean(true)),
            value_kind_of(&AttributeValue::Object(serde_json::json!({}))),
        ];
        assert_eq!(kinds, [ValueKind::Number, ValueKind::String, ValueKind::Boolean, ValueKind::Object]);
        assert!(!kinds.contains(&ValueKind::Tombstone));
    }
}
