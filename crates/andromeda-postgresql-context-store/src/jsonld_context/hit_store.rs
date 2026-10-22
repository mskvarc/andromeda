//! Direct PostgreSQL implementation of durable Context hit accounting.

use crate::{jsonld_context::repo::JsonLdContextRepository, store_fault::persistence_failure};
use andromeda_context_registry::{
    error::ContextRegistryError,
    hit_store::{ContextHitStore, ContextHitStoreFuture},
};
use andromeda_jsonld::hit_counter::HitDelta;
use andromeda_persistence_failure::persistence_failure::PersistenceFailure;
use andromeda_postgresql_schema::jsonld::JsonldContexts;
use andromeda_postgresql_seaquery::{
    clock::now,
    pg_array::text_array,
    pg_type::PostgresType,
    statement::Statement,
    unnest::{UnnestAliased, UnnestColumn, unnest_call},
};
use sea_query::{Expr, ExprTrait, Iden, IntoIden, Query, TableRef, UpdateStatement};

/// The alias of the flushed deltas, `delta`.
#[derive(Iden, Clone, Copy)]
#[iden = "delta"]
struct Delta;

/// A flushed delta's columns.
#[derive(Iden, Clone, Copy)]
enum DeltaColumn {
    /// The Context's source URL.
    Url,
    /// The hits to add.
    Count,
}

/// The columns of the unnested deltas, in argument order.
fn delta_columns() -> [UnnestColumn; 2] {
    [
        UnnestColumn::new(DeltaColumn::Url, PostgresType::TextArray),
        UnnestColumn::new(DeltaColumn::Count, PostgresType::BigintArray),
    ]
}

/// The update adding every delta's hits to its Context; its `UNNEST` source
/// still needs the column-list alias [`delta_columns`] names.
fn delta_update(urls: &[&str], counts: Vec<i64>) -> UpdateStatement {
    let [url, count] = &delta_columns();
    let deltas = unnest_call([(url, text_array(urls)), (count, Expr::val(counts))]);
    Query::update()
        .table(JsonldContexts::Table)
        .value(
            JsonldContexts::NumberOfHits,
            Expr::col((JsonldContexts::Table, JsonldContexts::NumberOfHits)).add(Expr::col((Delta, DeltaColumn::Count))),
        )
        .value(JsonldContexts::LastUsage, now())
        .from(TableRef::FunctionCall(deltas, Delta.into_iden()))
        .and_where(Expr::col((JsonldContexts::Table, JsonldContexts::Url)).equals((Delta, DeltaColumn::Url)))
        .to_owned()
}

impl ContextHitStore for JsonLdContextRepository {
    fn flush_hits<'a>(&'a self, deltas: &'a [HitDelta]) -> ContextHitStoreFuture<'a> {
        Box::pin(async move {
            if deltas.is_empty() {
                return Ok(());
            }
            let urls: Vec<&str> = deltas.iter().map(|delta| delta.url.as_str()).collect();
            let counts = deltas
                .iter()
                .map(|delta| i64::try_from(delta.count).map_err(|error| ContextRegistryError::from(PersistenceFailure::Unencodable.record(&error))))
                .collect::<Result<Vec<i64>, ContextRegistryError>>()?;
            UnnestAliased::new(delta_update(&urls, counts))
                .with_columns(Delta, &delta_columns())
                .to_query()
                .execute(&self.pool)
                .await
                .map_err(|error| ContextRegistryError::from(persistence_failure(&error)))?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::{hit_store::delta_update, repo::JsonLdContextRepository};
    use andromeda_context_registry::{error::ContextRegistryError, hit_store::ContextHitStore};
    use sea_query::PostgresQueryBuilder;
    use sqlx::postgres::PgPool;

    #[tokio::test]
    async fn empty_hit_batch_does_not_open_a_database_connection() -> Result<(), ContextRegistryError> {
        let pool = PgPool::connect_lazy("postgres://postgres:postgres@localhost/andromeda").expect("a valid test fixture");
        let repository = JsonLdContextRepository::new(&pool);
        repository.flush_hits(&[]).await
    }

    #[test]
    fn the_hits_of_every_delta_are_added_to_the_context_with_the_same_url_in_one_statement() {
        let (sql, values) = delta_update(&["https://example.com/a"], vec![3]).build(PostgresQueryBuilder);
        assert!(
            sql.starts_with(
                r#"UPDATE "jsonld_contexts" SET "number_of_hits" = "jsonld_contexts"."number_of_hits" + "delta"."count", "last_usage" = NOW() FROM UNNEST("#
            ),
            "{sql}"
        );
        assert!(sql.contains(r#") AS "delta" WHERE "jsonld_contexts"."url" = "delta"."url""#), "{sql}");
        pretty_assertions::assert_eq!(values.0.len(), 2, "the urls and the counts are one bound array each: {sql}");
    }
}
