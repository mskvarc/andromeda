//! Executing a compiled entity query: the page a window selects, its total,
//! and the identifiers alone (clauses 5.7.2 and 5.5.9).

use crate::{
    current_attribute::read::RowSelection,
    entity::{mapper::stored_uri, repo::EntityRepository, unreadable_entity::stored_entry},
    error::ContextStoreError,
    store_invariant::StoreInvariant,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query_outcome::QueryOutcome,
};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{
    count_policy::CountPolicy,
    entity::compiled::CompiledEntityQuery,
    filter::compiled_filter::CompiledFilter,
    page_window::PageWindow,
    projection::returned_attributes::ReturnedAttributes,
};
use andromeda_postgresql_branch::schema_iden;
use andromeda_postgresql_context_query::{
    entity as entity_sql,
    entity_page::{GATED_STEP_COLUMN, GatedStep, TOTAL_COUNT_COLUMN},
    entity_total::EntityTotal,
    filter_reads::counted_attributes,
};
use andromeda_postgresql_pool::pool_share::PoolLane;
use andromeda_postgresql_query::{
    attribute_carriers::{AttributeCarriers, CountedInstances, carriers_query},
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
    query_names::QueryNames,
};
use andromeda_postgresql_schema::census::AttributeCensus;
use andromeda_postgresql_seaquery::statement_planning::StatementPlanning;
use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
use iri_rs::IriBuf;
use sea_query::{Iden, SelectStatement};
use sqlx::{Row, postgres::PgRow};

impl EntityRepository {
    /// Execute a compiled entity query and return the matching stored states
    /// `window` selects, each with its clause 4.8 system attributes, counting
    /// every match when `count` asks for it. A match whose stored rows no
    /// longer read back keeps its place in the page, named by its identifier.
    ///
    /// Two statements: the page's heads, then the `current_attributes` rows of
    /// the Entities it kept, by their keys and limited to the attributes the
    /// query's `attrs` or clause 4.21 projection can return. A projection that
    /// returns no attribute reads no row.
    ///
    /// The page is returned in the order it is presented: a backward or
    /// from-end cursor page, read descending so one index scan serves it, is
    /// reversed here.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a name the query reads cannot be
    /// resolved through the dictionaries, when the statement cannot be rendered
    /// from the compiled query, when a statement or the separate count fails,
    /// or when a head's key or identifier does not read back.
    pub async fn query_compiled(
        &self,
        target: &ContextTarget,
        compiled: &CompiledEntityQuery,
        window: &PageWindow,
        count: CountPolicy,
    ) -> Result<QueryOutcome, ContextStoreError> {
        self.ensure_collations(&compiled.ordering).await?;
        let names = self.resolve_names(compiled).await?;
        let names = self.with_census_counts(target, compiled, window, count, names).await?;
        let built = entity_sql::build_query(target, compiled, window, count, &names)?;

        let head_rows = self.reads.acquire().await?.fetch_planned(&built.select, built.planning).await?;
        let window_count = page_total(&head_rows, matches!(built.total, EntityTotal::OnPageRows { .. }))?;
        let step = match &built.total {
            EntityTotal::OnPageRows { uncounted: total } | EntityTotal::Counted(total) if total.walked.is_some() => page_step(&head_rows)?,
            EntityTotal::Omitted | EntityTotal::OnPageRows { .. } | EntityTotal::Counted(_) => None,
        };
        let returned = ReturnedAttributes::of(&compiled.projection, &compiled.selection.attrs);
        let selection = RowSelection::of(&returned, &names.attributes);
        let mut rows: Vec<StoredEntry<RetrievedEntity>> = self
            .entities_of_heads(PoolLane::Reads, target, &head_rows, &selection)
            .await?
            .into_iter()
            .map(stored_entry)
            .collect();
        built.read_order.present(&mut rows);
        let rows = rows;

        let total_count = match built.total {
            EntityTotal::Omitted => None,
            EntityTotal::OnPageRows { uncounted } => match window_count {
                Some(count) => Some(count),
                None => Some(self.count_matches(&uncounted.for_step(step), built.total_planning).await?),
            },
            EntityTotal::Counted(count) => Some(self.count_matches(&count.for_step(step), built.total_planning).await?),
        };

        Ok(QueryOutcome { rows, total_count })
    }

