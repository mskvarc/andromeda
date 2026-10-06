//! Direct PostgreSQL implementation of the hosted-context persistence port.

use crate::{
    jsonld_context::{
        kind_code::{created_kind, kind_of, stored_kind},
        mapper::{row_to_identifier, row_to_metadata},
        repo::JsonLdContextRepository,
        rows::{ContextMetadataRow, ContextSummaryRow},
        statement::{
            delete_by_identifier,
            hosted_insert,
            metadata_by_identifier_query,
            metadata_page_query,
            payload_by_identifier_query,
            record_access_update,
            replace_download_update,
            stored_context_query,
            summary_page_query,
        },
    },
    store_fault::{StoreFault, persistence_failure, undecodable},
};
use andromeda_context_registry::{
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    insertion::ContextInsertion,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    store::{ContextRegistryFuture, ContextRegistryStore},
    stored_context::StoredContext,
};
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::context_cache::DownloadedContext;
use andromeda_ngsi_ld_lexicon::{identifier::Identifier, ngsi_ld_urn::JsonLdContextId};
use andromeda_ngsi_ld_model::jsonld_context::ContextMetadata;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
use iri_rs::IriBuf;
use sea_query::{InsertStatement, PostgresQueryBuilder, error::Error as SeaQueryError};
use sea_query_sqlx::SqlxBinder;
use sqlx::types::Json;
use url::Url;

impl ContextRegistryStore for JsonLdContextRepository {
    fn insert<'a>(
        &'a self,
        identifier: &'a JsonLdContextId,
        payload: &'a JsonLiteral,
        serving_url: &'a Url,
        kind: ContextCreationKind,
    ) -> ContextRegistryFuture<'a, ContextInsertion> {
        Box::pin(async move { insert_context(self, identifier, payload, serving_url, kind).await })
    }

    fn list<'a>(&'a self, query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ListedContexts> {
        Box::pin(async move {
            let after = query.after.map(Identifier::as_str);
            let limit = i64::from(query.limit);
            let kind = query.kind.map(stored_kind);
            match query.detail_level {
                ContextDetailLevel::Full => {
                    let (sql, values) = metadata_page_query(kind, after, limit).build_sqlx(PostgresQueryBuilder);
                    let rows: Vec<ContextMetadataRow> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                        .fetch_all(&self.pool)
                        .await
                        .map_err(|error| persistence_error(error))?;
                    let metadata = rows
                        .into_iter()
                        .map(row_to_metadata)
                        .collect::<Result<Vec<ContextMetadata>, _>>()
                        .map_err(|error| persistence_error(error))?;
                    Ok(ListedContexts::Detailed(metadata))
                }
                ContextDetailLevel::Summary => {
                    let (sql, values) = summary_page_query(kind, after, limit).build_sqlx(PostgresQueryBuilder);
                    let rows: Vec<ContextSummaryRow> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                        .fetch_all(&self.pool)
                        .await
                        .map_err(|error| persistence_error(error))?;
                    let identifiers = rows
                        .into_iter()
                        .map(row_to_identifier)
                        .collect::<Result<Vec<Identifier>, _>>()
                        .map_err(|error| persistence_error(error))?;
                    Ok(ListedContexts::Summary(identifiers))
                }
            }
        })
    }

    fn get<'a>(&'a self, identifier: &'a Identifier, detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
        Box::pin(async move {
            match detail_level {
                ContextDetailLevel::Full => {
                    let (sql, values) = metadata_by_identifier_query(identifier.as_str()).build_sqlx(PostgresQueryBuilder);
                    let row: Option<ContextMetadataRow> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|error| persistence_error(error))?;
                    let metadata = row.map(row_to_metadata).transpose().map_err(|error| persistence_error(error))?;
                    Ok(metadata.map(|value| FetchedContext::Detailed(Box::new(value))))
                }
                ContextDetailLevel::Summary => {
                    let (sql, values) = payload_by_identifier_query(identifier.as_str()).build_sqlx(PostgresQueryBuilder);
                    let row: Option<(Json<JsonLiteral>,)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|error| persistence_error(error))?;
                    Ok(row.map(|value| FetchedContext::Payload(value.0.0)))
                }
            }
        })
    }

    fn record_access<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let (sql, values) = record_access_update(identifier.as_str()).build_sqlx(PostgresQueryBuilder);
            let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error(error))?;
            Ok(mutation(result.rows_affected()))
        })
    }

    fn lookup<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
        Box::pin(async move {
            let (sql, values) = stored_context_query(identifier.as_str()).build_sqlx(PostgresQueryBuilder);
            let row: Option<(String, i16, Option<String>)> = sqlx::query_as_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| persistence_error(error))?;
            row.map(|(identifier, kind, source_url)| {
                let identifier = identifier.parse::<Identifier>().map_err(|error| persistence_error(error))?;
                let kind = kind_of(JsonldContextKindCode::try_from(kind).map_err(|error| persistence_error(error))?);
                let source_url = source_url
                    .map(IriBuf::new)
                    .transpose()
                    .map_err(|error| ContextRegistryError::from(undecodable(&error)))?;
                Ok(StoredContext { identifier, kind, source_url })
            })
            .transpose()
        })
    }

    fn delete<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let (sql, values) = delete_by_identifier(identifier.as_str()).build_sqlx(PostgresQueryBuilder);
            let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error(error))?;
            Ok(mutation(result.rows_affected()))
        })
    }

    fn replace_download<'a>(&'a self, identifier: &'a Identifier, download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let (sql, values) =
                replace_download_update(identifier.as_str(), download.document_url.as_str(), &download.document).build_sqlx(PostgresQueryBuilder);
            let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error(error))?;
            Ok(mutation(result.rows_affected()))
        })
    }
}

