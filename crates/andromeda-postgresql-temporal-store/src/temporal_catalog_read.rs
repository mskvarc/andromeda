//! Reading the temporal catalogue.
//!
//! Every question the temporal mutation policy asks before it acts — does this
//! entity have a Temporal Evolution, does it record this attribute, which
//! datasets does that attribute have — is answered from `temporal_entities` and
//! `temporal_attribute_streams` alone. Current state is never consulted: clause
//! 5.6.6 leaves the Temporal Evolution standing when the entity is deleted, so
//! an answer taken from `entities` would report history that exists as absent.
//!
//! The questions a read asks (existence, types, recorded shapes) read the
//! history sources, so on the live branch they count the entries still queued;
//! a mutation drains the Entity's queued entries before it asks its own.

use crate::{error::TemporalStoreError, repo::TemporalRepository, stored_attribute_shape::StoredAttributeShape, stored_catalog_error::StoredCatalogError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_codes::{attribute_kind::AttributeKind, value_kind::ValueKind};
use andromeda_postgresql_query::{
    dictionary::names_in_key_order::type_names_of,
    history_source::{HistoryBranch, HistoryOf, attribute_rows::attribute_rows, entity_rows::entity_rows, grain::HistoryGrain},
};
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{AttributeHistory, TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    pg_array::{any, text_array},
};
use andromeda_postgresql_temporal_query::{
    alias::{Ah, An, S, Te},
    recorded_shape::{RecordedShape, RecordedShapeColumn, recorded_shape},
};
use sea_query::{Alias, DynIden, Expr, ExprTrait, Iden, JoinType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Row};
use std::collections::{HashMap, HashSet};

impl TemporalRepository {
    /// Whether the entity has a Temporal Evolution at all.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn temporal_entity_exists(&self, branch: &HistoryBranch, entity_id: &Identifier) -> Result<bool, TemporalStoreError> {
        let (sql, values) = temporal_entity_exists_query(branch, entity_id.as_str()).build_sqlx(PostgresQueryBuilder);
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
    /// [`TemporalStoreError::StoredCatalog`] when a stored identifier or type
    /// name is not the validated shape the catalogue writes.
    pub(crate) async fn temporal_entity_types(
        &self,
        branch: &HistoryBranch,
        entity_ids: &[Identifier],
    ) -> Result<HashMap<Identifier, Vec<EntityTypeName>>, TemporalStoreError> {
        if entity_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let identifiers: Vec<&str> = entity_ids.iter().map(Identifier::as_str).collect();
        let (sql, values) = temporal_entity_types_query(branch, &identifiers).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        let mut types_by_entity = HashMap::with_capacity(rows.len());
        for row in rows {
            let raw_id: String = row.try_get("id")?;
            let entity_id = raw_id.parse::<Identifier>().map_err(StoredCatalogError::EntityId)?;
            let raw_types: Vec<String> = row.try_get("types")?;
            let types = raw_types
                .into_iter()
                .map(|raw| raw.parse::<EntityTypeName>().map_err(StoredCatalogError::EntityTypeName))
                .collect::<Result<Vec<EntityTypeName>, StoredCatalogError>>()?;
            types_by_entity.insert(entity_id, types);
        }
        Ok(types_by_entity)
    }

    /// The family-and-value-shape combinations each named attribute has been
    /// recorded with across the whole history, tombstones excepted (they carry
    /// no value).
    ///
    /// The family travels with the shape because Table 4.5.19.1-3 is keyed on
    /// it alone: a Relationship and a `ListRelationship` have `distinctCount`
    /// and `totalCount` and nothing else, whichever column their targets were
    /// parked in. The shape itself is refined past the stored `value_kind`
    /// code, because Tables 4.5.19.1-1 and 4.5.19.1-2 give a JSON Array methods
    /// a JSON Object does not have and a `Time` an `avg` a `Date` does not
    /// have, while one JSONB column holds both of the first pair and one text
    /// column both of the second.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::StoredCatalog`] when a stored name, family code or
    /// projected shape code is not one the dictionary or the projection knows.
    pub(crate) async fn stored_attribute_shapes(
        &self,
        branch: &HistoryBranch,
        attributes: &[AttributeName],
    ) -> Result<HashMap<AttributeName, HashSet<StoredAttributeShape>>, TemporalStoreError> {
        if attributes.is_empty() {
            return Ok(HashMap::new());
        }
        let names: Vec<&str> = attributes.iter().map(AttributeName::as_str).collect();
        let (sql, values) = stored_attribute_shapes_query(branch, &names).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        let mut shapes_by_attribute: HashMap<AttributeName, HashSet<StoredAttributeShape>> = HashMap::new();
        for row in rows {
            let raw_name: String = row.try_get("name")?;
            let name = raw_name.parse::<AttributeName>().map_err(StoredCatalogError::AttributeName)?;
            let stored_kind: i16 = row.try_get(Iden::to_string(&AttributeHistory::AttributeKind).as_str())?;
            let kind = AttributeKind::try_from(stored_kind).map_err(StoredCatalogError::AttributeKind)?;
            let code: i16 = row.try_get(Iden::to_string(&RecordedShapeColumn).as_str())?;
            let shape = RecordedShape::try_from(code).map_err(StoredCatalogError::RecordedShape)?;
            shapes_by_attribute.entry(name).or_default().insert(StoredAttributeShape { kind, shape });
        }
        Ok(shapes_by_attribute)
    }

    /// Whether the entity records any stream for the named attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn attribute_stream_exists(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<bool, TemporalStoreError> {
        let (sql, values) =
            attribute_stream_exists_query(schema_iden(target).as_ref(), entity_id.as_str(), attribute_name.as_str()).build_sqlx(PostgresQueryBuilder);
        let exists = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        Ok(exists)
    }

    /// The `datasetId` of every stream the entity records for one attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::StoredCatalog`] when a stored discriminator is not a
    /// URI.
    pub(crate) async fn attribute_dataset_ids(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<Vec<Option<Identifier>>, TemporalStoreError> {
        let (sql, values) =
            attribute_dataset_ids_query(schema_iden(target).as_ref(), entity_id.as_str(), attribute_name.as_str()).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        rows.iter()
            .map(|row| {
                Ok(row
                    .try_get::<Option<String>, _>("dataset_id")?
                    .map(|raw| raw.parse::<Identifier>().map_err(StoredCatalogError::StreamDatasetId))
                    .transpose()?)
            })
            .collect()
    }

    /// Every expanded attribute IRI the entity records a stream for.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::StoredCatalog`] when a stored name is not an
    /// attribute IRI.
    pub(crate) async fn temporal_attribute_names(&self, target: &ContextTarget, entity_id: &Identifier) -> Result<Vec<AttributeName>, TemporalStoreError> {
        let (sql, values) = temporal_attribute_names_query(schema_iden(target).as_ref(), entity_id.as_str()).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let raw: String = row.try_get("attribute_name")?;
                Ok(raw.parse::<AttributeName>().map_err(StoredCatalogError::AttributeName)?)
            })
            .collect()
    }
}

