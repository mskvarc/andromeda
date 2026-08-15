//! In-process cache over the `attribute_names` dictionary.
//!
//! `attribute_names` maps expanded attribute IRIs to an `INTEGER` key used by
//! the temporal catalogue, the history hypertable, and the
//! `current_attribute_values` projection. Both the write path (which must create
//! missing entries) and the `q`-filter read path (which must not) resolve
//! through this one cache, so a tenant holds a single copy of the dictionary
//! rather than one per repository.

use crate::{
    attribute_name_lookup::{AttributeNameLookup, AttributeNameLookupError},
    dictionary_growth::DictionaryNameLimit,
};
use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use andromeda_postgresql_query::{attribute_name_id::AttributeNameId, q_predicate::attribute_names::ResolvedAttributeNames};
use andromeda_postgresql_schema::attribute_dictionary::AttributeNames;
use andromeda_postgresql_seaquery::{
    pg_array::{any, text_array},
    pg_type::PostgresType,
    unnest::{UnnestColumn, unnest_call},
};
use dashmap::DashMap;
use sea_query::{Alias, Asterisk, Expr, ExprTrait, Func, InsertStatement, OnConflict, PostgresQueryBuilder, Query, SelectStatement};
use sea_query_sqlx::SqlxBinder;
use sqlx::{
    AssertSqlSafe,
    PgConnection,
    Row,
    postgres::{PgPool, PgRow},
};
use std::collections::{HashMap, HashSet};

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

/// Cache of expanded attribute IRI to dictionary id.
///
/// Only *committed hits* are ever stored. A miss is deliberately not memoised:
/// the `q`-filter path resolves per query execution, and caching "this attribute
/// does not exist" would make an attribute created after the first lookup
/// permanently invisible to value predicates, which compile an unknown IRI to a
/// constant `FALSE`. A mapping minted inside a caller's transaction is likewise
/// not stored — see [`CachePublication`] — so the first committed read of that
/// name is what publishes it.
#[derive(Debug)]
pub struct AttributeNameCache {
    /// The tenant database pool, used only by reads that need a committed
    /// snapshot. Minting runs on the caller's own connection.
    pool: PgPool,
    /// Cached mappings from expanded IRI text to dictionary id.
    ids: DashMap<AttributeName, AttributeNameId>,
    /// Bound on how far this tenant's dictionary may grow.
    growth_limit: DictionaryNameLimit,
}

impl AttributeNameCache {
    /// Create a cache backed by the given tenant pool, bounded by
    /// [`DictionaryNameLimit::DEFAULT`].
    pub fn new(pool: &PgPool) -> AttributeNameCache {
        AttributeNameCache {
            pool: pool.clone(), // clone: Arc ref-count bump
            ids: DashMap::new(),
            growth_limit: DictionaryNameLimit::DEFAULT,
        }
    }

    /// Bound this tenant's dictionary at `limit` instead of the default.
    ///
    /// Taken per cache rather than per process because the dictionary is
    /// per-tenant: one tenant modelling a large domain is not a reason to let
    /// every other tenant's dictionary grow with it.
    #[must_use = "the bound applies to the returned cache"]
    pub fn with_growth_limit(self, limit: DictionaryNameLimit) -> AttributeNameCache {
        AttributeNameCache { growth_limit: limit, ..self }
    }

    /// The bound on how far this tenant's dictionary may grow.
    pub const fn growth_limit(&self) -> DictionaryNameLimit {
        self.growth_limit
    }

    /// Load the whole dictionary into the cache.
    ///
    /// Removes the cold-miss round trip otherwise paid on first use of each
    /// attribute IRI, on both the write and the read path.
    ///
    /// # Errors
    ///
    /// Returns [`AttributeNameLookupError::Statement`] when the read fails and
    /// [`AttributeNameLookupError::StoredName`] when a stored row is unreadable.
    pub async fn preload(&self) -> Result<(), AttributeNameLookupError> {
        let (sql, values) = preload_query().build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        self.absorb(&decode(&rows)?, CachePublication::Publish);
        Ok(())
    }

