//! In-process cache over one name dictionary.
//!
//! `attribute_names` maps expanded attribute IRIs to the `INTEGER` key used by
//! the temporal catalogue, the history hypertable, and the
//! `current_attributes` rows; `entity_type_names` maps expanded
//! type IRIs to the key the `entity_type_name_keys` arrays carry. Both the write path
//! (which must create missing entries) and the read path (which must not)
//! resolve through this one cache, so a tenant holds a single copy of each
//! dictionary rather than one per repository. The cache is generic over the
//! [`DictionaryRelation`] so the two dictionaries share one implementation
//! and one set of statements.

use crate::{
    bounded_keys::{bounded_keys, preload_row_limit},
    dictionary_relation::DictionaryRelation,
    dictionary_rows::{CachePublication, decode, held_publication},
    dictionary_statements::{held_select_query, name_insert_query, name_select_query, preload_query},
    name_cache_config::NameCacheConfig,
    name_lookup::{NameLookup, NameLookupError},
    schema_epoch::{EPOCH_CHECK_INTERVAL, UNKNOWN_EPOCH, now_millis, read_schema_epoch},
};
use andromeda_postgresql_query::dictionary::{interned_name::InternedName, resolved_names::ResolvedNames};
use moka::sync::Cache;
use sea_query::PostgresQueryBuilder;
use sea_query_sqlx::SqlxBinder;
use sqlx::{AssertSqlSafe, Executor, PgConnection, Postgres, postgres::PgPool};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::atomic::{AtomicI64, AtomicU64, Ordering},
    time::Duration,
};

/// Cache of expanded IRI to dictionary key for one dictionary.
///
/// The cache is bounded by the byte budget and idle timeout of its
/// [`NameCacheConfig`]; the dictionary behind it is not. A name the cache has
/// dropped is a miss like any other and is read back from the table, so the
/// budget bounds this process's memory without ever refusing a name.
///
/// Only *committed hits* are ever stored. A miss is deliberately not memoised:
/// the read path resolves per query execution, and caching "this name does not
/// exist" would make a name created after the first lookup permanently
/// invisible to predicates, which compile an unknown IRI to a constant
/// `FALSE`. A mapping minted inside a caller's transaction is likewise not
/// stored (see [`CachePublication`]); it is published by the first read that
/// provably sees a committed snapshot, which is either a pool-side lookup or a
/// minting resolution made by a transaction that has not written yet.
#[derive(Debug)]
pub struct NameCache<N: DictionaryRelation> {
    /// The tenant database pool, used only by reads that need a committed
    /// snapshot. Minting runs on the caller's own connection.
    pool: PgPool,
    /// Cached mappings from expanded IRI text to dictionary key, bounded by
    /// the configured byte budget and idle timeout.
    keys: Cache<N, N::Key>,
    /// How many rows a preload reads at most, derived from the byte budget.
    preload_rows: u64,
    /// The tenant schema stamp (`schema_metadata.applied_at`, microseconds) this
    /// cache was warmed against, or [`UNKNOWN_EPOCH`] before it has been
    /// established. A fresh migration re-stamps it, so it advances whenever the
    /// schema was dropped and recreated, whether by dropping the database or
    /// the schema, which is exactly when every cached key goes stale. See
    /// [`NameCache::revalidate_epoch`].
    schema_epoch: AtomicI64,
    /// Milliseconds since [`EPOCH`] at the last schema-stamp probe, throttling
    /// the staleness check to at most once per [`EPOCH_CHECK_INTERVAL`].
    epoch_checked_at: AtomicU64,
}

impl<N: DictionaryRelation> NameCache<N> {
    /// Create a cache backed by the given tenant pool, holding as much of the
    /// dictionary as `config` budgets for.
    #[must_use]
    pub fn new(pool: &PgPool, config: &NameCacheConfig) -> NameCache<N> {
        NameCache {
            pool: pool.clone(), // clone: Arc ref-count bump
            keys: bounded_keys(config),
            preload_rows: preload_row_limit(config),
            schema_epoch: AtomicI64::new(UNKNOWN_EPOCH),
            // Seeded to now, not zero: a cache is inside its check interval the
            // moment it is built, so its first resolutions serve cached hits
            // without a catalogue probe until the interval first elapses.
            epoch_checked_at: AtomicU64::new(now_millis()),
        }
    }

