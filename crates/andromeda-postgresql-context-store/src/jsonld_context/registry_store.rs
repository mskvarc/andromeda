//! Direct PostgreSQL implementation of the hosted-context persistence port.

use crate::jsonld_context::{
    mapper::{row_to_identifier, row_to_metadata},
    repo::JsonLdContextRepository,
    rows::{ContextMetadataRow, ContextSummaryRow},
};
use andromeda_context_registry::{
    creation_kind::ContextCreationKind,
    detail_level::ContextDetailLevel,
    error::ContextRegistryError,
    fetched_context::FetchedContext,
    list_query::ContextListQuery,
    listed_contexts::ListedContexts,
    mutation::ContextMutation,
    store::{ContextRegistryFuture, ContextRegistryStore},
    stored_context::StoredContext,
};
use andromeda_ngsi_ld_lexicon::uri::{NGSI_LD_URN_NID, Uri};
use andromeda_ngsi_ld_model::jsonld_context::{ContextKind, ContextMetadata};
use iri_rs::IriBuf;
use serde_json::Value;
use sqlx::types::Json;
use std::fmt::Display;
use url::Url;
use urn_rs::{Urn, UrnBuilder};
use uuid::Uuid;

impl ContextRegistryStore for JsonLdContextRepository {
    fn create<'a>(&'a self, payload: Value, base_url: Option<&'a Url>, kind: ContextCreationKind) -> ContextRegistryFuture<'a, Urn> {
        Box::pin(async move { insert_context(self, payload, base_url, kind).await })
    }

    fn list<'a>(&'a self, query: ContextListQuery<'a>) -> ContextRegistryFuture<'a, ListedContexts> {
        Box::pin(async move {
            let after = query.after.map(Uri::as_str);
            let limit = i64::from(query.limit);
            match query.detail_level {
                ContextDetailLevel::Full => {
                    let rows: Vec<ContextMetadataRow> = sqlx::query_as(
                        r#"
                        SELECT id AS local_id, url, kind::text AS kind, created_at,
                               expires_at, last_usage, number_of_hits, extra_info
                        FROM jsonld_contexts
                        WHERE ($1::text IS NULL OR kind = $1::jsonld_context_kind)
                          AND ($2::text IS NULL OR id > $2)
                        ORDER BY id
                        LIMIT $3
                        "#,
                    )
                    .bind(query.kind.map(<&str>::from))
                    .bind(after)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                    let metadata = rows
                        .into_iter()
                        .map(row_to_metadata)
                        .collect::<Result<Vec<ContextMetadata>, _>>()
                        .map_err(persistence_error)?;
                    Ok(ListedContexts::Detailed(metadata))
                }
                ContextDetailLevel::Summary => {
                    let rows: Vec<ContextSummaryRow> = sqlx::query_as(
                        r#"
                        SELECT id, url
                        FROM jsonld_contexts
                        WHERE ($1::text IS NULL OR kind = $1::jsonld_context_kind)
                          AND ($2::text IS NULL OR id > $2)
                        ORDER BY id
                        LIMIT $3
                        "#,
                    )
                    .bind(query.kind.map(<&str>::from))
                    .bind(after)
                    .bind(limit)
                    .fetch_all(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                    let identifiers = rows
                        .into_iter()
                        .map(row_to_identifier)
                        .collect::<Result<Vec<Uri>, _>>()
                        .map_err(persistence_error)?;
                    Ok(ListedContexts::Summary(identifiers))
                }
            }
        })
    }

    fn get<'a>(&'a self, identifier: &'a Uri, detail_level: ContextDetailLevel) -> ContextRegistryFuture<'a, Option<FetchedContext>> {
        Box::pin(async move {
            match detail_level {
                ContextDetailLevel::Full => {
                    let row: Option<ContextMetadataRow> = sqlx::query_as(
                        r#"
                        SELECT id AS local_id, url, kind::text AS kind, created_at,
                               expires_at, last_usage, number_of_hits, extra_info
                        FROM jsonld_contexts
                        WHERE id = $1 OR url = $1
                        "#,
                    )
                    .bind(identifier.as_str())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(persistence_error)?;
                    let metadata = row.map(row_to_metadata).transpose().map_err(persistence_error)?;
                    Ok(metadata.map(|value| FetchedContext::Detailed(Box::new(value))))
                }
                ContextDetailLevel::Summary => {
                    let row: Option<(Json<Value>,)> = sqlx::query_as("SELECT payload FROM jsonld_contexts WHERE id = $1 OR url = $1")
                        .bind(identifier.as_str())
                        .fetch_optional(&self.pool)
                        .await
                        .map_err(persistence_error)?;
                    Ok(row.map(|value| FetchedContext::Payload(value.0.0)))
                }
            }
        })
    }

    fn record_access<'a>(&'a self, identifier: &'a Uri) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let result = sqlx::query("UPDATE jsonld_contexts SET last_usage = NOW(), number_of_hits = number_of_hits + 1 WHERE id = $1 OR url = $1")
                .bind(identifier.as_str())
                .execute(&self.pool)
                .await
                .map_err(persistence_error)?;
            Ok(mutation(result.rows_affected()))
        })
    }

    fn lookup<'a>(&'a self, identifier: &'a Uri) -> ContextRegistryFuture<'a, Option<StoredContext>> {
        Box::pin(async move {
            let row: Option<(String, String, Option<String>)> =
                sqlx::query_as("SELECT id, kind::text AS kind, url FROM jsonld_contexts WHERE id = $1 OR url = $1")
                    .bind(identifier.as_str())
                    .fetch_optional(&self.pool)
                    .await
                    .map_err(persistence_error)?;
            row.map(|(identifier, kind, source_url)| {
                let identifier = identifier.parse::<Uri>().map_err(persistence_error)?;
                let kind = kind.parse::<ContextKind>().map_err(persistence_error)?;
                let source_url = source_url.map(IriBuf::new).transpose().map_err(|error| ContextRegistryError::Persistence {
                    detail: format!("stored Context URL is invalid: {}", error.0),
                })?;
                Ok(StoredContext { identifier, kind, source_url })
            })
            .transpose()
        })
    }

    fn delete<'a>(&'a self, identifier: &'a Uri) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let result = sqlx::query("DELETE FROM jsonld_contexts WHERE id = $1 OR url = $1")
                .bind(identifier.as_str())
                .execute(&self.pool)
                .await
                .map_err(persistence_error)?;
            Ok(mutation(result.rows_affected()))
        })
    }

    fn replace_payload<'a>(&'a self, identifier: &'a Uri, payload: Value) -> ContextRegistryFuture<'a, ContextMutation> {
        Box::pin(async move {
            let result = sqlx::query("UPDATE jsonld_contexts SET payload = $2, last_usage = NOW() WHERE id = $1")
                .bind(identifier.as_str())
                .bind(payload)
                .execute(&self.pool)
                .await
                .map_err(persistence_error)?;
            Ok(mutation(result.rows_affected()))
        })
    }
}

