//! The total a counted entity query reports (clause 6.3.13): how the store
//! reads it, and the statement that counts it when the page cannot carry it.

use crate::{
    entity::{QFilter, build_entity_conditions},
    entity_relations::EntityRelations,
    keyed_count::{KeyedCountFilters, keyed_count},
};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery, filter::predicate::Predicate};
use andromeda_postgresql_query::{dictionary::query_names::QueryNames, entity_row_refs::EntityRowRefs, error::RenderError};
use sea_query::{Asterisk, Condition, DynIden, Expr, Func, Query, SelectStatement};

/// How the store reads the total a counted entity query reports (clause
/// 6.3.13).
pub enum EntityTotal {
    /// The query asked for no total.
    Omitted,
    /// Every row of an offset page carries the total as `COUNT(*) OVER ()`,
    /// so the common case runs in one round-trip. An empty page carries no row
    /// to read it from, and only then is this statement run.
    OnPageRows {
        /// The count run when the page comes back empty.
        empty_page: SelectStatement,
    },
    /// The page is a keyset window (clause 5.5.9.1), whose predicate keeps only
    /// the entities past its cursor, so a window count over it would count
    /// those alone, or a keyed page, whose branches each read part of the
    /// selection. The total is always this separate statement over the whole
    /// selection.
    Counted(SelectStatement),
}

/// The query a total counts the matches of, and how its fragments reach the
/// rows.
pub(crate) struct TotalSource<'a> {
    /// The query.
    pub(crate) query: &'a CompiledEntityQuery,
    /// The dictionary keys the query reads.
    pub(crate) names: &'a QueryNames,
    /// How the statement's fragments reach the rows.
    pub(crate) rows: &'a EntityRowRefs,
    /// The branch schema, `None` for the live branch.
    pub(crate) schema: Option<&'a DynIden>,
}

/// The term of `q` an index answers and its candidates, which a count reads
/// in place of probing every Entity for the term.
pub(crate) struct KeyedTotal<'q> {
    /// The term the candidates answer.
    pub(crate) term: &'q Predicate,
    /// `SELECT "entity_key" …`, every candidate.
    pub(crate) candidates: SelectStatement,
}

/// The total `count` asks for, carried on the page's rows when
/// `counted_on_page` says the page can, counted over `whole`, the query's
/// whole filter, or through `keyed` when a term of `q` has candidates.
///
/// # Errors
///
/// Fails as rendering the query's filter fails: a reference geometry with no
/// EWKB spelling, or a query-language rule a renderer decides first.
pub(crate) fn entity_total(
    source: &TotalSource<'_>,
    count: CountPolicy,
    counted_on_page: CountPolicy,
    whole: Condition,
    keyed: Option<KeyedTotal<'_>>,
) -> Result<EntityTotal, RenderError> {
    match count {
        CountPolicy::Omit => return Ok(EntityTotal::Omitted),
        CountPolicy::Emit => {}
    }
    let relations = EntityRelations::for_count(source.query, &source.names.types, source.schema);
    let count_select = match keyed {
        None => count_over(relations, source.schema, whole),
        Some(KeyedTotal { term, candidates }) => {
            let filters = KeyedCountFilters {
                remainder: build_entity_conditions(source.query, source.names, source.rows, &relations.source, QFilter::Except(term))?,
                whole,
            };
            keyed_count(&relations, source.schema, source.rows, filters, candidates)
        }
    };
    Ok(match counted_on_page {
        CountPolicy::Emit => EntityTotal::OnPageRows { empty_page: count_select },
        CountPolicy::Omit => EntityTotal::Counted(count_select),
    })
}

/// `SELECT COUNT(*)` over `relations` in the branch `schema` (`None` for the
/// live branch), filtered by `condition`.
pub(crate) fn count_over(relations: EntityRelations, schema: Option<&DynIden>, condition: Condition) -> SelectStatement {
    let mut count = Query::select();
    count.expr(Func::count(Expr::col(Asterisk)));
    relations.apply_from(&mut count, schema);
    count.cond_where(condition);
    count.take()
}