    /// Load as much of the dictionary as the budget holds into the cache,
    /// oldest names first.
    ///
    /// Removes the cold-miss round trip otherwise paid on first use of each
    /// IRI, on both the write and the read path. A dictionary larger than the
    /// budget is read only up to what could fit; the rest is resolved from the
    /// table on first use, as any miss is.
    ///
    /// # Errors
    ///
    /// Returns [`NameLookupError::Statement`] when the read fails and one of
    /// the `Stored*` variants when a stored row is unreadable.
    pub async fn preload(&self) -> Result<(), NameLookupError> {
        let (sql, values) = preload_query::<N>(self.preload_rows).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        self.absorb(&decode::<N>(&rows)?, CachePublication::Publish);
        Ok(())
    }

    /// Answer from the cache what it can, and report what it cannot.
    fn probe<'a>(&self, names: &'a [N]) -> (Vec<(N, N::Key)>, Vec<&'a N>) {
        let mut resolved = Vec::with_capacity(names.len());
        let mut missing = Vec::new();
        for name in names {
            match self.keys.get(name) {
                // clone: the answer owns its keys; the name is a `CompactString`
                Some(id) => resolved.push((name.clone(), id)),
                None => missing.push(name),
            }
        }
        (resolved, missing)
    }

    /// Record what one read resolved, publishing it only when the read saw a
    /// committed snapshot.
    fn absorb(&self, resolved: &[(N, N::Key)], publication: CachePublication) {
        match publication {
            CachePublication::Publish => {
                for (name, id) in resolved {
                    self.keys.insert(name.clone(), *id); // clone: the cache keeps its own key
                }
            }
            CachePublication::Withhold => {}
        }
    }

    /// Adopt a schema stamp read once at warm-up as this cache's baseline.
    ///
    /// Called after construction so that a later drop-and-recreate is a change
    /// from a known baseline rather than the first value ever observed: a cache
    /// that only learned its stamp lazily could adopt the recreated schema's
    /// stamp as its baseline while still holding the previous generation's keys.
    /// The stamp is read once by
    /// [`read_established_epoch`](crate::schema_epoch::read_established_epoch) and handed to every
    /// dictionary the tenant opens, so the caches share one catalogue read. A
    /// `None` (a schema migrated but not yet stamped) leaves the sentinel in
    /// place and only records that the baseline probe ran.
    pub fn adopt_epoch(&self, epoch: Option<i64>) {
        if let Some(epoch) = epoch {
            self.schema_epoch.store(epoch, Ordering::Relaxed);
        }
        self.epoch_checked_at.store(now_millis(), Ordering::Relaxed);
    }

    /// Re-read the tenant schema stamp when the throttle interval has elapsed,
    /// discarding every cached key if it changed.
    ///
    /// A tenant database dropped and recreated under the same name comes back
    /// with fresh `IDENTITY` sequences, so every name->key mapping this cache
    /// published against the previous generation now points at a dictionary row
    /// that does not exist, and a write that trusts one raises
    /// `fk_temporal_attribute_streams_attribute_name`. The broker performs
    /// no such drop itself and nothing evicts a runtime whose registry row still
    /// stands, so only operator surgery opens this window; the check closes it
    /// by clearing the cache the moment the schema stamp it was warmed against
    /// changes.
    ///
    /// The check is throttled so a steady-state resolution that touches nothing
    /// but cached hits still pays at most one catalogue read per
    /// [`EPOCH_CHECK_INTERVAL`], and it runs on the caller's executor so a
    /// minting resolution never reaches into the pool for a second connection
    /// while its transaction holds one.
    async fn revalidate_epoch<'e, E: Executor<'e, Database = Postgres>>(&self, executor: E) -> Result<(), NameLookupError> {
        let now = now_millis();
        let last = self.epoch_checked_at.load(Ordering::Relaxed);
        if Duration::from_millis(now.saturating_sub(last)) < EPOCH_CHECK_INTERVAL {
            return Ok(());
        }
        // Stamp the check before issuing the probe so concurrent resolutions
        // inside the same interval do not each pay for it.
        self.epoch_checked_at.store(now, Ordering::Relaxed);
        // A schema migrated but not yet stamped answers nothing about staleness;
        // leave the cache and the baseline as they are until a stamp appears.
        let Some(observed) = read_schema_epoch(executor).await? else {
            return Ok(());
        };
        let previous = self.schema_epoch.swap(observed, Ordering::Relaxed);
        if previous != UNKNOWN_EPOCH && previous != observed {
            self.keys.invalidate_all();
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl<N: DictionaryRelation> NameLookup<N> for NameCache<N> {
    /// Resolve names that already exist, without creating any.
    ///
    /// Misses are re-queried on every call rather than remembered, so a name
    /// becomes visible to predicates as soon as it is first written.
    async fn lookup(&self, names: &[N]) -> Result<ResolvedNames<N>, NameLookupError> {
        // The read path resolves on the pool, so it revalidates there too.
        self.revalidate_epoch(&self.pool).await?;
        let (mut resolved, missing) = self.probe(names);
        if missing.is_empty() {
            return Ok(ResolvedNames::from_pairs(resolved));
        }
        let wanted = distinct_text(&missing);
        // The pool rather than a caller's connection: publication to the cache
        // is only sound from a committed snapshot, which is what this read sees.
        let (sql, values) = name_select_query::<N>(&wanted).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        let found = decode::<N>(&rows)?;
        self.absorb(&found, CachePublication::Publish);
        resolved.extend(found);
        Ok(ResolvedNames::from_pairs(resolved))
    }

    /// Resolve every name in the caller's transaction, minting the new ones.
    ///
    /// Every statement runs on `connection`, which is the connection the
    /// caller's transaction holds. Two consequences follow, and both are the
    /// point of the method taking it: the dictionary rows commit and roll back
    /// with the write that needed them, and no phase of a commit reaches into
    /// the pool for a second connection while already holding one.
    ///
    /// A cache miss is not evidence that a name is new. The cache only ever
    /// holds what a *committed* read published, and only as much of that as
    /// its budget allows, so every name minted inside a transaction, every name
    /// minted before this process started and every name the cache has since
    /// dropped misses. The dictionary is therefore asked which of the missing
    /// names it already holds, and only the rest are minted.
    async fn resolve_or_create(&self, connection: &mut PgConnection, names: &[N]) -> Result<HashMap<N, N::Key>, NameLookupError> {
        // On the caller's own connection, never the pool: a minting resolution
        // must not hold a second connection while its transaction holds one.
        self.revalidate_epoch(&mut *connection).await?;
        let (mut resolved, missing) = self.probe(names);
        if missing.is_empty() {
            return Ok(resolved.into_iter().collect());
        }
        let wanted = distinct_text(&missing);

        // The caller's connection rather than the pool: a name an earlier phase
        // of this same transaction minted is uncommitted, and a read that could
        // not see it would count it as new a second time.
        let (held_sql, held_values) = held_select_query::<N>(&wanted).build_sqlx(PostgresQueryBuilder);
        let held_rows = sqlx::query_with(AssertSqlSafe(held_sql), held_values).fetch_all(&mut *connection).await?;
        let held = decode::<N>(&held_rows)?;
        let unheld = names_not_yet_held(&wanted, &held);
        // What the dictionary already held may still be rolled back by the
        // caller's transaction when that transaction wrote it, so by default the
        // mappings answer this caller and go no further (see
        // [`CachePublication`]). The one case that is provably safe is a
        // transaction that has written nothing yet: it holds no transaction id,
        // so every row its snapshot shows was committed by someone else, and
        // publishing those is exactly what lets a write path that resolves its
        // names before its first statement stop asking the dictionary once a
        // name has been committed by any writer.
        self.absorb(&held, held_publication(&held_rows)?);
        resolved.extend(held);
        if unheld.is_empty() {
            return Ok(resolved.into_iter().collect());
        }

        let (insert_sql, insert_values) = name_insert_query::<N>(&unheld)
            .map_err(NameLookupError::StatementBuild)?
            .build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(AssertSqlSafe(insert_sql), insert_values).execute(&mut *connection).await?;
        let (minted_sql, minted_values) = name_select_query::<N>(&unheld).build_sqlx(PostgresQueryBuilder);
        let minted = decode::<N>(&sqlx::query_with(AssertSqlSafe(minted_sql), minted_values).fetch_all(&mut *connection).await?)?;
        self.absorb(&minted, CachePublication::Withhold);
        resolved.extend(minted);
        Ok(resolved.into_iter().collect())
    }
}

/// The text of every distinct name a read has to ask the dictionary for.
///
/// Sorted, so that two transactions minting overlapping sets of new names take
/// the unique index's conflict waits in the same order and neither can hold what
/// the other is waiting for. Deduplicated, so that one name repeated across an
/// entity's members is asked for and minted once.
fn distinct_text<'a, N: InternedName>(missing: &[&'a N]) -> Vec<&'a str> {
    let mut text: Vec<&'a str> = missing.iter().map(|name| InternedName::as_str(*name)).collect();
    text.sort_unstable();
    text.dedup();
    text
}

