//! Reading and pruning the temporal catalogue.
//!
//! Every question the temporal mutation policy asks before it acts — does this
//! entity have a Temporal Evolution, does it record this attribute, which
//! datasets does that attribute have — is answered from `temporal_entities` and
//! `temporal_attribute_streams` alone. Current state is never consulted: clause
//! 5.6.6 leaves the Temporal Evolution standing when the entity is deleted, so
//! an answer taken from `entities` would report history that exists as absent.

use crate::{error::TemporalStoreError, repo::TemporalRepository, write_record_release::release_named};
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, uri::Uri};
use andromeda_postgresql_query::entity_type_names::type_names_of;
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::pg_array::{any, text_array};
use andromeda_postgresql_temporal_query::alias::{An, S, Te};
use sea_query::{Alias, DeleteStatement, Expr, ExprTrait, JoinType, LockType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Row};
use std::collections::HashMap;
use uuid::Uuid;

/// Which dataset instances of one attribute a deletion addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeStreamScope {
    /// Every dataset instance of the attribute.
    All,
    /// The one named dataset instance, or the default instance when none is named.
    Single,
}

impl TemporalRepository {
    /// Whether the entity has a Temporal Evolution at all.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn temporal_entity_exists(&self, entity_id: &Uri) -> Result<bool, TemporalStoreError> {
        let (sql, values) = temporal_entity_exists_query(entity_id.as_str()).build_sqlx(PostgresQueryBuilder);
        let exists = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        Ok(exists)
    }

    /// The entity types the catalogue records for each named Temporal Evolution.
    ///
    /// The catalogue is the authority for them (clause 4.5.1 makes `type`
    /// mandatory in every representation, and clause 5.6.6 leaves the Temporal
    /// Evolution standing when current state is deleted), so an entity absent
    /// from the catalogue is absent from the result rather than typed empty.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::Decode`] when a stored identifier or type name is
    /// not the validated shape the catalogue writes.
    pub(crate) async fn temporal_entity_types(&self, entity_ids: &[Uri]) -> Result<HashMap<Uri, Vec<EntityTypeName>>, TemporalStoreError> {
        if entity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let identifiers: Vec<&str> = entity_ids.iter().map(Uri::as_str).collect();
        let (sql, values) = temporal_entity_types_query(&identifiers).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        let mut types_by_entity = HashMap::with_capacity(rows.len());
        for row in rows {
            let raw_id: String = row
                .try_get("id")
                .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.id: {error}")))?;
            let entity_id = raw_id
                .parse::<Uri>()
                .map_err(|error| TemporalStoreError::Decode(format!("temporal_entities.id '{raw_id}': {error}")))?;
            let raw_types: Vec<String> = row
                .try_get("types")
                .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_entities.type_ids names: {error}")))?;
            let types = raw_types
                .into_iter()
                .map(|raw| {
                    raw.parse::<EntityTypeName>()
                        .map_err(|error| TemporalStoreError::Decode(format!("temporal_entities.type_ids name '{raw}': {error}")))
                })
                .collect::<Result<Vec<EntityTypeName>, TemporalStoreError>>()?;
            types_by_entity.insert(entity_id, types);
        }
        Ok(types_by_entity)
    }

    /// Whether the entity records any stream for the named attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn attribute_stream_exists(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<bool, TemporalStoreError> {
        let (sql, values) = attribute_stream_exists_query(entity_id.as_str(), attribute_name.as_str()).build_sqlx(PostgresQueryBuilder);
        let exists = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        Ok(exists)
    }

    /// The `datasetId` of every stream the entity records for one attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::Decode`] when a stored discriminator is not a URI.
    pub(crate) async fn attribute_dataset_ids(&self, entity_id: &Uri, attribute_name: &AttributeName) -> Result<Vec<Option<Uri>>, TemporalStoreError> {
        let (sql, values) = attribute_dataset_ids_query(entity_id.as_str(), attribute_name.as_str()).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                row.try_get::<Option<String>, _>("dataset_id")
                    .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id: {error}")))?
                    .map(|raw| {
                        raw.parse::<Uri>()
                            .map_err(|error| TemporalStoreError::Decode(format!("invalid temporal_attribute_streams.dataset_id '{raw}': {error}")))
                    })
                    .transpose()
            })
            .collect()
    }

    /// Every expanded attribute IRI the entity records a stream for.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::Decode`] when a stored name is not an attribute IRI.
    pub(crate) async fn temporal_attribute_names(&self, entity_id: &Uri) -> Result<Vec<AttributeName>, TemporalStoreError> {
        let (sql, values) = temporal_attribute_names_query(entity_id.as_str()).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("attribute_name")?;
                raw.parse::<AttributeName>()
                    .map_err(|error| TemporalStoreError::Decode(format!("Invalid attribute_names.name '{raw}': {error}")))
            })
            .collect()
    }

    /// Remove the addressed attribute streams and every observation they hold.
    ///
    /// Clause 5.6.17 deletes the Temporal Evolution of one attribute, so the
    /// history goes with the stream rather than being tombstoned; nothing in the
    /// database sequences that, so the order is stated here inside one
    /// transaction, and the idempotency records the removed rows were the last
    /// protected effect of are released with them.
    ///
    /// Returns how many streams the deletion removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails and
    /// [`TemporalStoreError::Decode`] when a removed row's write identity does
    /// not carry the expected type.
    pub(crate) async fn delete_attribute_streams(
        &self,
        entity_id: &Uri,
        attribute_name: &AttributeName,
        dataset_id: Option<&Uri>,
        scope: AttributeStreamScope,
    ) -> Result<u64, TemporalStoreError> {
        let dataset_selector = match (scope, dataset_id) {
            (AttributeStreamScope::All, _) => DatasetSelector::AllStreams,
            (AttributeStreamScope::Single, Some(dataset_id)) => DatasetSelector::Named(dataset_id.as_str()),
            (AttributeStreamScope::Single, None) => DatasetSelector::Default,
        };
        let (select_sql, select_values) =
            select_attribute_streams_query(entity_id.as_str(), attribute_name.as_str(), dataset_selector).build_sqlx(PostgresQueryBuilder);
        let mut tx = self.pool.begin().await?;
        let stream_keys: Vec<i64> = sqlx::query_scalar_with(AssertSqlSafe(select_sql), select_values).fetch_all(&mut *tx).await?;
        if stream_keys.is_empty() {
            tx.rollback().await?;
            return Ok(0);
        }
        let (history_sql, history_values) = delete_stream_history_query(&stream_keys).build_sqlx(PostgresQueryBuilder);
        let removed = sqlx::query_with(AssertSqlSafe(history_sql), history_values).fetch_all(&mut *tx).await?;
        let touched_writes = removed
            .iter()
            .map(|row| {
                row.try_get::<Uuid, _>("write_id")
                    .map_err(|error| TemporalStoreError::Decode(format!("invalid attribute_history.write_id: {error}")))
            })
            .collect::<Result<Vec<Uuid>, _>>()?;
        let (streams_sql, streams_values) = delete_streams_query(&stream_keys).build_sqlx(PostgresQueryBuilder);
        let streams = sqlx::query_with(AssertSqlSafe(streams_sql), streams_values)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        release_named(&mut tx, &touched_writes).await?;
        tx.commit().await?;
        Ok(streams)
    }
}

