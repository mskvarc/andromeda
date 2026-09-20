//! In-process cache over one name dictionary.
//!
//! `attribute_names` maps expanded attribute IRIs to the `INTEGER` key used by
//! the temporal catalogue, the history hypertable, and the
//! `current_attribute_values` projection; `entity_type_names` maps expanded
//! type IRIs to the key the `type_ids` arrays carry. Both the write path
//! (which must create missing entries) and the read path (which must not)
//! resolve through this one cache, so a tenant holds a single copy of each
//! dictionary rather than one per repository. The cache is generic over the
//! [`DictionaryRelation`] so the two dictionaries share one implementation
//! and one set of statements.

use crate::{
    dictionary_growth::DictionaryNameLimit,
    dictionary_relation::{DictionaryKind, DictionaryRelation},
    name_lookup::{NameLookup, NameLookupError},
};
use andromeda_ngsi_ld_lexicon::reference::NgsiLdReferenceError;
use andromeda_postgresql_query::dictionary::{interned_name::InternedName, resolved_names::ResolvedNames};
use andromeda_postgresql_schema::tenancy::SchemaMetadata;
use andromeda_postgresql_seaquery::{
    introspection::transaction_has_not_written,
    pg_array::{any, text_array},
    pg_type::PostgresType,
    unnest::{UnnestColumn, unnest_call},
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use sea_query::{
    Alias,
    Asterisk,
    Expr,
    ExprTrait,
    Func,
    InsertStatement,
    OnConflict,
    PostgresQueryBuilder,
    Query,
    SelectStatement,
    error::Error as SeaQueryError,
};
use sea_query_sqlx::SqlxBinder;
use sqlx::{
    AssertSqlSafe,
    Executor,
    PgConnection,
    Postgres,
    Row,
    postgres::{PgPool, PgRow},
};
use std::{
    collections::{HashMap, HashSet},
    fmt::Debug,
    sync::{
        LazyLock,
        atomic::{AtomicI64, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Whether what a read resolved may be published to the process-wide cache.
///
/// This is the difference between the two reads the dictionary performs, and it
/// is a correctness property rather than a tuning knob: a mapping published from
/// inside an uncommitted transaction would answer every later resolution in the
/// process with a key that a rollback then took away, and the rows written
/// against that key would reference a dictionary entry that does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CachePublication {
    /// The read saw a committed snapshot, so what it resolved is durable and
    /// may answer later resolutions without a round trip.
    Publish,
    /// The read ran inside a caller's transaction and may have seen rows that
    /// transaction has not committed, so what it resolved answers this caller
    /// only.
    Withhold,
}

/// Cache of expanded IRI to dictionary key for one dictionary.
///
/// Only *committed hits* are ever stored. A miss is deliberately not memoised:
/// the read path resolves per query execution, and caching "this name does not
/// exist" would make a name created after the first lookup permanently
/// invisible to predicates, which compile an unknown IRI to a constant
/// `FALSE`. A mapping minted inside a caller's transaction is likewise not
/// stored — see [`CachePublication`]; it is published by the first read that
/// provably sees a committed snapshot, which is either a pool-side lookup or a
/// minting resolution made by a transaction that has not written yet.
#[derive(Debug)]
pub struct NameCache<N: DictionaryRelation> {
    /// The tenant database pool, used only by reads that need a committed
    /// snapshot. Minting runs on the caller's own connection.
    pool: PgPool,
    /// Cached mappings from expanded IRI text to dictionary key.
    keys: DashMap<N, N::Key>,
    /// Bound on how far this tenant's dictionary may grow.
    growth_limit: DictionaryNameLimit,
    /// The tenant schema stamp (`schema_metadata.applied_at`, microseconds) this
    /// cache was warmed against, or [`UNKNOWN_EPOCH`] before it has been
    /// established. A fresh migration re-stamps it, so it advances whenever the
    /// schema was dropped and recreated -- whether by dropping the database or
    /// the schema -- which is exactly when every cached key goes stale. See
    /// [`NameCache::revalidate_epoch`].
    schema_epoch: AtomicI64,
    /// Milliseconds since [`EPOCH`] at the last schema-stamp probe, throttling
    /// the staleness check to at most once per [`EPOCH_CHECK_INTERVAL`].
    epoch_checked_at: AtomicU64,
}

/// The instant the epoch throttle measures elapsed time from. A process-wide
/// monotonic origin lets the last-checked stamp be a relaxed atomic store
/// rather than a lock.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// Milliseconds elapsed since [`EPOCH`].
///
/// `Duration::as_millis` reports a `u128`; the origin is a process-monotonic
/// instant, so the elapsed value cannot approach the `u64` bound and saturating
/// there is unreachable rather than a silent narrowing.
fn now_millis() -> u64 {
    u64::try_from(EPOCH.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// The sentinel for a schema epoch not yet read. `schema_metadata.applied_at`
/// is a wall-clock instant that only ever advances, so this floor cannot
/// collide with an observed stamp.
const UNKNOWN_EPOCH: i64 = i64::MIN;

/// How long a resolution trusts the established schema epoch before it re-reads
/// the tenant schema stamp to detect an out-of-band drop and recreate. The
/// window bounds how long a stale cache can serve keys of a previous schema
/// generation; the broker itself performs no drop, so only operator surgery can
/// open it, and five seconds keeps the steady-state catalogue cost negligible.
const EPOCH_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// The tenant schema stamp: `SELECT applied_at FROM schema_metadata WHERE
/// singleton`, the instant the broker last stamped this schema's contract.
fn schema_epoch_query() -> SelectStatement {
    Query::select()
        .column(SchemaMetadata::AppliedAt)
        .from(SchemaMetadata::Table)
        .and_where(Expr::col(SchemaMetadata::Singleton).eq(true))
        .to_owned()
}

/// Read the tenant schema stamp on the given executor, as microseconds since
/// the Unix epoch, or `None` while the schema is migrated but not yet stamped.
///
/// Taken as an executor rather than always the pool so a minting resolution can
/// probe on the connection its transaction already holds, never reaching into
/// the pool for a second connection mid-commit.
async fn read_schema_epoch<'e, E: Executor<'e, Database = Postgres>>(executor: E) -> Result<Option<i64>, NameLookupError> {
    let (sql, values) = schema_epoch_query().build_sqlx(PostgresQueryBuilder);
    let stamped: Option<DateTime<Utc>> = sqlx::query_scalar_with(AssertSqlSafe(sql), values).fetch_optional(executor).await?;
    Ok(stamped.map(|applied_at| applied_at.timestamp_micros()))
}

/// Read the tenant schema stamp once over the tenant pool, for a caller warming
/// several dictionaries against the same database so they share one read rather
/// than each issuing its own. Feed the result to [`NameCache::adopt_epoch`].
///
/// # Errors
///
/// Returns [`NameLookupError::Statement`] when the stamp read fails.
pub async fn read_established_epoch(pool: &PgPool) -> Result<Option<i64>, NameLookupError> {
    read_schema_epoch(pool).await
}

impl<N: DictionaryRelation> NameCache<N> {
    /// Create a cache backed by the given tenant pool, bounded by
    /// [`DictionaryNameLimit::DEFAULT`].
    #[must_use]
    pub fn new(pool: &PgPool) -> NameCache<N> {
        NameCache {
            pool: pool.clone(), // clone: Arc ref-count bump
            keys: DashMap::new(),
            growth_limit: DictionaryNameLimit::DEFAULT,
            schema_epoch: AtomicI64::new(UNKNOWN_EPOCH),
            // Seeded to now, not zero: a cache is inside its check interval the
            // moment it is built, so its first resolutions serve cached hits
            // without a catalogue probe until the interval first elapses.
            epoch_checked_at: AtomicU64::new(now_millis()),
        }
    }

    /// Bound this tenant's dictionary at `limit` instead of the default.
    ///
    /// Taken per cache rather than per process because the dictionary is
    /// per-tenant: one tenant modelling a large domain is not a reason to let
    /// every other tenant's dictionary grow with it.
    #[must_use = "the bound applies to the returned cache"]
    pub fn with_growth_limit(self, limit: DictionaryNameLimit) -> NameCache<N> {
        NameCache { growth_limit: limit, ..self }
    }

    /// The bound on how far this tenant's dictionary may grow.
    pub const fn growth_limit(&self) -> DictionaryNameLimit {
        self.growth_limit
    }

    /// Load the whole dictionary into the cache.
    ///
    /// Removes the cold-miss round trip otherwise paid on first use of each
    /// IRI, on both the write and the read path.
    ///
    /// # Errors
    ///
    /// Returns [`NameLookupError::Statement`] when the read fails and one of
    /// the `Stored*` variants when a stored row is unreadable.
    pub async fn preload(&self) -> Result<(), NameLookupError> {
        let (sql, values) = preload_query::<N>().build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        self.absorb(&decode::<N>(&rows)?, CachePublication::Publish);
        Ok(())
    }

    /// Answer from the cache what it can, and report what it cannot.
    fn probe<'a>(&self, names: &'a [N]) -> (Vec<(N, N::Key)>, Vec<&'a N>) {
        let mut resolved = Vec::with_capacity(names.len());
        let mut missing = Vec::new();
        for name in names {
            match self.keys.get::<N>(name) {
                // clone: the answer owns its keys; the name is a `CompactString`
                Some(id) => resolved.push((name.clone(), *id)),
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
    /// The stamp is read once by [`read_established_epoch`] and handed to every
    /// dictionary the tenant opens, so the caches share one catalogue read. A
    /// `None` -- a schema migrated but not yet stamped -- leaves the sentinel in
    /// place and only records that the baseline probe ran.
    pub fn adopt_epoch(&self, epoch: Option<i64>) {
        if let Some(epoch) = epoch {
            self.schema_epoch.store(epoch, Ordering::Relaxed);
        }
        self.epoch_checked_at.store(now_millis(), Ordering::Relaxed);
    }

    /// Re-read the tenant database oid when the throttle interval has elapsed,
    /// discarding every cached key if it changed.
    ///
    /// A tenant database dropped and recreated under the same name comes back
    /// with fresh `IDENTITY` sequences, so every name->key mapping this cache
    /// published against the previous generation now points at a dictionary row
    /// that does not exist -- a write that trusts one raises
    /// `temporal_attribute_streams_attribute_name_id_fkey`. The broker performs
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
            self.keys.clear();
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
    /// caller's transaction holds. Three consequences follow, and all three are
    /// the point of the method taking it: the dictionary rows commit and roll
    /// back with the write that needed them; the size the bound is decided
    /// against is the size that transaction sees, so a rollback hands the budget
    /// back; and no phase of a commit reaches into the pool for a second
    /// connection while already holding one.
    ///
    /// A cache miss is not evidence that a name is new. The cache only ever
    /// holds what a *committed* read published, so every name minted inside a
    /// transaction — and every name minted before this process started, until
    /// something reads it back — misses. The dictionary is therefore asked
    /// which of the missing names it already holds *before* the growth bound is
    /// consulted, and the bound is decided against the names that genuinely
    /// remain to be created. Deciding it against every miss would refuse a write
    /// that reuses nothing but names the dictionary already holds, which is
    /// precisely the write a tenant at its bound must keep serving.
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
        // mappings answer this caller and go no further -- see
        // [`CachePublication`]. The one case that is provably safe is a
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

        // The dictionary size is read from the table rather than from `keys`,
        // which only ever holds what this process has seen: a cache that was
        // never preloaded would otherwise report a size of nearly zero and let
        // the bound be crossed silently. The read is paid only when a name is
        // genuinely new, which is once per distinct IRI per process.
        let (size_sql, size_values) = size_query::<N>().build_sqlx(PostgresQueryBuilder);
        let existing: i64 = sqlx::query_scalar_with(AssertSqlSafe(size_sql), size_values)
            .fetch_one(&mut *connection)
            .await?;
        // `COUNT(*)` is never negative; saturating to `u64::MAX` on a value
        // that says otherwise refuses the write rather than admitting it.
        self.growth_limit.admit(N::KIND, u64::try_from(existing).unwrap_or(u64::MAX), unheld.len())?;

        let (insert_sql, insert_values) = name_insert_query::<N>(&unheld)
            .map_err(|error| statement_error(&error))?
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
/// entity's members is counted once against the growth bound.
fn distinct_text<'a, N: InternedName>(missing: &[&'a N]) -> Vec<&'a str> {
    let mut text: Vec<&'a str> = missing.iter().map(|name| InternedName::as_str(*name)).collect();
    text.sort_unstable();
    text.dedup();
    text
}

/// The names of a batch the dictionary does not hold yet, in the order the
/// batch was sorted into.
///
/// This is the count the growth bound is decided against: a name the dictionary
/// already holds creates nothing, whether or not this process had it cached.
fn names_not_yet_held<'a, N: InternedName>(wanted: &[&'a str], held: &[(N, N::Key)]) -> Vec<&'a str> {
    let held: HashSet<&str> = held.iter().map(|(name, _)| InternedName::as_str(name)).collect();
    wanted.iter().filter(|name| !held.contains(*name)).copied().collect()
}

/// Read one dictionary result set back into typed pairs.
///
/// The row's `name` is the text that was bound, so each hit could be matched
/// back to the name that produced it; it is parsed instead because `preload`
/// reads rows nobody asked for, and one decoding serves both.
fn decode<N: DictionaryRelation>(rows: &[PgRow]) -> Result<Vec<(N, N::Key)>, NameLookupError> {
    let mut decoded = Vec::with_capacity(rows.len());
    for row in rows {
        let stored: String = row.try_get("name")?;
        let name = match stored.parse::<N>() {
            Ok(name) => name,
            Err(source) => return Err(stored_name_error::<N>(stored, source)),
        };
        decoded.push((name, N::Key::from(row.try_get::<i32, _>("id")?)));
    }
    Ok(decoded)
}

/// The failure a stored row that is not a valid name of type `N` becomes.
const fn stored_name_error<N: DictionaryRelation>(name: String, source: NgsiLdReferenceError) -> NameLookupError {
    match N::KIND {
        DictionaryKind::AttributeNames => NameLookupError::StoredAttributeName { name, source },
        DictionaryKind::EntityTypeNames => NameLookupError::StoredEntityTypeName { name, source },
    }
}

/// The failure a statement that could not be assembled becomes.
fn statement_error(error: &SeaQueryError) -> NameLookupError {
    NameLookupError::StatementBuild(error.to_string())
}

/// Every mapping the dictionary holds, for warming the cache.
fn preload_query<N: DictionaryRelation>() -> SelectStatement {
    Query::select().columns([N::id_column(), N::name_column()]).from(N::table()).to_owned()
}

/// The dictionary's size, as the growth bound is decided against it.
fn size_query<N: DictionaryRelation>() -> SelectStatement {
    Query::select().expr(Func::count(Expr::col(Asterisk))).from(N::table()).to_owned()
}

/// The column that reports whether the transaction reading the dictionary has
/// written anything yet.
const SNAPSHOT_IS_COMMITTED_COLUMN: &str = "snapshot_is_committed";

/// The read a minting resolution starts with: the keys of the names the
/// dictionary already holds, plus one flag telling whether the caller's
/// transaction holds a transaction id.
///
/// `pg_current_xact_id_if_assigned()` is null until the transaction's first
/// write, and a transaction that has written nothing cannot have inserted an
/// uncommitted dictionary row, so when the flag is true every row this read
/// returns is committed and may be published to the cache. Once the
/// transaction has written, the flag is false and the rows are withheld
/// exactly as before, because one of them may be this transaction's own.
fn held_select_query<N: DictionaryRelation>(names: &[&str]) -> SelectStatement {
    let mut query = name_select_query::<N>(names);
    query.expr_as(transaction_has_not_written(), Alias::new(SNAPSHOT_IS_COMMITTED_COLUMN));
    query
}

/// Whether the rows a [`held_select_query`] returned may be published.
///
/// The flag is constant across the rows of one read; an empty read has nothing
/// to publish, so it withholds without consulting anything.
fn held_publication(rows: &[PgRow]) -> Result<CachePublication, NameLookupError> {
    match rows.first() {
        Some(row) if row.try_get::<bool, _>(SNAPSHOT_IS_COMMITTED_COLUMN)? => Ok(CachePublication::Publish),
        Some(_) | None => Ok(CachePublication::Withhold),
    }
}

/// Read back the keys of a batch of names, bound as one array so that a large
/// batch cannot exhaust the protocol's parameter space.
fn name_select_query<N: DictionaryRelation>(names: &[&str]) -> SelectStatement {
    Query::select()
        .columns([N::id_column(), N::name_column()])
        .from(N::table())
        .and_where(Expr::col(N::name_column()).eq(any(text_array(names))))
        .to_owned()
}

/// Mint every name of a batch that the dictionary does not already hold.
///
/// One statement per batch rather than one per name, and `DO NOTHING` rather
/// than a prior existence check: another transaction may have minted the same
/// name between this one's read and its insert. The batch arrives as one bound
/// array through `UNNEST`, so a large batch cannot exhaust the parameter space.
fn name_insert_query<N: DictionaryRelation>(names: &[&str]) -> Result<InsertStatement, SeaQueryError> {
    let column = UnnestColumn::new(N::name_column(), PostgresType::TextArray);
    let source_alias = Alias::new("t");
    let mut source = Query::select();
    source
        .column(source_alias.clone())
        .from_function(unnest_call([(&column, text_array(names))]), source_alias);
    let mut insert = Query::insert();
    insert
        .into_table(N::table())
        .columns([N::name_column()])
        .select_from(source.clone())?
        .on_conflict(OnConflict::column(N::name_column()).do_nothing().to_owned());
    Ok(insert.clone())
}

#[cfg(test)]
mod tests {
    use crate::{
        dictionary_growth::DictionaryNameLimit,
        dictionary_relation::DictionaryRelation,
        name_cache::{
            CachePublication,
            NameCache,
            SNAPSHOT_IS_COMMITTED_COLUMN,
            distinct_text,
            held_select_query,
            name_insert_query,
            name_select_query,
            names_not_yet_held,
            size_query,
        },
        name_lookup::NameLookup,
    };
    use andromeda_ngsi_ld_lexicon::{attribute_name::AttributeName, entity_type_name::EntityTypeName};
    use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, dictionary::interned_name::InternedName, entity_type_id::EntityTypeId};
    use sea_query::PostgresQueryBuilder;
    use sea_query_sqlx::SqlxBinder;
    use sqlx::postgres::{PgConnectOptions, PgPool};
    use std::num::NonZeroU32;

    /// A cache over a pool that can never connect.
    ///
    /// Every test below asserts that some path issues no statement, and an
    /// unroutable host is what turns a violation into a failure instead of a
    /// silent round trip to whatever database the environment points at.
    /// `connect_lazy_with` does not dial on construction, so building it is
    /// infallible.
    fn cache<N: DictionaryRelation>() -> NameCache<N> {
        NameCache::new(&PgPool::connect_lazy_with(PgConnectOptions::new().host("invalid.invalid").port(1)))
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
                    assert!(cache.keys.is_empty(), "a failed lookup must not populate the cache");
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
                        cache.keys.is_empty(),
                        "an uncommitted mapping must not answer any later resolution in this process"
                    );
                }

                /// The counterpart: a read that saw a committed snapshot is exactly what
                /// may publish, and it is the only thing that ever fills the cache.
                #[tokio::test]
                async fn a_committed_read_publishes_what_it_resolved_to_the_cache() {
                    let cache = cache::<$name>();
                    cache.absorb(&[(first(), <$key>::from(7))], CachePublication::Publish);
                    assert_eq!(cache.keys.get(&first()).map(|id| *id), Some(<$key>::from(7)));
                }

                /// Ordering is what keeps two concurrent writers from deadlocking on the
                /// unique index, and deduplication is what keeps one name repeated across
                /// an entity's members from being counted twice against the bound.
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

                /// The bound is decided against the dictionary itself, read on the
                /// caller's connection, so a rolled-back write releases the budget it
                /// took.
                #[test]
                fn the_bound_is_decided_against_the_dictionarys_own_size() {
                    assert_eq!(
                        size_query::<$name>().to_string(PostgresQueryBuilder),
                        concat!("SELECT COUNT(*) FROM \"", $table, "\"")
                    );
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

                /// The growth bound is decided against the names that genuinely have to
                /// be created, not against every name this process happened not to have
                /// cached. A dictionary at its bound must keep serving a write that
                /// reuses only names it already holds, and a cache miss says nothing
                /// about whether it holds them.
                #[test]
                fn only_the_names_the_dictionary_does_not_hold_count_against_the_growth_bound() {
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

                /// A tenant given its own bound keeps it; the default is only the
                /// fallback for a tenant that was configured with none.
                #[tokio::test]
                async fn a_configured_growth_bound_replaces_the_default_one() {
                    let configured = DictionaryNameLimit::new(NonZeroU32::new(64).expect("a non-zero test bound"));
                    assert_eq!(cache::<$name>().growth_limit(), DictionaryNameLimit::DEFAULT);
                    assert_eq!(cache::<$name>().with_growth_limit(configured).growth_limit(), configured);
                }

                /// A freshly warmed cache is inside its check interval, so a
                /// resolution that finds every name cached issues no catalogue
                /// probe: the unroutable pool turns any probe into an error, so
                /// an Ok proves none was issued. This is the invariant the
                /// staleness check must not break -- a cached hit stays free.
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
        AttributeNameId,
        "https://example.com/speed",
        "https://example.com/brand",
        "attribute_names"
    );
    name_cache_tests!(
        entity_type_names,
        EntityTypeName,
        EntityTypeId,
        "https://example.com/Vehicle",
        "https://example.com/Building",
        "entity_type_names"
    );
}