/// The names of a batch the dictionary does not hold yet, in the order the
/// batch was sorted into.
///
/// These are the names a minting resolution inserts: a name the dictionary
/// already holds creates nothing, whether or not this process had it cached.
fn names_not_yet_held<'a, N: InternedName>(wanted: &[&'a str], held: &[(N, N::Key)]) -> Vec<&'a str> {
    let held: HashSet<&str> = held.iter().map(|(name, _)| InternedName::as_str(name)).collect();
    wanted.iter().filter(|name| !held.contains(*name)).copied().collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        dictionary_relation::DictionaryRelation,
        dictionary_rows::CachePublication,
        dictionary_statements::{SNAPSHOT_IS_COMMITTED_COLUMN, held_select_query, name_insert_query, name_select_query, preload_query},
        name_cache::{NameCache, distinct_text, names_not_yet_held},
        name_cache_config::NameCacheConfig,
        name_lookup::NameLookup,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_query::{attribute_name_key::AttributeNameKey, dictionary::interned_name::InternedName, entity_type_name_key::EntityTypeNameKey};
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;
    use sqlx::postgres::{PgConnectOptions, PgPool};

    /// A cache over a pool that can never connect.
    ///
    /// Every test below asserts that some path issues no statement, and an
    /// unroutable host is what turns a violation into a failure instead of a
    /// silent round trip to whatever database the environment points at.
    /// `connect_lazy_with` does not dial on construction, so building it is
    /// infallible.
    fn cache<N: DictionaryRelation>() -> NameCache<N> {
        NameCache::new(
            &PgPool::connect_lazy_with(PgConnectOptions::new().host("invalid.invalid").port(1)),
            &NameCacheConfig::default(),
        )
    }

    /// The behaviour every dictionary shares, stated once and instantiated per
    /// name type: the two dictionaries differ in their relation and their key
    /// type and in nothing else.
    macro_rules! name_cache_tests {
        ($module:ident, $name:ty, $key:ty, $first:literal, $second:literal, $table:literal) => {
            mod $module {
                use super::*;
                use pretty_assertions::assert_eq;

                /// The one interned name these tests resolve.
                fn first() -> $name {
                    $first.parse().expect("valid name")
                }

                /// A second interned name, ordered before the first.
                fn second() -> $name {
                    $second.parse().expect("valid name")
                }

                #[tokio::test]
                async fn lookup_of_no_names_does_not_touch_the_database() {
                    let resolved = cache::<$name>().lookup(&[]).await;
                    assert!(
                        resolved.is_ok_and(|names| names.is_empty()),
                        "an empty lookup must resolve to nothing without a query"
                    );
                }

                #[tokio::test]
                async fn lookup_serves_cached_hits_without_a_query() {
                    let cache = cache::<$name>();
                    cache.keys.insert(first(), <$key>::from(7));
                    // The pool points nowhere, so this only succeeds if no query is issued.
                    let resolved = cache.lookup(&[first()]).await.ok().and_then(|names| names.get(&first()));
                    assert_eq!(resolved, Some(<$key>::from(7)));
                }

                /// A resolution with nothing left to find issues no statement at all,
                /// which is what lets a fully cached batch mint names without a round
                /// trip. The empty request is the degenerate case of the same rule.
                #[tokio::test]
                async fn a_request_whose_names_are_all_cached_leaves_nothing_to_mint() {
                    let cache = cache::<$name>();
                    cache.keys.insert(first(), <$key>::from(7));
                    let asked = [first()];
                    let (resolved, missing) = cache.probe(&asked);
                    assert_eq!(resolved, vec![(first(), <$key>::from(7))]);
                    assert!(missing.is_empty(), "a cached name is never asked of the dictionary again");
                    let (empty, nothing) = cache.probe(&[]);
                    assert!(empty.is_empty() && nothing.is_empty(), "an empty request resolves to nothing");
                }

                #[tokio::test]
                async fn a_miss_is_never_written_back_to_the_cache() {
                    // Guards the invariant that makes execution-time resolution safe: if
                    // a miss were memoised, a name created after the first lookup would
                    // stay invisible to predicates for the lifetime of the process.
                    let cache = cache::<$name>();
                    let asked = ["https://example.com/never-written".parse::<$name>().expect("valid name")];
                    let (resolved, missing) = cache.probe(&asked);
                    assert!(resolved.is_empty());
                    assert_eq!(missing.len(), 1);
                    assert!(!cache.keys.contains_key(&asked[0]), "a failed lookup must not populate the cache");
                }

                /// The rule the transaction-aware write path depends on: a name minted
                /// inside the caller's transaction is handed back to that caller and to
                /// nobody else, because the transaction may still roll it away.
                #[tokio::test]
                async fn a_resolution_made_inside_a_transaction_is_not_published_to_the_cache() {
                    let cache = cache::<$name>();
                    let minted = [(first(), <$key>::from(7))];
                    cache.absorb(&minted, CachePublication::Withhold);
                    assert!(
                        !cache.keys.contains_key(&first()),
                        "an uncommitted mapping must not answer any later resolution in this process"
                    );
                }

                /// The counterpart: a read that saw a committed snapshot is exactly what
                /// may publish, and it is the only thing that ever fills the cache.
                #[tokio::test]
                async fn a_committed_read_publishes_what_it_resolved_to_the_cache() {
                    let cache = cache::<$name>();
                    cache.absorb(&[(first(), <$key>::from(7))], CachePublication::Publish);
                    assert_eq!(cache.keys.get(&first()), Some(<$key>::from(7)));
                }

                /// Eviction is safe because a dropped name is a miss like any
                /// other: it is asked of the dictionary again rather than
                /// answered wrongly or refused.
                #[tokio::test]
                async fn a_name_the_cache_has_dropped_is_asked_of_the_dictionary_again() {
                    let cache = cache::<$name>();
                    cache.absorb(&[(first(), <$key>::from(7))], CachePublication::Publish);
                    cache.keys.invalidate(&first());
                    let asked = [first()];
                    let (resolved, missing) = cache.probe(&asked);
                    assert!(resolved.is_empty(), "a dropped name has no cached key to answer with");
                    assert_eq!(missing, vec![&first()]);
                }

                /// Ordering is what keeps two concurrent writers from deadlocking on the
                /// unique index, and deduplication is what keeps one name repeated across
                /// an entity's members from being asked for and minted twice.
                #[test]
                fn names_are_minted_as_one_sorted_distinct_batch() {
                    let (first, second) = (first(), second());
                    let batch = distinct_text(&[&first, &second, &first]);
                    assert_eq!(batch, vec![InternedName::as_str(&second), InternedName::as_str(&first)]);
                }

                /// Another transaction may mint the same name between this one's read
                /// and its insert, so the insert states what happens then rather than
                /// failing.
                #[test]
                fn minting_leaves_a_name_another_writer_already_minted_alone() {
                    let (sql, values) = name_insert_query::<$name>(&[$first]).expect("arity").build_sqlx(PostgresQueryBuilder);
                    assert!(sql.contains(concat!("INSERT INTO \"", $table, "\" (\"name\")")), "{sql}");
                    assert!(
                        sql.contains("SELECT \"t\" FROM UNNEST($1::text[]) AS \"t\""),
                        "the scalar table-function result is selected through its table alias: {sql}"
                    );
                    assert!(sql.contains("ON CONFLICT (\"name\") DO NOTHING"), "{sql}");
                    assert_eq!(values.0.0.len(), 1, "{sql}");
                }

                /// A preload reads the oldest names first and stops at what the
                /// budget could hold, so a dictionary far larger than the cache is
                /// never read whole at tenant startup.
                #[test]
                fn a_preload_reads_the_oldest_names_up_to_the_row_limit() {
                    let sql = preload_query::<$name>(100).to_string(PostgresQueryBuilder);
                    assert!(sql.contains(concat!("FROM \"", $table, "\" ORDER BY")), "{sql}");
                    assert!(sql.ends_with("ASC LIMIT 100"), "{sql}");
                }

                /// One array bind rather than a placeholder per name, which is what
                /// keeps a large batch inside the protocol's parameter space.
                #[test]
                fn a_batch_of_names_is_read_back_through_a_single_array_bind() {
                    let (sql, values) = name_select_query::<$name>(&[$first, $second]).build_sqlx(PostgresQueryBuilder);
                    assert!(sql.contains(concat!("FROM \"", $table, "\" WHERE \"name\" = ANY(")), "{sql}");
                    assert_eq!(values.0.0.len(), 1, "the whole batch is one bound array: {sql}");
                }

                /// The minting read carries the one fact that decides whether its
                /// hits may be published: whether the caller's transaction has
                /// written anything yet. Nothing else about the read changes.
                #[test]
                fn the_minting_read_reports_whether_the_transaction_still_reads_a_committed_snapshot() {
                    let (sql, values) = held_select_query::<$name>(&[$first]).build_sqlx(PostgresQueryBuilder);
                    assert!(
                        sql.contains(concat!("pg_current_xact_id_if_assigned() IS NULL AS \"", "snapshot_is_committed", "\"")),
                        "{sql}"
                    );
                    assert!(sql.contains(concat!("FROM \"", $table, "\" WHERE \"name\" = ANY(")), "{sql}");
                    assert_eq!(values.0.0.len(), 1, "the flag is computed server-side, not bound: {sql}");
                    assert_eq!(SNAPSHOT_IS_COMMITTED_COLUMN, "snapshot_is_committed");
                }

                /// Only the names the dictionary does not hold are minted, not every
                /// name this process happened not to have cached: a cache miss says
                /// nothing about whether the dictionary holds the name.
                #[test]
                fn only_the_names_the_dictionary_does_not_hold_are_minted() {
                    let asked = [second(), first()];
                    let wanted: Vec<&str> = asked.iter().map(InternedName::as_str).collect();
                    let holds_first = [(first(), <$key>::from(7))];
                    let holds_both = [(first(), <$key>::from(7)), (second(), <$key>::from(9))];

                    assert_eq!(names_not_yet_held(&wanted, &holds_first), vec![InternedName::as_str(&asked[0])]);
                    assert!(
                        names_not_yet_held(&wanted, &holds_both).is_empty(),
                        "a batch the dictionary holds in full creates nothing"
                    );
                }

                /// A freshly warmed cache is inside its check interval, so a
                /// resolution that finds every name cached issues no catalogue
                /// probe: the unroutable pool turns any probe into an error, so
                /// an Ok proves none was issued. This is the invariant the
                /// staleness check must not break: a cached hit stays free.
                #[tokio::test]
                async fn a_warm_cache_does_not_probe_the_database_within_the_check_interval() {
                    let cache = cache::<$name>();
                    assert!(
                        cache.revalidate_epoch(&cache.pool).await.is_ok(),
                        "a cache inside its check interval must not probe the database"
                    );
                }
            }
        };
    }

    name_cache_tests!(
        attribute_names,
        AttributeName,
        AttributeNameKey,
        "https://example.com/speed",
        "https://example.com/brand",
        "attribute_names"
    );
    name_cache_tests!(
        entity_type_names,
        EntityTypeName,
        EntityTypeNameKey,
        "https://example.com/Vehicle",
        "https://example.com/Building",
        "entity_type_names"
    );
}