    /// Answer from the cache what it can, and report what it cannot.
    fn probe<'a>(&self, names: &'a [AttributeName]) -> (Vec<(AttributeName, AttributeNameId)>, Vec<&'a AttributeName>) {
        let mut resolved = Vec::with_capacity(names.len());
        let mut missing = Vec::new();
        for name in names {
            match self.ids.get(name) {
                // clone: the answer owns its keys; the name is a `CompactString`
                Some(id) => resolved.push((name.clone(), *id)),
                None => missing.push(name),
            }
        }
        (resolved, missing)
    }

    /// Record what one read resolved, publishing it only when the read saw a
    /// committed snapshot.
    fn absorb(&self, resolved: &[(AttributeName, AttributeNameId)], publication: CachePublication) {
        match publication {
            CachePublication::Publish => {
                for (name, id) in resolved {
                    self.ids.insert(name.clone(), *id); // clone: the cache keeps its own key
                }
            }
            CachePublication::Withhold => {}
        }
    }
}

#[async_trait::async_trait]
impl AttributeNameLookup for AttributeNameCache {
    /// Resolve names that already exist, without creating any.
    ///
    /// Misses are re-queried on every call rather than remembered, so an
    /// attribute becomes visible to `q` filters as soon as it is first written.
    async fn lookup(&self, names: &[AttributeName]) -> Result<ResolvedAttributeNames, AttributeNameLookupError> {
        let (mut resolved, missing) = self.probe(names);
        if missing.is_empty() {
            return Ok(ResolvedAttributeNames::from_pairs(resolved));
        }
        let wanted = distinct_text(&missing);
        // The pool rather than a caller's connection: publication to the cache
        // is only sound from a committed snapshot, which is what this read sees.
        let (sql, values) = name_select_query(&wanted).build_sqlx(PostgresQueryBuilder);
        let rows = sqlx::query_with(AssertSqlSafe(sql), values).fetch_all(&self.pool).await?;
        let found = decode(&rows)?;
        self.absorb(&found, CachePublication::Publish);
        resolved.extend(found);
        Ok(ResolvedAttributeNames::from_pairs(resolved))
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
    async fn resolve_or_create(
        &self,
        connection: &mut PgConnection,
        names: &[AttributeName],
    ) -> Result<HashMap<AttributeName, AttributeNameId>, AttributeNameLookupError> {
        let (mut resolved, missing) = self.probe(names);
        if missing.is_empty() {
            return Ok(resolved.into_iter().collect());
        }
        let wanted = distinct_text(&missing);

        // The caller's connection rather than the pool: a name an earlier phase
        // of this same transaction minted is uncommitted, and a read that could
        // not see it would count it as new a second time.
        let (held_sql, held_values) = name_select_query(&wanted).build_sqlx(PostgresQueryBuilder);
        let held = decode(&sqlx::query_with(AssertSqlSafe(held_sql), held_values).fetch_all(&mut *connection).await?)?;
        let unheld = names_not_yet_held(&wanted, &held);
        // What the dictionary already held may still be rolled back by the
        // caller's transaction, so the mappings answer this caller and go no
        // further -- see [`CachePublication`].
        self.absorb(&held, CachePublication::Withhold);
        resolved.extend(held);
        if unheld.is_empty() {
            return Ok(resolved.into_iter().collect());
        }

        // The dictionary size is read from the table rather than from `ids`,
        // which only ever holds what this process has seen: a cache that was
        // never preloaded would otherwise report a size of nearly zero and let
        // the bound be crossed silently. The read is paid only when a name is
        // genuinely new, which is once per distinct attribute IRI per process.
        let (size_sql, size_values) = size_query().build_sqlx(PostgresQueryBuilder);
        let existing: i64 = sqlx::query_scalar_with(AssertSqlSafe(size_sql), size_values)
            .fetch_one(&mut *connection)
            .await?;
        // `COUNT(*)` is never negative; saturating to `u64::MAX` on a value
        // that says otherwise refuses the write rather than admitting it.
        self.growth_limit.admit(u64::try_from(existing).unwrap_or(u64::MAX), unheld.len())?;

        let (insert_sql, insert_values) = name_insert_query(&unheld).map_err(statement_error)?.build_sqlx(PostgresQueryBuilder);
        sqlx::query_with(AssertSqlSafe(insert_sql), insert_values).execute(&mut *connection).await?;
        let (minted_sql, minted_values) = name_select_query(&unheld).build_sqlx(PostgresQueryBuilder);
        let minted = decode(&sqlx::query_with(AssertSqlSafe(minted_sql), minted_values).fetch_all(&mut *connection).await?)?;
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
/// entity's attributes is counted once against the growth bound.
fn distinct_text<'a>(missing: &[&'a AttributeName]) -> Vec<&'a str> {
    let mut text: Vec<&'a str> = missing.iter().map(|name| name.as_str()).collect();
    text.sort_unstable();
    text.dedup();
    text
}

