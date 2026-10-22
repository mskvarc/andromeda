//! Direct PostgreSQL implementation of the hosted-context persistence port.

use crate::{
    jsonld_context::{
        kind_code::{created_kind, kind_of, stored_kind},
        listing_statement::{listing_page_query, metadata_by_identifier_query},
        mapper::{page_boundary, row_to_identifier, row_to_metadata},
        repo::JsonLdContextRepository,
        rows::{ContextMetadataRow, ContextSummaryRow},
        statement::{delete_by_identifier, hosted_insert, payload_by_identifier_query, record_access_update, replace_download_update, stored_context_query},
    },
    store_fault::{persistence_error, undecodable},
};
use andromeda_context_registry::{
    context_page::ContextPage,
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
use andromeda_postgresql_seaquery::statement::Statement;
use iri_rs::IriBuf;
use sea_query::{InsertStatement, error::Error as SeaQueryError};
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

    fn list<'a>(&'a self, query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ContextPage> {
        Box::pin(async move {
            let kind = query.kind.map(stored_kind);
            let (statement, read_order) = listing_page_query(query.detail_level, kind, query.embedded, &query.window.with_lookahead())
                .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
            match query.detail_level {
                ContextDetailLevel::Full => {
                    let mut rows: Vec<ContextMetadataRow> = statement
                        .to_query_as()
                        .fetch_all(&self.pool)
                        .await
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    read_order.present(&mut rows);
                    let (rows, reach) = query.window.settle(rows);
                    let boundary = page_boundary(&rows, |row| row.local_id.as_str()).map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    let metadata = rows
                        .into_iter()
                        .map(row_to_metadata)
                        .collect::<Result<Vec<ContextMetadata>, _>>()
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    Ok(ContextPage {
                        contexts: ListedContexts::Detailed(metadata),
                        boundary,
                        reach,
                    })
                }
                ContextDetailLevel::Summary => {
                    let mut rows: Vec<ContextSummaryRow> = statement
                        .to_query_as()
                        .fetch_all(&self.pool)
                        .await
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    read_order.present(&mut rows);
                    let (rows, reach) = query.window.settle(rows);
                    let boundary = page_boundary(&rows, |row| row.local_id.as_str()).map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    let identifiers = rows
                        .into_iter()
                        .map(row_to_identifier)
                        .collect::<Result<Vec<Identifier>, _>>()
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    Ok(ContextPage {
                        contexts: ListedContexts::Summary(identifiers),
                        boundary,
                        reach,
                    })
                }
            }
        })
    }

    fn get<'a>(&'a self, identifier: &'a Identifier, detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
        Box::pin(async move {
            match detail_level {
                ContextDetailLevel::Full => {
                    let row: Option<ContextMetadataRow> = metadata_by_identifier_query(identifier.as_str())
                        .to_query_as()
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    let metadata = row
                        .map(row_to_metadata)
                        .transpose()
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    Ok(metadata.map(|value| FetchedContext::Detailed(Box::new(value))))
                }
                ContextDetailLevel::Summary => {
                    let row: Option<(Json<JsonLiteral>,)> = payload_by_identifier_query(identifier.as_str())
                        .to_query_as()
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                    Ok(row.map(|value| FetchedContext::Payload(value.0.0)))
                }
            }
        })
    }

    fn record_access<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let result = record_access_update(identifier.as_str())
                .to_query()
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
            Ok(mutation(result.rows_affected()))
        })
    }

    fn lookup<'a>(&'a self, identifier: &'a Identifier) -> ContextRegistryFuture<'a, Option<StoredContext>> {
        Box::pin(async move {
            let row: Option<(String, i16, Option<String>)> = stored_context_query(identifier.as_str())
                .to_query_as()
                .fetch_optional(&self.pool)
                .await
                .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
            row.map(|(identifier, kind, source_url)| {
                let identifier = identifier
                    .parse::<Identifier>()
                    .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
                let kind = kind_of(JsonldContextKindCode::try_from(kind).map_err(|error| persistence_error::<ContextRegistryError>(error))?);
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
            let result = delete_by_identifier(identifier.as_str())
                .to_query()
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
            Ok(mutation(result.rows_affected()))
        })
    }

    fn replace_download<'a>(&'a self, identifier: &'a Identifier, download: &'a DownloadedContext) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let result = replace_download_update(identifier.as_str(), download.document_url.as_str(), &download.document)
                .to_query()
                .execute(&self.pool)
                .await
                .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
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
    let statement = insert_statement(identifier, payload, serving_url, kind).map_err(|error| persistence_error::<ContextRegistryError>(error))?;
    let result = statement
        .to_query()
        .execute(&repository.pool)
        .await
        .map_err(|error| persistence_error::<ContextRegistryError>(error))?;
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
    use crate::{
        jsonld_context::registry_store::{insert_statement, insertion, mutation},
        store_fault::persistence_error,
    };
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
        assert!(matches!(
            persistence_error::<ContextRegistryError>(sqlx::Error::PoolTimedOut),
            ContextRegistryError::StoreSaturated
        ));
        assert!(matches!(
            persistence_error::<ContextRegistryError>(sqlx::Error::RowNotFound),
            ContextRegistryError::Persistence { .. }
        ));
    }
}
