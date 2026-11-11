//! `PostedQueryStore` implementation for `PostedQueryRepository`.

use crate::{
    posted_query::{
        lifecycle::{expired_statement, hold_statement, live_statement, lock_writes},
        mapper::posted_query_of,
        quota::eviction_statement,
        repo::PostedQueryRepository,
    },
    store_fault::persistence_error,
};
use andromeda_ngsi_ld_request::pagination::page_query::PageQuery;
use andromeda_posted_query::{
    error::PostedQueryError,
    posted_query::{PostedQuery, PostedQueryKind},
    retention::PostedQueryQuota,
    store::{PostedQueryStore, PostedQueryStoreFuture},
};
use andromeda_postgresql_seaquery::statement::Statement;
use chrono::{DateTime, Utc};

impl PostedQueryStore for PostedQueryRepository {
    fn hold<'a>(&'a self, query: &'a PostedQuery, expires_at: DateTime<Utc>, quota: PostedQueryQuota) -> PostedQueryStoreFuture<'a, ()> {
        Box::pin(async move {
            let mut transaction = self.pool().begin().await.map_err(|error| persistence_error::<PostedQueryError>(error))?;
            lock_writes(&mut transaction).await?;
            hold_statement(query, expires_at)?
                .to_query()
                .execute(&mut *transaction)
                .await
                .map_err(|error| persistence_error::<PostedQueryError>(error))?;
            if let Some(eviction) = eviction_statement(query.key(), quota) {
                eviction
                    .to_query()
                    .execute(&mut *transaction)
                    .await
                    .map_err(|error| persistence_error::<PostedQueryError>(error))?;
            }
            transaction.commit().await.map_err(|error| persistence_error::<PostedQueryError>(error))
        })
    }

    fn get<'a>(&'a self, key: &'a PageQuery, kind: PostedQueryKind) -> PostedQueryStoreFuture<'a, Option<PostedQuery>> {
        Box::pin(async move {
            let row = live_statement(key, kind)
                .to_query()
                .fetch_optional(self.pool())
                .await
                .map_err(|error| persistence_error::<PostedQueryError>(error))?;
            row.map(|row| posted_query_of(kind, &row)).transpose()
        })
    }

    fn delete_expired(&self, cutoff: DateTime<Utc>) -> PostedQueryStoreFuture<'_, u64> {
        Box::pin(async move {
            let mut transaction = self.pool().begin().await.map_err(|error| persistence_error::<PostedQueryError>(error))?;
            lock_writes(&mut transaction).await?;
            let deleted = expired_statement(cutoff)
                .to_query()
                .execute(&mut *transaction)
                .await
                .map_err(|error| persistence_error::<PostedQueryError>(error))?;
            transaction.commit().await.map_err(|error| persistence_error::<PostedQueryError>(error))?;
            Ok(deleted.rows_affected())
        })
    }
}