/// The names of a batch the dictionary does not hold yet, in the order the
/// batch was sorted into.
///
/// This is the count the growth bound is decided against: a name the dictionary
/// already holds creates nothing, whether or not this process had it cached.
fn names_not_yet_held<'a>(wanted: &[&'a str], held: &[(AttributeName, AttributeNameId)]) -> Vec<&'a str> {
    let held: HashSet<&str> = held.iter().map(|(name, _)| name.as_str()).collect();
    wanted.iter().filter(|name| !held.contains(*name)).copied().collect()
}

/// Read one dictionary result set back into typed pairs.
///
/// The row's `name` is the text that was bound, so each hit could be matched
/// back to the name that produced it; it is parsed instead because `preload`
/// reads rows nobody asked for, and one decoding serves both.
fn decode(rows: &[PgRow]) -> Result<Vec<(AttributeName, AttributeNameId)>, AttributeNameLookupError> {
    let mut decoded = Vec::with_capacity(rows.len());
    for row in rows {
        let stored: String = row.try_get("name")?;
        let name = match stored.parse::<AttributeName>() {
            Ok(name) => name,
            Err(source) => return Err(AttributeNameLookupError::StoredName { name: stored, source }),
        };
        decoded.push((name, AttributeNameId::from(row.try_get::<i32, _>("id")?)));
    }
    Ok(decoded)
}

/// The failure a statement that could not be assembled becomes.
fn statement_error(error: sea_query::error::Error) -> AttributeNameLookupError {
    AttributeNameLookupError::StatementBuild(error.to_string())
}

/// Every mapping the dictionary holds, for warming the cache.
fn preload_query() -> SelectStatement {
    Query::select()
        .columns([AttributeNames::Id, AttributeNames::Name])
        .from(AttributeNames::Table)
        .to_owned()
}

/// The dictionary's size, as the growth bound is decided against it.
fn size_query() -> SelectStatement {
    Query::select().expr(Func::count(Expr::col(Asterisk))).from(AttributeNames::Table).to_owned()
}

/// Read back the keys of a batch of names, bound as one array so that a large
/// batch cannot exhaust the protocol's parameter space.
fn name_select_query(names: &[&str]) -> SelectStatement {
    Query::select()
        .columns([AttributeNames::Id, AttributeNames::Name])
        .from(AttributeNames::Table)
        .and_where(Expr::col(AttributeNames::Name).eq(any(text_array(names))))
        .to_owned()
}

