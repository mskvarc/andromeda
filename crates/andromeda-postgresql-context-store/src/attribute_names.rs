//! In-process cache over the `attribute_names` dictionary.
//!
//! `attribute_names` maps expanded attribute IRIs to a `SMALLINT` key used by
//! the temporal catalogue, the history hypertable, and the `attribute_current`
//! projection. Both the write path (which must create missing entries) and the
//! `q`-filter read path (which must not) resolve through this one cache, so a
//! tenant holds a single copy of the dictionary rather than one per repository.

use crate::error::ContextStoreError;
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_query::{
    attribute_name_id::AttributeNameId,
    attribute_name_lookup::{AttributeNameLookup, AttributeNameLookupError},
    q_predicate::attribute_names::ResolvedAttributeNames,
};
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
    /// The tenant database pool.
    pool: PgPool,
    /// Cached mappings from expanded IRI text to dictionary id.
    ids: DashMap<AttributeName, AttributeNameId>,
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
    pub async fn preload(&self) -> Result<(), ContextStoreError> {
        let rows = sqlx::query("SELECT id, name FROM attribute_names").fetch_all(&self.pool).await?;
        for row in rows {
            let id: i16 = row.get("id");
            let name_raw: String = row.get("name");
            let name = name_raw
                .parse::<AttributeName>()
                .map_err(|error| ContextStoreError::Internal(format!("Invalid attribute_names.name '{name_raw}': {error}")))?;
            self.ids.insert(name, AttributeNameId::from(id));
        }
        Ok(())
    }

    /// Resolve every name, creating dictionary entries for those that are new.
    ///
    /// The write path: recording an observation for a previously unseen
    /// attribute has to mint its dictionary key.
    pub async fn resolve_or_create(&self, names: &[AttributeName]) -> Result<HashMap<AttributeName, AttributeNameId>, ContextStoreError> {
        if names.is_empty() {
            return Ok(HashMap::new());
        }

        let mut resolved = HashMap::with_capacity(names.len());
        let mut missing: Vec<&AttributeName> = Vec::new();
        for name in names {
            match self.ids.get(name) {
                Some(id) => {
                    resolved.insert(name.clone(), *id); // clone: the result map owns the validated name.
                }
                None => missing.push(name),
            }
        }

        if missing.is_empty() {
            return Ok(resolved);
        }

        let missing_text: Vec<&str> = missing.iter().map(|name| name.as_str()).collect();
        sqlx::query(
            "INSERT INTO attribute_names (name) \
             SELECT name FROM UNNEST($1::text[]) AS t(name) \
             ON CONFLICT (name) DO NOTHING",
        )
        .bind(&missing_text)
        .execute(&self.pool)
        .await?;

        let rows = sqlx::query("SELECT id, name FROM attribute_names WHERE name = ANY($1)")
            .bind(&missing_text)
            .fetch_all(&self.pool)
            .await?;

        for row in rows {
            let name_raw: String = row.get("name");
            let name = name_raw
                .parse::<AttributeName>()
                .map_err(|error| ContextStoreError::Internal(format!("Invalid attribute_names.name '{name_raw}': {error}")))?;
            let id = AttributeNameId::from(row.try_get::<i16, _>("id")?);
            self.ids.insert(name.clone(), id); // clone: the cache and the result map each need an owned key
            resolved.insert(name, id);
        }

        Ok(resolved)
    }

    /// Resolve names that already exist, without creating any.
    ///
    /// The read path. An IRI the broker has never recorded resolves to nothing,
    /// and its predicates compile to a constant `FALSE` -- correct, because no
    /// `attribute_current` or `temporal_entity_attributes` row can reference a
    /// dictionary entry that does not exist.
    ///
    /// Misses are re-queried on every call rather than remembered, so an
    /// attribute becomes visible to `q` filters as soon as it is first written.
    pub async fn lookup(&self, names: &[AttributeName]) -> Result<ResolvedAttributeNames, ContextStoreError> {
        if names.is_empty() {
            return Ok(ResolvedAttributeNames::default());
        }

        let mut resolved: Vec<(AttributeName, AttributeNameId)> = Vec::with_capacity(names.len());
        let mut missing: Vec<&AttributeName> = Vec::new();
        for name in names {
            match self.ids.get(name) {
                // clone: the returned map owns its keys; the name is a `CompactString`
                Some(id) => resolved.push((name.clone(), *id)),
                None => missing.push(name),
            }
        }

        if !missing.is_empty() {
            let missing_text: Vec<&str> = missing.iter().map(|name| name.as_str()).collect();
            let rows = sqlx::query("SELECT id, name FROM attribute_names WHERE name = ANY($1)")
                .bind(&missing_text)
                .fetch_all(&self.pool)
                .await?;
            // The row's `name` is the text that was bound, so each hit is matched
            // back to the typed name that produced it rather than reparsed: an
            // `AttributeName` that reached the dictionary was valid on the way in.
            let mut ids_by_name: HashMap<AttributeName, AttributeNameId> = HashMap::with_capacity(rows.len());
            for row in rows {
                let name_raw: String = row.get("name");
                let name = name_raw
                    .parse::<AttributeName>()
                    .map_err(|error| ContextStoreError::Internal(format!("Invalid attribute_names.name '{name_raw}': {error}")))?;
                let id = AttributeNameId::from(row.try_get::<i16, _>("id")?);
                self.ids.insert(name.clone(), id); // clone: cache keeps its own key
                ids_by_name.insert(name, id);
            }
            for name in missing {
                if let Some(id) = ids_by_name.get(name) {
                    // clone: as above
                    resolved.push((name.clone(), *id));
                }
            }
        }

        Ok(ResolvedAttributeNames::from_pairs(resolved))
    }
}