/// Which dataset instances of one attribute a stream selection addresses.
#[derive(Clone, Copy)]
enum DatasetSelector<'a> {
    /// Every stream of the attribute.
    AllStreams,
    /// The one named dataset instance.
    Named(&'a str),
    /// The default instance, whose `datasetId` is absent.
    Default,
}

/// A bound `bigint[]` value from a batch of stream keys.
///
/// The array value owns its elements, so the slice is copied: a bound parameter
/// cannot borrow from the caller's slice across the round trip.
fn stream_key_array(keys: &[i64]) -> sea_query::SimpleExpr {
    Expr::val(keys.to_vec())
}

/// Join a stream to its temporal entity and its attribute name.
///
/// Every catalogue read reaches the entity URI and the attribute IRI through
/// this join rather than through `entities`, so a current-state deletion (clause
/// 5.6.6) leaves the catalogue answering.
fn stream_catalog_join(statement: &mut SelectStatement) {
    statement
        .from_as(TemporalAttributeStreams::Table, S)
        .join_as(
            JoinType::Join,
            TemporalEntities::Table,
            Te,
            Expr::col((Te, TemporalEntities::EntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::Id)).equals((S, TemporalAttributeStreams::AttributeNameId)),
        );
}

/// Whether the entity has a Temporal Evolution at all.
fn temporal_entity_exists_query(entity_id: &str) -> SelectStatement {
    let mut inner = Query::select();
    inner
        .expr(Expr::val(1))
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::Id).eq(entity_id));
    let mut statement = Query::select();
    statement.expr(Expr::exists(inner));
    statement
}

/// The identifier and type IRIs of every entity in `ids`, the IRIs read back
/// through the type dictionary in stored order under the `types` column.
fn temporal_entity_types_query(ids: &[&str]) -> SelectStatement {
    Query::select()
        .column(TemporalEntities::Id)
        .expr_as(type_names_of(Expr::col(TemporalEntities::TypeIds)), Alias::new("types"))
        .from(TemporalEntities::Table)
        .and_where(Expr::col(TemporalEntities::Id).eq(any(text_array(ids))))
        .to_owned()
}

/// Whether the entity records any stream for the named attribute.
fn attribute_stream_exists_query(entity_id: &str, attribute_name: &str) -> SelectStatement {
    let mut inner = Query::select();
    inner.expr(Expr::val(1));
    stream_catalog_join(&mut inner);
    inner
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(attribute_name));
    let mut statement = Query::select();
    statement.expr(Expr::exists(inner));
    statement
}