/// Mint every name of a batch that the dictionary does not already hold.
///
/// One statement per batch rather than one per name, and `DO NOTHING` rather
/// than a prior existence check: another transaction may have minted the same
/// name between this one's read and its insert. The batch arrives as one bound
/// array through `UNNEST`, so a large batch cannot exhaust the parameter space.
fn name_insert_query(names: &[&str]) -> Result<InsertStatement, sea_query::error::Error> {
    let column = UnnestColumn::new(AttributeNames::Name, PostgresType::TextArray);
    let source_alias = Alias::new("t");
    let mut source = Query::select();
    source
        .column(source_alias.clone())
        .from_function(unnest_call([(&column, text_array(names))]), source_alias);
    let mut insert = Query::insert();
    insert
        .into_table(AttributeNames::Table)
        .columns([AttributeNames::Name])
        .select_from(source.to_owned())?
        .on_conflict(OnConflict::column(AttributeNames::Name).do_nothing().to_owned());
    Ok(insert.to_owned())
}

#[cfg(test)]
mod tests {
    use crate::{
        attribute_name_cache::{AttributeNameCache, CachePublication, distinct_text, name_insert_query, name_select_query, names_not_yet_held, size_query},
        attribute_name_lookup::AttributeNameLookup,
        dictionary_growth::DictionaryNameLimit,
    };
    use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
    use andromeda_postgresql_query::attribute_name_id::AttributeNameId;
    use sea_query::PostgresQueryBuilder;
    use sqlx::postgres::{PgConnectOptions, PgPool};
    use std::num::NonZeroU32;

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

    /// A second expanded attribute name, ordered before the first.
    fn brand() -> AttributeName {
        "https://example.com/brand".parse().expect("valid attribute name")
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
    async fn lookup_serves_cached_hits_without_a_query() {
        let cache = cache();
        cache.ids.insert(speed(), AttributeNameId::from(7));
        // The pool points nowhere, so this only succeeds if no query is issued.
        let resolved = cache.lookup(&[speed()]).await.ok().and_then(|names| names.get(&speed()));
        assert_eq!(resolved, Some(AttributeNameId::from(7)));
    }

    /// A resolution with nothing left to find issues no statement at all, which
    /// is what lets a fully cached batch mint names without a round trip. The
    /// empty request is the degenerate case of the same rule.
    #[tokio::test]
    async fn a_request_whose_names_are_all_cached_leaves_nothing_to_mint() {
        let cache = cache();
        cache.ids.insert(speed(), AttributeNameId::from(7));
        let asked = [speed()];
        let (resolved, missing) = cache.probe(&asked);
        assert_eq!(resolved, vec![(speed(), AttributeNameId::from(7))]);
        assert!(missing.is_empty(), "a cached name is never asked of the dictionary again");
        let (empty, nothing) = cache.probe(&[]);
        assert!(empty.is_empty() && nothing.is_empty(), "an empty request resolves to nothing");
    }

    #[tokio::test]
    async fn a_miss_is_never_written_back_to_the_cache() {
        // Guards the invariant that makes execution-time resolution safe: if a
        // miss were memoised, an attribute created after the first lookup would
        // stay invisible to q filters for the lifetime of the process.
        let cache = cache();
        let asked = ["https://example.com/never-written".parse::<AttributeName>().expect("valid attribute name")];
        let (resolved, missing) = cache.probe(&asked);
        assert!(resolved.is_empty());
        assert_eq!(missing.len(), 1);
        assert!(cache.ids.is_empty(), "a failed lookup must not populate the cache");
    }

    /// The rule the transaction-aware write path depends on: a name minted
    /// inside the caller's transaction is handed back to that caller and to
    /// nobody else, because the transaction may still roll it away.
    #[tokio::test]
    async fn a_resolution_made_inside_a_transaction_is_not_published_to_the_cache() {
        let cache = cache();
        let minted = [(speed(), AttributeNameId::from(7))];
        cache.absorb(&minted, CachePublication::Withhold);
        assert!(
            cache.ids.is_empty(),
            "an uncommitted mapping must not answer any later resolution in this process"
        );
    }

    /// The counterpart: a read that saw a committed snapshot is exactly what may
    /// publish, and it is the only thing that ever fills the cache.
    #[tokio::test]
    async fn a_committed_read_publishes_what_it_resolved_to_the_cache() {
        let cache = cache();
        cache.absorb(&[(speed(), AttributeNameId::from(7))], CachePublication::Publish);
        assert_eq!(cache.ids.get(&speed()).map(|id| *id), Some(AttributeNameId::from(7)));
    }

    /// Ordering is what keeps two concurrent writers from deadlocking on the
    /// unique index, and deduplication is what keeps one name repeated across an
    /// entity's attributes from being counted twice against the bound.
    #[test]
    fn names_are_minted_as_one_sorted_distinct_batch() {
        let (speed, brand) = (speed(), brand());
        let batch = distinct_text(&[&speed, &brand, &speed]);
        assert_eq!(batch, vec![brand.as_str(), speed.as_str()]);
    }

    /// Another transaction may mint the same name between this one's read and
    /// its insert, so the insert states what happens then rather than failing.
    #[test]
    fn minting_leaves_a_name_another_writer_already_minted_alone() {
        use sea_query_sqlx::SqlxBinder;
        let (sql, values) = name_insert_query(&["https://example.com/speed"])
            .expect("arity")
            .build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains("INSERT INTO \"attribute_names\" (\"name\")"), "{sql}");
        assert!(
            sql.contains("SELECT \"t\" FROM UNNEST($1::text[]) AS \"t\""),
            "the scalar table-function result is selected through its table alias: {sql}"
        );
        assert!(sql.contains("ON CONFLICT (\"name\") DO NOTHING"), "{sql}");
        assert_eq!(values.0.0.len(), 1, "{sql}");
    }

