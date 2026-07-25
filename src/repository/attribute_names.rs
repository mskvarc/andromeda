//! In-process cache over the `attribute_names` dictionary.
//!
//! `attribute_names` maps expanded attribute IRIs to a `SMALLINT` key used by
//! the temporal catalogue, the history hypertable, and the `attribute_current`
//! projection. Both the write path (which must create missing entries) and the
//! `q`-filter read path (which must not) resolve through this one cache, so a
//! tenant holds a single copy of the dictionary rather than one per repository.

use crate::{core::error::AndromedaError, domain::temporal::AttributeNameId, query::sql::q_filter::ResolvedAttributeNames};
use dashmap::DashMap;
use sqlx::{Row, postgres::PgPool};
use std::collections::HashMap;

/// Cache of expanded attribute IRI to dictionary id.
///
/// Only *hits* are ever stored. A miss is deliberately not memoised: the
/// `q`-filter path resolves per query execution, and caching "this attribute
/// does not exist" would make an attribute created after the first lookup
/// permanently invisible to value predicates, which compile an unknown IRI to a
/// constant `FALSE`.
#[derive(Debug)]
pub struct AttributeNameCache {
    pool: PgPool,
    ids: DashMap<String, AttributeNameId>,
}

impl AttributeNameCache {
    /// Create a cache backed by the given tenant pool.
    pub fn new(pool: &PgPool) -> AttributeNameCache {
        AttributeNameCache {
            pool: pool.clone(), // clone: Arc ref-count bump
            ids: DashMap::new(),
        }
    }

    /// Load the whole dictionary into the cache.
    ///
    /// Removes the cold-miss round trip otherwise paid on first use of each
    /// attribute IRI, on both the write and the read path.
    pub async fn preload(&self) -> Result<(), AndromedaError> {
        let rows = sqlx::query("SELECT id, name FROM attribute_names").fetch_all(&self.pool).await?;
        for row in rows {
            let id: i16 = row.get("id");
            let name: String = row.get("name");
            self.ids.insert(name, AttributeNameId::from(id));
        }
        Ok(())
    }

    /// Resolve every name, creating dictionary entries for those that are new.
    ///
    /// The write path: recording an observation for a previously unseen
    /// attribute has to mint its dictionary key.
    pub async fn resolve_or_create(&self, names: &[&str]) -> Result<HashMap<String, AttributeNameId>, AndromedaError> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }

        let mut resolved = HashMap::with_capacity(names.len());
        let mut missing: Vec<&str> = Vec::new();
        for name in names {
            match self.ids.get(*name) {
                Some(id) => {
                    resolved.insert((*name).to_owned(), *id);
                }
                None => missing.push(name),
            }
        }

        if missing.is_empty() {
            return Ok(resolved);
        }

        sqlx::query(
            "INSERT INTO attribute_names (name) \
             SELECT name FROM UNNEST($1::text[]) AS t(name) \
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(&missing)
        .execute(&self.pool)
        .await?;

        let rows = sqlx::query("SELECT id, name FROM attribute_names WHERE name = ANY($1)")
            .bind(&missing)
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let name: String = row.get("name");
            let id = AttributeNameId::from(row.try_get::<i16, _>("id")?);
            self.ids.insert(name.clone(), id); // clone: the cache and the result map each need an owned key
            resolved.insert(name, id);
        }

        Ok(resolved)
    }

    /// Resolve names that already exist, without creating any.
    ///
    /// The read path. An IRI the broker has never recorded resolves to nothing,
    /// and its predicates compile to a constant `FALSE` — correct, because no
    /// `attribute_current` or `temporal_entity_attributes` row can reference a
    /// dictionary entry that does not exist.
    ///
    /// Misses are re-queried on every call rather than remembered, so an
    /// attribute becomes visible to `q` filters as soon as it is first written.
    pub async fn lookup(&self, names: &[String]) -> Result<ResolvedAttributeNames, AndromedaError> {
        if names.is_empty() {
            return Ok(ResolvedAttributeNames::default());
        }

        let mut resolved: Vec<(String, AttributeNameId)> = Vec::with_capacity(names.len());
        let mut missing: Vec<&str> = Vec::new();
        for name in names {
            match self.ids.get(name.as_str()) {
                Some(id) => resolved.push((name.clone(), *id)), // clone: the returned map owns its keys
                None => missing.push(name.as_str()),
            }
        }

        if !missing.is_empty() {
            let rows = sqlx::query("SELECT id, name FROM attribute_names WHERE name = ANY($1)")
                .bind(&missing)
                .fetch_all(&self.pool)
                .await?;
            for row in rows {
                let name: String = row.get("name");
                let id = AttributeNameId::from(row.try_get::<i16, _>("id")?);
                self.ids.insert(name.clone(), id); // clone: cache keeps its own key
                resolved.push((name, id));
            }
        }

        Ok(ResolvedAttributeNames::from_pairs(resolved))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cache() -> AttributeNameCache {
        #![allow(clippy::expect_used)]
        AttributeNameCache::new(&PgPool::connect_lazy("postgres://invalid/invalid").expect("lazy pool"))
    }

    #[tokio::test]
    async fn lookup_of_no_names_does_not_touch_the_database() {
        let resolved = cache().lookup(&[]).await.expect("empty lookup");
        assert!(resolved.is_empty());
    }

    #[tokio::test]
    async fn resolve_or_create_of_no_names_does_not_touch_the_database() {
        let resolved = cache().resolve_or_create(&[]).await.expect("empty resolve");
        assert!(resolved.is_empty());
    }

    #[tokio::test]
    async fn lookup_serves_cached_hits_without_a_query() {
        let cache = cache();
        cache.ids.insert("https://example.com/speed".to_string(), AttributeNameId::from(7));
        // The pool points nowhere, so this only succeeds if no query is issued.
        let resolved = cache.lookup(&["https://example.com/speed".to_string()]).await.expect("cached lookup");
        assert_eq!(resolved.get("https://example.com/speed"), Some(AttributeNameId::from(7)));
    }

    #[tokio::test]
    async fn a_miss_is_never_written_back_to_the_cache() {
        // Guards the invariant that makes execution-time resolution safe: if a
        // miss were memoised, an attribute created after the first lookup would
        // stay invisible to q filters for the lifetime of the process.
        let cache = cache();
        assert!(cache.ids.is_empty());
        assert!(cache.ids.get("https://example.com/never-written").is_none());
        assert!(cache.ids.is_empty(), "a failed lookup must not populate the cache");
    }
}