/// The `datasetId` of every stream the entity records for one attribute.
fn attribute_dataset_ids_query(entity_id: &str, attribute_name: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((S, TemporalAttributeStreams::DatasetId));
    stream_catalog_join(&mut statement);
    statement
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(attribute_name));
    statement
}

/// Every distinct attribute IRI the entity records a stream for.
fn temporal_attribute_names_query(entity_id: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .distinct()
        .expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    stream_catalog_join(&mut statement);
    statement.and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id));
    statement
}

/// The identifiers of the streams a deletion addresses, locked for update.
///
/// The default instance is addressed by the absence of a `datasetId`, never by
/// a sentinel value, so a stream with no dataset is matched by `IS NULL`.
fn select_attribute_streams_query(entity_id: &str, attribute_name: &str, dataset: DatasetSelector<'_>) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((S, TemporalAttributeStreams::StreamKey));
    stream_catalog_join(&mut statement);
    statement
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(attribute_name));
    match dataset {
        DatasetSelector::AllStreams => {}
        DatasetSelector::Named(dataset_id) => {
            statement.and_where(Expr::col((S, TemporalAttributeStreams::DatasetId)).eq(dataset_id));
        }
        DatasetSelector::Default => {
            statement.and_where(Expr::col((S, TemporalAttributeStreams::DatasetId)).is_null());
        }
    }
    statement.lock_with_tables(LockType::Update, [S]);
    statement
}

/// Remove every observation of the addressed streams, returning the write
/// identity each removed row belonged to.
fn delete_stream_history_query(stream_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(AttributeHistory::Table)
        .and_where(Expr::col(AttributeHistory::StreamKey).eq(any(stream_key_array(stream_keys))))
        .returning_col(AttributeHistory::WriteId)
        .to_owned()
}

/// Remove the addressed streams themselves.
fn delete_streams_query(stream_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(TemporalAttributeStreams::Table)
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(any(stream_key_array(stream_keys))))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::temporal_catalog_read::{
        DatasetSelector,
        attribute_dataset_ids_query,
        attribute_stream_exists_query,
        delete_stream_history_query,
        delete_streams_query,
        select_attribute_streams_query,
        temporal_attribute_names_query,
        temporal_entity_exists_query,
    };
    use sea_query::PostgresQueryBuilder;

    fn read_sql() -> Vec<String> {
        vec![
            temporal_entity_exists_query("urn:x").to_string(PostgresQueryBuilder),
            attribute_stream_exists_query("urn:x", "https://ex/speed").to_string(PostgresQueryBuilder),
            attribute_dataset_ids_query("urn:x", "https://ex/speed").to_string(PostgresQueryBuilder),
            temporal_attribute_names_query("urn:x").to_string(PostgresQueryBuilder),
            select_attribute_streams_query("urn:x", "https://ex/speed", DatasetSelector::AllStreams).to_string(PostgresQueryBuilder),
        ]
    }

    #[test]
    fn no_catalogue_question_is_answered_from_current_state() {
        for statement in read_sql() {
            assert!(!statement.contains("\"entities\""), "{statement}");
            assert!(!statement.contains("temporal_entity_attributes"), "{statement}");
        }
    }

    #[test]
    fn the_default_instance_is_addressed_by_absence_rather_than_by_a_value() {
        let default = select_attribute_streams_query("urn:x", "a", DatasetSelector::Default).to_string(PostgresQueryBuilder);
        let named = select_attribute_streams_query("urn:x", "a", DatasetSelector::Named("urn:d")).to_string(PostgresQueryBuilder);
        assert!(default.contains("\"dataset_id\" IS NULL"), "{default}");
        assert!(named.contains("\"dataset_id\" ="), "{named}");
    }

    #[test]
    fn a_stream_selection_locks_the_streams_for_update() {
        let sql = select_attribute_streams_query("urn:x", "a", DatasetSelector::AllStreams).to_string(PostgresQueryBuilder);
        assert!(sql.contains("FOR UPDATE OF \"s\""), "{sql}");
    }

    #[test]
    fn attribute_deletion_removes_the_observations_before_the_stream_holding_them() {
        let history = delete_stream_history_query(&[1]).to_string(PostgresQueryBuilder);
        let streams = delete_streams_query(&[1]).to_string(PostgresQueryBuilder);
        assert!(history.starts_with("DELETE FROM \"attribute_history\""), "{history}");
        assert!(history.contains("RETURNING \"write_id\""), "{history}");
        assert!(streams.starts_with("DELETE FROM \"temporal_attribute_streams\""), "{streams}");
    }

    #[test]
    fn attribute_deletion_records_no_tombstone() {
        for statement in [delete_stream_history_query(&[1]), delete_streams_query(&[1])] {
            let sql = statement.to_string(PostgresQueryBuilder);
            assert!(!sql.contains("INSERT"), "{sql}");
            assert!(!sql.contains("deleted_at"), "{sql}");
        }
    }
}