    /// The bound is decided against the dictionary itself, read on the caller's
    /// connection, so a rolled-back write releases the budget it took.
    #[test]
    fn the_bound_is_decided_against_the_dictionarys_own_size() {
        assert_eq!(size_query().to_string(PostgresQueryBuilder), "SELECT COUNT(*) FROM \"attribute_names\"");
    }

    /// One array bind rather than a placeholder per name, which is what keeps a
    /// large batch inside the protocol's parameter space.
    #[test]
    fn a_batch_of_names_is_read_back_through_a_single_array_bind() {
        use sea_query_sqlx::SqlxBinder;
        let (sql, values) = name_select_query(&["https://example.com/speed", "https://example.com/brand"]).build_sqlx(PostgresQueryBuilder);
        assert!(sql.contains("\"name\" = ANY("), "{sql}");
        assert_eq!(values.0.0.len(), 1, "the whole batch is one bound array: {sql}");
    }

    /// The growth bound is decided against the names that genuinely have to be
    /// created, not against every name this process happened not to have cached.
    /// A dictionary at its bound must keep serving a write that reuses only
    /// names it already holds, and a cache miss says nothing about whether it
    /// holds them.
    #[test]
    fn only_the_names_the_dictionary_does_not_hold_count_against_the_growth_bound() {
        let asked = [brand(), speed()];
        let wanted: Vec<&str> = asked.iter().map(AttributeName::as_str).collect();
        let holds_speed = [(speed(), AttributeNameId::from(7))];
        let holds_both = [(speed(), AttributeNameId::from(7)), (brand(), AttributeNameId::from(9))];

        assert_eq!(names_not_yet_held(&wanted, &holds_speed), vec![asked[0].as_str()]);
        assert!(
            names_not_yet_held(&wanted, &holds_both).is_empty(),
            "a batch the dictionary holds in full creates nothing"
        );
    }

    /// A tenant given its own bound keeps it; the default is only the fallback
    /// for a tenant that was configured with none.
    #[tokio::test]
    async fn a_configured_growth_bound_replaces_the_default_one() {
        let configured = DictionaryNameLimit::new(NonZeroU32::new(64).expect("a non-zero test bound"));
        assert_eq!(cache().growth_limit(), DictionaryNameLimit::DEFAULT);
        assert_eq!(cache().with_growth_limit(configured).growth_limit(), configured);
    }
}