    /// Run a `COUNT` statement as `planning` asks, on a connection of the
    /// read share, and read back its one value.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when no connection is free within the
    /// acquire deadline, the statement fails or returns no row, or the
    /// transaction it runs in cannot begin or commit.
    async fn count_matches(&self, count: &SelectStatement, planning: StatementPlanning) -> Result<i64, ContextStoreError> {
        let rows = self.reads.acquire().await?.fetch_planned(count, planning).await?;
        let total = rows.first().ok_or(sqlx::Error::RowNotFound)?;
        Ok(total.try_get(0)?)
    }

    /// Execute a compiled query and return only the identifiers of the
    /// matching entities `window` selects, in the order they are presented.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when a name the query reads cannot be
    /// resolved through the dictionaries, when the statement cannot be rendered
    /// from the compiled query, when the select fails, or when a returned
    /// identifier does not read back as a URI.
    pub async fn query_ids(&self, target: &ContextTarget, compiled: &CompiledEntityQuery, window: &PageWindow) -> Result<Vec<Identifier>, ContextStoreError> {
        self.ensure_collations(&compiled.ordering).await?;
        let names = self.resolve_names(compiled).await?;
        let statement = entity_sql::build_ids_query(target, compiled, window, &names)?;

        let rows = self.fetch_on(PoolLane::Reads, &statement.select).await?;
        let mut ids: Vec<Identifier> = rows
            .iter()
            .map(|row| -> Result<Identifier, ContextStoreError> { stored_uri(row.try_get("id")?) })
            .collect::<Result<_, _>>()?;
        statement.read_order.present(&mut ids);
        Ok(ids)
    }

