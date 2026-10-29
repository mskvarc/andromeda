//! Deleting the Temporal Evolution of one attribute (clause 5.6.17).
//!
//! The addressed streams are found through the temporal catalogue alone, never
//! through current state, so an attribute whose entity was deleted (clause
//! 5.6.6) still has its history removable.

use crate::{
    error::TemporalStoreError,
    history_removal::{remove_stream_history, stream_key_array},
    repo::TemporalRepository,
    stream_census::counted_stream_removal,
    temporal_catalog_read::{of_attribute, stream_catalog_join},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::{current::Entities, temporal::TemporalAttributeStreams};
use andromeda_postgresql_seaquery::{
    branch::qualified,
    nullable_text_key::{is_null_text_key, nullable_text_key},
    pg_array::any,
    statement::Statement,
};
use andromeda_postgresql_temporal_query::alias::{S, Te};
use sea_query::{DeleteStatement, DynIden, Expr, ExprTrait, LockType, Query, SelectStatement};

/// Which dataset instances of one attribute a deletion addresses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeStreamScope {
    /// Every dataset instance of the attribute.
    All,
    /// The one named dataset instance, or the default instance when none is named.
    Single,
}

impl TemporalRepository {
    /// Remove the addressed attribute streams and every observation they hold.
    ///
    /// Clause 5.6.17 deletes the Temporal Evolution of one attribute, so the
    /// history goes with the stream rather than being tombstoned; nothing in the
    /// database sequences that, so the order is stated here inside one
    /// transaction.
    ///
    /// Returns how many streams the deletion removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails.
    pub(crate) async fn delete_attribute_streams(
        &self,
        target: &ContextTarget,
        entity_id: &Identifier,
        attribute_name: &AttributeName,
        dataset_id: Option<&Identifier>,
        scope: AttributeStreamScope,
    ) -> Result<u64, TemporalStoreError> {
        let dataset_selector = match (scope, dataset_id) {
            (AttributeStreamScope::All, _) => DatasetSelector::AllStreams,
            (AttributeStreamScope::Single, Some(dataset_id)) => DatasetSelector::Named(dataset_id.as_str()),
            (AttributeStreamScope::Single, None) => DatasetSelector::Default,
        };
        let Some(attribute) = self.attribute_key(attribute_name).await? else {
            return Ok(0);
        };
        let select = select_attribute_streams_query(schema_iden(target).as_ref(), entity_id.as_str(), attribute, dataset_selector);
        let mut tx = self.pool.begin().await?;
        let stream_keys: Vec<i64> = select.to_query_scalar().fetch_all(&mut *tx).await?;
        if stream_keys.is_empty() {
            tx.rollback().await?;
            return Ok(0);
        }
        let schema = schema_iden(target);
        remove_stream_history(&mut tx, schema.as_ref(), &stream_keys).await?;
        let streams: i64 = counted_stream_removal(schema.as_ref(), delete_streams_query(schema.as_ref(), &stream_keys))
            .map_err(TemporalStoreError::StatementBuild)?
            .to_query_scalar()
            .fetch_one(&mut *tx)
            .await?;
        let streams = streams.unsigned_abs();
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

/// The identifiers of the streams a deletion addresses, locked for update.
///
/// The `datasetId` is compared as the grain index keys it, so one stream of
/// an attribute with many is found by an index probe rather than by reading
/// every stream of the attribute.
fn select_attribute_streams_query(schema: Option<&DynIden>, entity_id: &str, attribute: AttributeNameKey, dataset: DatasetSelector<'_>) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((S, TemporalAttributeStreams::StreamKey));
    stream_catalog_join(&mut statement, schema);
    statement
        .and_where(Expr::col((Te, Entities::Id)).eq(entity_id))
        .and_where(of_attribute(attribute));
    match dataset {
        DatasetSelector::AllStreams => {}
        DatasetSelector::Named(dataset_id) => {
            statement.and_where(nullable_text_key(Expr::col((S, TemporalAttributeStreams::DatasetId))).eq(dataset_id));
        }
        DatasetSelector::Default => {
            statement.and_where(is_null_text_key(Expr::col((S, TemporalAttributeStreams::DatasetId))));
        }
    }
    statement.lock_with_tables(LockType::Update, [S]);
    statement
}

/// Remove the addressed streams themselves.
fn delete_streams_query(schema: Option<&DynIden>, stream_keys: &[i64]) -> DeleteStatement {
    Query::delete()
        .from_table(qualified(schema, TemporalAttributeStreams::Table))
        .and_where(Expr::col(TemporalAttributeStreams::StreamKey).eq(any(stream_key_array(stream_keys))))
        .to_owned()
}

#[cfg(test)]
mod tests {
    use crate::attribute_stream_deletion::{DatasetSelector, delete_streams_query, select_attribute_streams_query};
    use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, DynIden, IntoIden, PostgresQueryBuilder};

    /// Clause 5.6.6 keeps the streams when the current state is deleted, so
    /// they are found through the stream's identity row, whatever its current
    /// state holds.
    #[test]
    fn a_stream_selection_is_not_answered_from_current_state() {
        let sql = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::AllStreams).to_string(PostgresQueryBuilder);
        assert!(
            sql.contains(r#"FROM "temporal_attribute_streams" AS "s" JOIN "entities" AS "te" ON "te"."entity_key" = "s"."entity_key""#),
            "{sql}"
        );
        for current_state in ["\"entity_search\"", "\"current_attributes\"", "\"created_at\""] {
            assert!(!sql.contains(current_state), "{current_state}: {sql}");
        }
    }
    #[test]
    fn one_stream_is_addressed_by_the_grain_index_key() {
        let default = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::Default).to_string(PostgresQueryBuilder);
        let named = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::Named("urn:d")).to_string(PostgresQueryBuilder);
        assert!(default.contains(r#"COALESCE("s"."dataset_id", '') = ('')"#), "{default}");
        assert!(named.contains(r#"COALESCE("s"."dataset_id", '') = 'urn:d'"#), "{named}");
    }

    #[test]
    fn a_stream_selection_locks_the_streams_for_update() {
        let sql = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::AllStreams).to_string(PostgresQueryBuilder);
        assert!(sql.contains("FOR UPDATE OF \"s\""), "{sql}");
    }

    #[test]
    fn attribute_deletion_removes_the_streams_by_their_bound_keys() {
        let streams = delete_streams_query(None, &[1]).to_string(PostgresQueryBuilder);
        assert_eq!(streams, r#"DELETE FROM "temporal_attribute_streams" WHERE "stream_key" = ANY(ARRAY [1])"#);
    }

    #[test]
    fn attribute_deletion_removes_the_streams_of_the_addressed_branch_schema() {
        // A Snapshot branch's attribute deletion must remove the branch's own
        // rows, not the identically-keyed rows of the live default schema.
        let schema: DynIden = Alias::new("snap_7").into_iden();
        let streams = delete_streams_query(Some(&schema), &[1]).to_string(PostgresQueryBuilder);
        assert!(streams.starts_with("DELETE FROM \"snap_7\".\"temporal_attribute_streams\""), "{streams}");
    }
}
