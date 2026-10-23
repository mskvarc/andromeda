//! Resolving what a temporal statement renders against before it is built:
//! the dictionary keys of the names it reads and, for a selection by
//! attribute alone, the types holding each attribute.

use crate::{error::TemporalStoreError, repo::TemporalRepository};
use andromeda_ngsi_ld_query::temporal::{
    compiled::CompiledTemporalQuery,
    grouping::{AggregationGrouping, group_key::GroupKey},
};
use andromeda_postgresql_query::{
    attribute_name_key::AttributeNameKey,
    dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    entity_type_name_key::EntityTypeNameKey,
    history_source::{
        HistoryBranch,
        attribute_holders::{AttributeHolders, HeldStreams, HolderType, holder_types_query},
    },
    projected_attribute_names::projected_attribute_names,
    projected_type_names::projected_type_names,
};
use andromeda_postgresql_schema::temporal::StreamTypeCensus;
use andromeda_postgresql_seaquery::statement::Statement;
use andromeda_postgresql_temporal_query::stage1::evolution_walk::{concerned_attribute_keys, walks_attribute_holders};
use andromeda_temporal::error::TemporalError;
use iri_rs::IriBuf;
use sea_query::Iden;
use sqlx::{Row, postgres::PgRow};

/// One row of the stream census read: the attribute, a type holding it and
/// how many streams it holds.
fn census_holder(row: &PgRow) -> Result<(AttributeNameKey, HolderType), TemporalStoreError> {
    let attribute: i32 = row.try_get(Iden::to_string(&StreamTypeCensus::AttributeNameKey).as_str())?;
    let type_key: i32 = row.try_get(Iden::to_string(&StreamTypeCensus::EntityTypeNameKey).as_str())?;
    let streams: i64 = row.try_get(Iden::to_string(&HeldStreams).as_str())?;
    Ok((
        AttributeNameKey::from(attribute),
        HolderType {
            type_key: EntityTypeNameKey::from(type_key),
            streams: streams.unsigned_abs(),
        },
    ))
}

impl TemporalRepository {
    /// Resolve the attribute and type names a compiled query's selection reads
    /// through the dictionaries the write path minted their keys in.
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a dictionary lookup fails.
    pub(crate) async fn resolve_query_names(&self, compiled: &CompiledTemporalQuery) -> Result<QueryNames, TemporalError> {
        // Clause 5.7.4.3 confines a temporal ordering to the `id` member, which
        // is a column of the temporal catalogue and names no attribute, so the
        // dictionary lookup below has no ordering term to resolve. The `attrs`
        // selection, the `pick`/`omit` projection and the attributes an
        // `aggrGroupBy` keys by resolve with the filters, so a statement
        // narrows the history it reads by key inside the source rather than by
        // name above it.
        let grouped = compiled
            .representation
            .grouping
            .iter()
            .flat_map(AggregationGrouping::terms)
            .filter_map(|term| match &term.key {
                GroupKey::Path(path) => Some(path.attr.as_str()),
                GroupKey::Member(_) => None,
            });
        let attribute_names = projected_attribute_names(
            compiled
                .instances
                .attr_filter
                .iter()
                .map(IriBuf::as_str)
                .chain(compiled.entities.projected_members.names())
                .chain(grouped),
            compiled.entities.q_expr.as_ref(),
            compiled.entities.geo.as_ref(),
            &[],
        );
        // The projection is written inside the transaction that writes the
        // entity, so a dictionary hit is already backed by committed projection
        // rows and there is nothing to wait for.
        let attributes = if attribute_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.attribute_names.lookup(&attribute_names).await.map_err(TemporalStoreError::from)?
        };
        // The catalogue carries type keys, so a type selection resolves through
        // the same dictionary the write path minted them in.
        let type_names = projected_type_names(&compiled.entities);
        let types = if type_names.is_empty() {
            ResolvedNames::default()
        } else {
            self.entity_type_names.lookup(&type_names).await.map_err(TemporalStoreError::from)?
        };
        Ok(QueryNames {
            attributes,
            types,
            ..QueryNames::default()
        })
    }

    /// [`resolve_query_names`](Self::resolve_query_names) beside what stage 1
    /// walks the Evolutions of `branch` with: for a selection by attribute
    /// alone, the types the stream census names as holding each attribute
    /// (clause 5.7.4.4 S1).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when a dictionary lookup or the census read
    /// fails.
    pub(crate) async fn resolve_walk_names(&self, branch: &HistoryBranch, compiled: &CompiledTemporalQuery) -> Result<QueryNames, TemporalError> {
        let mut names = self.resolve_query_names(compiled).await?;
        if walks_attribute_holders(compiled) {
            let keys = concerned_attribute_keys(compiled, &names.attributes);
            if !keys.is_empty() {
                let rows = holder_types_query(branch.schema(), &keys)
                    .to_query()
                    .fetch_all(&self.pool)
                    .await
                    .map_err(TemporalStoreError::from)?;
                names.holders = AttributeHolders::new(rows.iter().map(census_holder).collect::<Result<Vec<_>, TemporalStoreError>>()?);
            }
        }
        Ok(names)
    }
}