#[async_trait::async_trait]
impl AttributeNameLookup for AttributeNameCache {
    async fn lookup(&self, names: &[AttributeName]) -> Result<ResolvedAttributeNames, AttributeNameLookupError> {
        AttributeNameCache::lookup(self, names)
            .await
            .map_err(|e| AttributeNameLookupError::new(e.to_string()))
    }

    async fn resolve_or_create(&self, names: &[AttributeName]) -> Result<HashMap<AttributeName, AttributeNameId>, AttributeNameLookupError> {
        AttributeNameCache::resolve_or_create(self, names)
            .await
            .map_err(|e| AttributeNameLookupError::new(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgConnectOptions;

    /// A cache over a pool that can never connect.
    ///
    /// Every test below asserts that some path issues no statement, and an
    /// unroutable host is what turns a violation into a failure instead of a
    /// silent round trip to whatever database the environment points at.
    /// `connect_lazy_with` does not dial on construction, so building it is
    /// infallible.
    fn cache() -> AttributeNameCache {
        AttributeNameCache::new(&PgPool::connect_lazy_with(PgConnectOptions::new().host("invalid.invalid").port(1)))
    }

    /// The one expanded attribute name these tests resolve.
    fn speed() -> AttributeName {
        "https://example.com/speed".parse().expect("valid attribute name")
    }

    #[tokio::test]
    async fn lookup_of_no_names_does_not_touch_the_database() {
        let resolved = cache().lookup(&[]).await;
        assert!(
            resolved.is_ok_and(|names| names.is_empty()),
            "an empty lookup must resolve to nothing without a query"
        );
    }

    #[tokio::test]
    async fn resolve_or_create_of_no_names_does_not_touch_the_database() {
        let resolved = cache().resolve_or_create(&[]).await;
        assert!(
            resolved.is_ok_and(|names| names.is_empty()),
            "an empty resolve must create nothing without a query"
        );
    }

    #[tokio::test]
    async fn lookup_serves_cached_hits_without_a_query() {
        let cache = cache();
        cache.ids.insert(speed(), AttributeNameId::from(7));
        // The pool points nowhere, so this only succeeds if no query is issued.
        let resolved = cache.lookup(&[speed()]).await.ok().and_then(|names| names.get(&speed()));
        assert_eq!(resolved, Some(AttributeNameId::from(7)));
    }

    #[tokio::test]
    async fn a_miss_is_never_written_back_to_the_cache() {
        // Guards the invariant that makes execution-time resolution safe: if a
        // miss were memoised, an attribute created after the first lookup would
        // stay invisible to q filters for the lifetime of the process.
        let cache = cache();
        assert!(cache.ids.is_empty());
        let never_written = "https://example.com/never-written".parse::<AttributeName>().expect("valid attribute name");
        assert!(cache.ids.get(&never_written).is_none());
        assert!(cache.ids.is_empty(), "a failed lookup must not populate the cache");
    }
}