    /// Resolve the attribute and entity type IRIs a compiled query reads by
    /// dictionary key.
    ///
    /// A clause 5.5.9 `attrs` term is answered by the search row's interned
    /// attribute names, so every term resolves, and so is every name a clause
    /// 4.21 projection keeps an entity alive by or returns, which the rows of
    /// the page are read by. A clause 4.9 linked-entity
    /// traversal contributes more than one name per predicate: the Relationship
    /// each hop descends through is probed by dictionary key just as the
    /// terminal attribute is, and both come back from the same collection walk.
    /// A geo-query and a distance ordering each contribute their target
    /// `GeoProperty`, which is read from the same projection (clauses 4.10 and
    /// 4.23). The type selection of every selector
    /// (clause 4.17) and the type guard of every traversal hop are tested
    /// against the interned `entity_type_name_keys` column, so they resolve through the type
    /// dictionary.
    ///
    /// The lookups need no barrier in front of them. Dictionary rows and the
    /// rows keyed on them are written in the entity's own transaction, so a
    /// name that resolves to nothing is a name nothing has ever written, not
    /// one written moments ago.
    async fn resolve_names(&self, compiled: &CompiledEntityQuery) -> Result<QueryNames, ContextStoreError> {
        let returned = match ReturnedAttributes::of(&compiled.projection, &compiled.selection.attrs) {
            ReturnedAttributes::Every => Vec::new(),
            ReturnedAttributes::Named(iris) | ReturnedAttributes::AllBut(iris) => iris,
        };
        let members = compiled
            .selection
            .attrs
            .iter()
            .chain(returned)
            .map(IriBuf::as_str)
            .chain(compiled.selection.projected_members.names());
        let attribute_names = projected_attribute_names(
            members,
            compiled.selection.q.as_ref().map(CompiledFilter::reading),
            compiled.selection.geo.as_ref(),
            &compiled.ordering.terms,
        );
        let attributes = if attribute_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.attribute_names.lookup(&attribute_names).await?
        };
        let type_names = projected_type_names(&compiled.selection);
        let types = if type_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.entity_type_names.lookup(&type_names).await?
        };
        Ok(QueryNames {
            attributes,
            types,
            ..QueryNames::default()
        })
    }

    /// `names` with the census counts of the attributes the page of
    /// `compiled` over `window`, its total as `count` asks, reads them for
    /// ([`counted_attributes`]), read from the branch `target` names: the
    /// keyed conjuncts of `q` when two or more of them contend for the page's
    /// index, and the attributes `attrs` names beside a keyed term or on a
    /// geo page. Left without counts when the page reads none.
    ///
    /// # Errors
    ///
    /// Returns [`ContextStoreError`] when no connection is free within the
    /// acquire deadline or the census read fails.
    async fn with_census_counts(
        &self,
        target: &ContextTarget,
        compiled: &CompiledEntityQuery,
        window: &PageWindow,
        count: CountPolicy,
        mut names: QueryNames,
    ) -> Result<QueryNames, ContextStoreError> {
        let keys = counted_attributes(compiled, &names, window, count);
        if keys.is_empty() {
            return Ok(names);
        }
        let rows = self.fetch_on(PoolLane::Reads, &carriers_query(schema_iden(target).as_ref(), &keys)).await?;
        let counted = rows.iter().map(census_count).collect::<Result<Vec<_>, sqlx::Error>>()?;
        // A ledger summing below zero counts nothing, so its attribute is left
        // uncounted rather than read as having no instance.
        let covered: Vec<AttributeNameKey> = keys
            .iter()
            .copied()
            .filter(|key| !counted.iter().any(|(counted_key, instances)| counted_key == key && instances.is_none()))
            .collect();
        let counts = counted.into_iter().filter_map(|(key, instances)| instances.map(|count| (key, count)));
        names.carriers = AttributeCarriers::new(&covered, counts);
        Ok(names)
    }
}

/// One row of the census read: an attribute and the instances counted of it,
/// `None` for a ledger summing below zero, which counts nothing.
///
/// # Errors
///
/// Returns [`sqlx::Error`] when the row lacks either column.
fn census_count(row: &PgRow) -> Result<(AttributeNameKey, Option<u64>), sqlx::Error> {
    let attribute: i32 = row.try_get(Iden::to_string(&AttributeCensus::AttributeNameKey).as_str())?;
    let instances: i64 = row.try_get(Iden::to_string(&CountedInstances).as_str())?;
    Ok((AttributeNameKey::from(attribute), u64::try_from(instances).ok()))
}

/// The piggybacked total of a page, read off its first row when the page
/// carries one (`COUNT(*) OVER ()` is constant across the set). A gated page
/// whose index branch did not answer carries a null total.
///
/// # Errors
///
/// Returns [`sqlx::Error`] when the page's rows have no total column.
fn page_total(rows: &[PgRow], read_total_count: bool) -> Result<Option<i64>, sqlx::Error> {
    match (read_total_count, rows.first()) {
        (true, Some(first)) => first.try_get::<Option<i64>, _>(TOTAL_COUNT_COLUMN),
        (true, None) | (false, _) => Ok(None),
    }
}

/// The gated step that answered a page whose rows name it, read off its
/// first row; `None` for an empty page.
///
/// # Errors
///
/// Returns [`ContextStoreError`] when the rows have no step column or name a
/// step no gated page has.
fn page_step(rows: &[PgRow]) -> Result<Option<GatedStep>, ContextStoreError> {
    rows.first()
        .map(|first| -> Result<GatedStep, ContextStoreError> {
            let step: i16 = first.try_get(GATED_STEP_COLUMN)?;
            GatedStep::try_from(step).map_err(|_| ContextStoreError::from(StoreInvariant::UnknownGatedStep { step }))
        })
        .transpose()
}
