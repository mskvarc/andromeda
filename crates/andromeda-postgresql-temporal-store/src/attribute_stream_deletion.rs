//! Deleting the Temporal Evolution of one attribute (clause 5.6.17).
//!
//! The addressed streams are found through the temporal catalogue alone, never
//! through current state, so an attribute whose entity was deleted (clause
//! 5.6.6) still has its history removable.

use crate::{
    error::TemporalStoreError,
    history_removal::{remove_stream_history, stream_key_array},
    repo::TemporalRepository,
    temporal_catalog_read::{of_attribute, stream_catalog_join},
    write_record_release::release_named,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, identifier::Identifier};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_query::attribute_name_key::AttributeNameKey;
use andromeda_postgresql_schema::temporal::{TemporalAttributeStreams, TemporalEntities};
use andromeda_postgresql_seaquery::{branch::qualified, pg_array::any};
use andromeda_postgresql_temporal_query::alias::{S, Te};
use sea_query::{DeleteStatement, DynIden, Expr, ExprTrait, LockType, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::AssertSqlSafe;

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
    /// transaction, and the idempotency records the removed rows were the last
    /// protected effect of are released with them.
    ///
    /// Returns how many streams the deletion removed.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalStoreError::Database`] when a statement fails, including
    /// when a removed row's write identity does not carry the expected type.
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
        let (select_sql, select_values) =
            select_attribute_streams_query(schema_iden(target).as_ref(), entity_id.as_str(), attribute, dataset_selector).build_sqlx(PostgresQueryBuilder);
        let mut tx = self.pool.begin().await?;
        let stream_keys: Vec<i64> = sqlx::query_scalar_with(AssertSqlSafe(select_sql), select_values).fetch_all(&mut *tx).await?;
        if stream_keys.is_empty() {
            tx.rollback().await?;
            return Ok(0);
        }
        let schema = schema_iden(target);
        let removed = remove_stream_history(&mut tx, schema.as_ref(), &stream_keys).await?;
        let (streams_sql, streams_values) = delete_streams_query(schema.as_ref(), &stream_keys).build_sqlx(PostgresQueryBuilder);
        let streams = sqlx::query_with(AssertSqlSafe(streams_sql), streams_values)
            .execute(&mut *tx)
            .await?
            .rows_affected();
        release_named(&mut tx, &removed.writes).await?;
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
/// The default instance is addressed by the absence of a `datasetId`, never by
/// a sentinel value, so a stream with no dataset is matched by `IS NULL`.
fn select_attribute_streams_query(schema: Option<&DynIden>, entity_id: &str, attribute: AttributeNameKey, dataset: DatasetSelector<'_>) -> SelectStatement {
    let mut statement = Query::select();
    statement.column((S, TemporalAttributeStreams::StreamKey));
    stream_catalog_join(&mut statement, schema);
    statement
        .and_where(Expr::col((Te, TemporalEntities::Id)).eq(entity_id))
        .and_where(of_attribute(attribute));
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

    #[test]
    fn a_stream_selection_is_not_answered_from_current_state() {
        let sql = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::AllStreams).to_string(PostgresQueryBuilder);
        assert!(!sql.contains("\"entities\""), "{sql}");
    }
    #[test]
    fn the_default_instance_is_addressed_by_absence_rather_than_by_a_value() {
        let default = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::Default).to_string(PostgresQueryBuilder);
        let named = select_attribute_streams_query(None, "urn:x", AttributeNameKey::from(7), DatasetSelector::Named("urn:d")).to_string(PostgresQueryBuilder);
        assert!(default.contains("\"dataset_id\" IS NULL"), "{default}");
        assert!(named.contains("\"dataset_id\" ="), "{named}");
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