/// Join a stream to its temporal entity and its attribute name.
///
/// Every catalogue read reaches the entity URI and the attribute IRI through
/// this join rather than through `entities`, so a current-state deletion (clause
/// 5.6.6) leaves the catalogue answering.
pub(crate) fn stream_catalog_join(statement: &mut SelectStatement, schema: Option<&DynIden>) {
    statement
        .from(qualified(schema, TemporalAttributeStreams::Table).alias(S))
        .join_as(
            JoinType::Join,
            qualified(schema, TemporalEntities::Table),
            Te,
            Expr::col((Te, TemporalEntities::TemporalEntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
        )
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((S, TemporalAttributeStreams::AttributeNameKey)),
        );
}

/// The distinct `(name, recorded shape)` pairs recorded for the named
/// attributes, tombstone rows excluded.
///
/// The shape is the refined one Tables 4.5.19.1-1 and 4.5.19.1-2 distinguish,
/// not the stored `value_kind`: an array and an object share one JSONB column
/// and admit different methods, and so do a `Time`, a `Date` and a plain string
/// in the text column. The family is projected beside it because Table
/// 4.5.19.1-3 is keyed on the family alone. Tombstone rows are excluded, which
/// is also what makes the projection total — a deletion carries no value and so
/// no shape.
fn stored_attribute_shapes_query(branch: &HistoryBranch, names: &[&str]) -> SelectStatement {
    Query::select()
        .distinct()
        .column((An, AttributeNames::Name))
        .column((Ah, AttributeHistory::AttributeKind))
        .expr_as(recorded_shape(), RecordedShapeColumn)
        .from(attribute_rows(branch, &HistoryOf::EveryEntity, Ah))
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((Ah, HistoryGrain::AttributeNameKey)),
        )
        .and_where(Expr::col((An, AttributeNames::Name)).eq(any(text_array(names))))
        .and_where(Expr::col((Ah, AttributeHistory::ValueKind)).ne(ValueKind::Tombstone.code()))
        .to_owned()
}