/// Insert a non-cached Context using the exact kind selected by policy.
async fn insert_context(
    repository: &JsonLdContextRepository,
    payload: Value,
    base_url: Option<&Url>,
    kind: ContextCreationKind,
) -> Result<Urn, ContextRegistryError> {
    let local_id = UrnBuilder::new(NGSI_LD_URN_NID, &format!("Context:{}", Uuid::new_v4()))
        .build()
        .map_err(|error| ContextRegistryError::Invariant {
            detail: format!("failed to build hosted Context URN: {error}"),
        })?;
    let serving_url = base_url.map(|base| {
        let mut serving = base.clone(); // clone: the caller retains its request origin while persistence owns the serving URL.
        serving.set_path(&format!("/ngsi-ld/v1/jsonldContexts/{local_id}"));
        serving
    });
    let kind_label = match kind {
        ContextCreationKind::Hosted => "Hosted",
        ContextCreationKind::ImplicitlyCreated => "ImplicitlyCreated",
    };
    sqlx::query(
        "INSERT INTO jsonld_contexts (id, url, kind, payload, created_at, last_usage, number_of_hits) VALUES ($1, $2, $3::jsonld_context_kind, $4, NOW(), NOW(), 0)",
    )
    .bind(local_id.to_string())
    .bind(serving_url.as_ref().map(Url::as_str))
    .bind(kind_label)
    .bind(payload)
    .execute(&repository.pool)
    .await
    .map_err(persistence_error)?;
    Ok(local_id)
}

/// Collapse an implementation error into the policy's backend-neutral category.
fn persistence_error(error: impl Display) -> ContextRegistryError {
    ContextRegistryError::Persistence { detail: error.to_string() }
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
    use crate::jsonld_context::registry_store::mutation;
    use andromeda_context_registry::mutation::ContextMutation;

    #[test]
    fn affected_rows_map_to_named_mutation_outcomes() {
        pretty_assertions::assert_eq!(mutation(0), ContextMutation::Missing);
        pretty_assertions::assert_eq!(mutation(1), ContextMutation::Applied);
        pretty_assertions::assert_eq!(mutation(4), ContextMutation::Applied);
    }
}