/// Insert a non-cached Context under the identifier and kind policy chose,
/// unless a row already holds the identifier.
async fn insert_context(
    repository: &JsonLdContextRepository,
    identifier: &JsonLdContextId,
    payload: &JsonLiteral,
    serving_url: &Url,
    kind: ContextCreationKind,
) -> Result<ContextInsertion, ContextRegistryError> {
    let statement = insert_statement(identifier, payload, serving_url, kind).map_err(|error| persistence_error(error))?;
    let (sql, values) = statement.build_sqlx(PostgresQueryBuilder);
    let result = sqlx::query_with(sqlx::AssertSqlSafe(sql.as_str()), values)
        .execute(&repository.pool)
        .await
        .map_err(|error| persistence_error(error))?;
    Ok(insertion(result.rows_affected()))
}

/// The insert of a non-cached Context.
///
/// `serving_url` is the URL the HTTP binding minted for the entry, on its
/// `apiRoot` path prefix and with its tenant parameter, and is stored exactly
/// as minted: the clause 6.30 path is the binding's to build, never this
/// adapter's.
fn insert_statement(
    identifier: &JsonLdContextId,
    payload: &JsonLiteral,
    serving_url: &Url,
    kind: ContextCreationKind,
) -> Result<InsertStatement, SeaQueryError> {
    hosted_insert(identifier.as_ref(), Some(serving_url.as_str()), created_kind(kind), payload)
}

/// Convert the affected-row count of a conflict-free insert into its outcome.
///
/// The insert does nothing on an identifier conflict, so no row affected means
/// the identifier was already held.
const fn insertion(rows_affected: u64) -> ContextInsertion {
    if rows_affected == 0 {
        ContextInsertion::Occupied
    } else {
        ContextInsertion::Inserted
    }
}

/// Collapse an implementation error into the policy's backend-neutral category.
///
/// Classify an adapter failure at the port boundary: backpressure keeps the
/// port's retryable category and every fault becomes its persistence failure.
#[track_caller]
fn persistence_error(error: impl StoreFault) -> ContextRegistryError {
    ContextRegistryError::from(persistence_failure(&error))
}

/// Convert affected-row counts into a named persistence outcome.
const fn mutation(rows_affected: u64) -> ContextMutation {
    if rows_affected == 0 {
        ContextMutation::Missing
    } else {
        ContextMutation::Applied
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::registry_store::{insert_statement, insertion, mutation, persistence_error};
    use andromeda_context_registry::{creation_kind::ContextCreationKind, error::ContextRegistryError, insertion::ContextInsertion, mutation::ContextMutation};
    use andromeda_json_literal::JsonLiteral;
    use andromeda_ngsi_ld_lexicon::ngsi_ld_urn::JsonLdContextId;
    use sea_query::PostgresQueryBuilder;
    use url::Url;

    /// Clause 6.2 lets the `apiRoot` carry a path prefix: the serving URL the
    /// HTTP binding minted is stored exactly as minted, prefix included, and
    /// with no query, since the URL names no tenant.
    #[test]
    fn a_prefixed_serving_url_is_stored_byte_for_byte() {
        let identifier: JsonLdContextId = "urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b".parse().expect("an identifier");
        let minted = "https://gw.example.com/broker/ngsi-ld/v1/jsonldContexts/urn:ngsi-ld:JsonLdContext:6f1c0e2a-3b5d-4e7f-8a9b-0c1d2e3f4a5b";
        let serving_url = Url::parse(minted).expect("a URL");

        let sql = insert_statement(
            &identifier,
            &JsonLiteral::admit(r#"{"@context": {}}"#).expect("an admissible document"),
            &serving_url,
            ContextCreationKind::ImplicitlyCreated,
        )
        .expect("the statement builds")
        .to_string(PostgresQueryBuilder);

        assert!(sql.contains(&format!("'{minted}'")), "{sql}");
    }

    /// The insert does nothing on a conflicting identifier, so no affected row
    /// is an identifier already held.
    #[test]
    fn affected_rows_map_to_named_insertion_outcomes() {
        pretty_assertions::assert_eq!(insertion(0), ContextInsertion::Occupied);
        pretty_assertions::assert_eq!(insertion(1), ContextInsertion::Inserted);
    }

    #[test]
    fn affected_rows_map_to_named_mutation_outcomes() {
        pretty_assertions::assert_eq!(mutation(0), ContextMutation::Missing);
        pretty_assertions::assert_eq!(mutation(1), ContextMutation::Applied);
        pretty_assertions::assert_eq!(mutation(4), ContextMutation::Applied);
    }

    #[test]
    fn a_pool_timeout_becomes_store_saturated() {
        assert!(matches!(persistence_error(sqlx::Error::PoolTimedOut), ContextRegistryError::StoreSaturated));
        assert!(matches!(persistence_error(sqlx::Error::RowNotFound), ContextRegistryError::Persistence { .. }));
    }
}