/// Whether the entity has a Temporal Evolution at all.
fn temporal_entity_exists_query(branch: &HistoryBranch, entity_id: &str) -> SelectStatement {
    let mut inner = Query::select();
    inner
        .expr(Expr::val(1))
        .from(entity_rows(branch, Te))
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id));
    let mut statement = Query::select();
    statement.expr(Expr::exists(inner));
    statement
}

/// The identifier and type IRIs of every entity in `ids`, the IRIs read back
/// through the type dictionary in stored order under the `types` column.
fn temporal_entity_types_query(branch: &HistoryBranch, ids: &[&str]) -> SelectStatement {
    Query::select()
        .column((Te, TemporalEntities::Id))
        .expr_as(type_names_of(Expr::col((Te, TemporalEntities::EntityTypeNameKeys))), Alias::new("types"))
        .from(entity_rows(branch, Te))
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(any(text_array(ids))))
        .to_owned()
}

/// Whether the entity records any stream for the named attribute.
fn attribute_stream_exists_query(schema: Option<&DynIden>, entity_id: &str, attribute_name: &str) -> SelectStatement {
    let mut inner = Query::select();
    inner.expr(Expr::val(1));
    stream_catalog_join(&mut inner, schema);
    inner
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(attribute_name));
    let mut statement = Query::select();
    statement.expr(Expr::exists(inner));
    statement
}

/// The `datasetId` of every stream the entity records for one attribute.
fn attribute_dataset_ids_query(schema: Option<&DynIden>, entity_id: &str, attribute_name: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((S, TemporalAttributeStreams::DatasetId));
    stream_catalog_join(&mut statement, schema);
    statement
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(Expr::col((An, AttributeNames::Name)).eq(attribute_name));
    statement
}

/// Every distinct attribute IRI the entity records a stream for.
fn temporal_attribute_names_query(schema: Option<&DynIden>, entity_id: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .distinct()
        .expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    stream_catalog_join(&mut statement, schema);
    statement.and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id));
    statement
}

#[cfg(test)]
mod tests {
    use crate::temporal_catalog_read::{
        attribute_dataset_ids_query,
        attribute_stream_exists_query,
        stored_attribute_shapes_query,
        temporal_attribute_names_query,
        temporal_entity_exists_query,
        temporal_entity_types_query,
    };
    use andromeda_postgresql_query::history_source::HistoryBranch;
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn no_catalogue_question_is_answered_from_current_state() {
        for statement in [
            temporal_entity_exists_query(&HistoryBranch::live(), "urn:x").to_string(PostgresQueryBuilder),
            attribute_stream_exists_query(None, "urn:x", "https://ex/speed").to_string(PostgresQueryBuilder),
            attribute_dataset_ids_query(None, "urn:x", "https://ex/speed").to_string(PostgresQueryBuilder),
            temporal_attribute_names_query(None, "urn:x").to_string(PostgresQueryBuilder),
        ] {
            assert!(!statement.contains("\"entities\""), "{statement}");
            assert!(!statement.contains("temporal_entity_attributes"), "{statement}");
        }
    }

    /// Clause 5.7.4.4 refuses an aggregation over an ineligible attribute from
    /// the shapes it has been recorded with, which on the live branch include
    /// those of its queued entries.
    #[test]
    fn recorded_shapes_include_pending_entries() {
        let sql = stored_attribute_shapes_query(&HistoryBranch::live(), &["https://ex/speed"]).to_string(PostgresQueryBuilder);
        assert!(sql.contains(r#"UNION ALL (SELECT "pe"."entity_id" AS "entity_id""#), "{sql}");
        assert!(sql.contains(r#""ah"."value_kind" <> 0"#), "{sql}");
    }

    /// Clause 4.5.6: an Entity whose only history is a queued intent stating
    /// types has a Temporal Evolution, under the types of its latest such
    /// intent.
    #[test]
    fn an_entity_with_only_pending_history_exists() {
        for sql in [
            temporal_entity_exists_query(&HistoryBranch::live(), "urn:x").to_string(PostgresQueryBuilder),
            temporal_entity_types_query(&HistoryBranch::live(), &["urn:x"]).to_string(PostgresQueryBuilder),
        ] {
            assert!(sql.contains(r#"SELECT DISTINCT ON ("pt"."entity_id") "pt"."entity_id" AS "id""#), "{sql}");
            assert!(sql.contains(r#""te"."id""#), "{sql}");
        }
    }
}
