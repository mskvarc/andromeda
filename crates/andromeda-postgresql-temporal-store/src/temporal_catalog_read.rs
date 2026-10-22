//! Reading the temporal catalogue.
//!
//! Every question the temporal mutation policy asks before it acts — does this
//! entity have a Temporal Evolution, does it record this attribute, which
//! datasets does that attribute have — is answered from `temporal_entities` and
//! `temporal_attribute_streams` alone. Current state is never consulted: clause
//! 5.6.6 leaves the Temporal Evolution standing when the entity is deleted, so
//! an answer taken from `entities` would report history that exists as absent.
//!
//! The questions a read asks (existence, types) read the history sources, so
//! on the live branch they count the entries still queued; a mutation drains
//! the Entity's queued entries before it asks its own.

use crate::{error::TemporalStoreError, repo::TemporalRepository, stored_catalog_error::StoredCatalogError};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName, identifier::Identifier};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::names_in_key_order::type_names_of,
    history_source::{HistoryBranch, entity_rows::entity_rows},
};
use andromeda_postgresql_schema::{
    name_dictionary::AttributeNames,
    temporal::{TemporalAttributeStreams, TemporalEntities},
};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::is_any_of};
use andromeda_postgresql_temporal_query::alias::{An, S, Te};
use sea_query::{Alias, DynIden, Expr, ExprTrait, JoinType, PostgresQueryBuilder, Query, SelectStatement, SimpleExpr};
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Row};
use std::collections::HashMap;

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

    /// Whether the entity records any stream for the named attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when the dictionary read fails,
    /// [`TemporalStoreError::Database`] when the statement fails.
    pub(crate) async fn attribute_stream_exists(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<bool, TemporalStoreError> {
        let Some(attribute) = self.attribute_key(attribute_name).await? else {
            return Ok(false);
        };
        let (sql, values) = attribute_stream_exists_query(schema_iden(target).as_ref(), entity_id.as_str(), attribute).build_sqlx(PostgresQueryBuilder);
        let exists = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_one(&self.pool).await?;
        Ok(exists)
    }

    /// The `datasetId` of every stream the entity records for one attribute.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::NameLookup`] when the dictionary read fails,
    /// [`TemporalStoreError::Database`] when the statement fails and
    /// [`TemporalStoreError::StoredCatalog`] when a stored discriminator is not a
    /// URI.
    pub(crate) async fn attribute_dataset_ids(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
    ) -> Result<Vec<Option<Identifier>>, TemporalStoreError> {
        let Some(attribute) = self.attribute_key(attribute_name).await? else {
            return Ok(Vec::new());
        };
        let (sql, values) = attribute_dataset_ids_query(schema_iden(target).as_ref(), entity_id.as_str(), attribute).build_sqlx(PostgresQueryBuilder);
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

/// Join a stream to its temporal entity.
///
/// Every catalogue read reaches the entity URI through this join rather than
/// through `entities`, so a current-state deletion (clause 5.6.6) leaves the
/// catalogue answering. An attribute is addressed by the dictionary key its
/// name resolved to ([`of_attribute`]).
pub(crate) fn stream_catalog_join(statement: &mut SelectStatement, schema: Option<&DynIden>) {
    statement.from(qualified(schema, TemporalAttributeStreams::Table).alias(S)).join_as(
        JoinType::Join,
        qualified(schema, TemporalEntities::Table),
        Te,
        Expr::col((Te, TemporalEntities::TemporalEntityKey)).equals((S, TemporalAttributeStreams::TemporalEntityKey)),
    );
}

/// The streams of the attribute keyed `attribute` among those
/// [`stream_catalog_join`] reads.
pub(crate) fn of_attribute(attribute: AttributeNameKey) -> SimpleExpr {
    Expr::col((S, TemporalAttributeStreams::AttributeNameKey)).eq(attribute.get())
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
        .and_where(is_any_of(Expr::col((Te, TemporalEntities::Id)), ids.iter().map(|id| (*id).to_owned())))
        .to_owned()
}

/// Whether the entity records any stream for the named attribute.
fn attribute_stream_exists_query(schema: Option<&DynIden>, entity_id: &str, attribute: AttributeNameKey) -> SelectStatement {
    let mut inner = Query::select();
    inner.expr(Expr::val(1));
    stream_catalog_join(&mut inner, schema);
    inner
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(of_attribute(attribute));
    let mut statement = Query::select();
    statement.expr(Expr::exists(inner));
    statement
}

/// The `datasetId` of every stream the entity records for one attribute.
fn attribute_dataset_ids_query(schema: Option<&DynIden>, entity_id: &str, attribute: AttributeNameKey) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((S, TemporalAttributeStreams::DatasetId));
    stream_catalog_join(&mut statement, schema);
    statement
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(of_attribute(attribute));
    statement
}

/// Every distinct attribute IRI the entity records a stream for.
fn temporal_attribute_names_query(schema: Option<&DynIden>, entity_id: &str) -> SelectStatement {
    let mut statement = Query::select();
    statement
        .distinct()
        .expr_as(Expr::col((An, AttributeNames::Name)), Alias::new("attribute_name"));
    stream_catalog_join(&mut statement, schema);
    statement
        .join_as(
            JoinType::Join,
            AttributeNames::Table,
            An,
            Expr::col((An, AttributeNames::AttributeNameKey)).equals((S, TemporalAttributeStreams::AttributeNameKey)),
        )
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id));
    statement
}

#[cfg(test)]
mod tests {
    use crate::temporal_catalog_read::{
        attribute_dataset_ids_query,
        attribute_stream_exists_query,
        temporal_attribute_names_query,
        temporal_entity_exists_query,
        temporal_entity_types_query,
    };
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, history_source::HistoryBranch};
    use sea_query::PostgresQueryBuilder;

    #[test]
    fn no_catalogue_question_is_answered_from_current_state() {
        for statement in [
            temporal_entity_exists_query(&HistoryBranch::live(), "urn:x").to_string(PostgresQueryBuilder),
            attribute_stream_exists_query(None, "urn:x", AttributeNameKey::from(7)).to_string(PostgresQueryBuilder),
            attribute_dataset_ids_query(None, "urn:x", AttributeNameKey::from(7)).to_string(PostgresQueryBuilder),
            temporal_attribute_names_query(None, "urn:x").to_string(PostgresQueryBuilder),
        ] {
            assert!(!statement.contains("\"entities\""), "{statement}");
            assert!(!statement.contains("temporal_entity_attributes"), "{statement}");
        }
    }

    /// Clause 4.5.6: an Entity whose only history is a queued intent stating
    /// types has a Temporal Evolution, under every type its queued intents
    /// state.
    #[test]
    fn an_entity_with_only_pending_history_exists() {
        for sql in [
            temporal_entity_exists_query(&HistoryBranch::live(), "urn:x").to_string(PostgresQueryBuilder),
            temporal_entity_types_query(&HistoryBranch::live(), &["urn:x"]).to_string(PostgresQueryBuilder),
        ] {
            assert!(
                sql.contains(r#"(SELECT "pt"."entity_id" AS "id", "pt"."entity_type_name_keys" AS "entity_type_name_keys""#),
                "{sql}"
            );
            assert!(sql.contains(r#""te"."id""#), "{sql}");
        }
    }
}
