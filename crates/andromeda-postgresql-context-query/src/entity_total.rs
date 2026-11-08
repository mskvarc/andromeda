//! The total a counted entity query reports (clause 6.3.13): how the store
//! reads it, and the statement that counts it when the page cannot carry it.

use crate::{
    entity::{QFilter, build_entity_conditions},
    entity_page::GatedStep,
    entity_relations::EntityRelations,
    keyed_count::{KeyedCountFilters, count_bound, keyed_count},
    keyed_page::page_bound as keyed_page_bound,
};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery};
use andromeda_postgresql_query::{
    dictionary::query_names::QueryNames,
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    id_pattern_trigrams::ID_PATTERN_CANDIDATE_BOUND,
    one_time_gate::CandidateBound,
    q_predicate::keyed_term::{CandidateRead, KeyedCandidates},
};
use sea_query::{Asterisk, Condition, DynIden, Expr, Func, Query, SelectStatement};

/// How the store reads the total a counted entity query reports (clause
/// 6.3.13).
pub enum EntityTotal {
    /// The query asked for no total.
    Omitted,
    /// The rows of an offset page carry the total as `COUNT(*) OVER ()`, so
    /// the common case runs in one round-trip: every row of a plain page, the
    /// rows of a gated page when its index branch answered. An empty page,
    /// or a gated page another branch answered, carries none, and only then
    /// is the count run.
    OnPageRows {
        /// The count run when the page carries no total.
        uncounted: TotalCount,
    },
    /// The page is a keyset window (clause 5.5.9.1), whose predicate keeps only
    /// the entities past its cursor, so a window count over it would count
    /// those alone, or a value walk page, whose branches each read part of the
    /// selection. The total is always this separate count over the whole
    /// selection.
    Counted(TotalCount),
}

/// The statements that count a total its page does not carry.
pub struct TotalCount {
    /// The count over the whole selection.
    pub count: SelectStatement,
    /// The count run instead after some steps of a keyed page, `None` for a
    /// page whose rows name no gated step.
    pub walked: Option<WalkedCount>,
}

/// The selection walked and counted without reading candidates, and the
/// steps of the page after which that is the cheaper count.
///
/// A short walk that fills the page found the matches dense where the
/// selection starts, and a dense term almost always passes the count's
/// candidate bound, so reading the candidates first would only find too many
/// of them; a term whose few matches cluster at the start is walked too, at
/// the cost the walk has without the candidates. A page cut by its full walk
/// found more candidates than its bound, which passes the count's own bound
/// whenever that is no larger.
pub struct WalkedCount {
    /// `SELECT COUNT(*)` over the selection under the whole filter.
    pub select: SelectStatement,
    /// The steps whose page this count is run for.
    pub after: &'static [GatedStep],
}

impl TotalCount {
    /// The count to run for a page the gated `step` answered, `None` when
    /// its rows name none.
    #[must_use]
    pub fn for_step(self, step: Option<GatedStep>) -> SelectStatement {
        match (step, self.walked) {
            (Some(step), Some(walked)) if walked.after.contains(&step) => walked.select,
            (Some(GatedStep::ShortWalk | GatedStep::Candidates | GatedStep::FullWalk) | None, _) => self.count,
        }
    }
}

/// The candidates a total reads its matches from, in place of walking the
/// selection.
pub(crate) enum TotalCandidates<'q> {
    /// The term of `q` an index answers, and its candidates.
    Term(KeyedCandidates<'q>),
    /// The trigram matches of an `idPattern`, the selection's one filter,
    /// each rechecked against the pattern as it is read.
    IdPattern(SelectStatement),
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

/// The total `count` asks for, carried on the page's rows when
/// `counted_on_page` says the page can, counted over `whole`, the query's
/// whole filter, or from `candidates`, which the count reads in place of
/// probing every Entity for what they answer, the selection walked instead
/// when the page's short walk answered.
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
    candidates: Option<TotalCandidates<'_>>,
) -> Result<EntityTotal, RenderError> {
    match count {
        CountPolicy::Omit => return Ok(EntityTotal::Omitted),
        CountPolicy::Emit => {}
    }
    let relations = EntityRelations::for_count(source.query, &source.names.types, source.schema);
    let total = match candidates {
        None => TotalCount {
            count: count_over(relations, source.schema, whole),
            walked: None,
        },
        Some(candidates) => {
            let (remainder, select, read, page_bound) = match candidates {
                TotalCandidates::Term(KeyedCandidates { term, select, read }) => (
                    build_entity_conditions(source.query, source.names, source.rows, &relations.source, QFilter::Except(term))?,
                    select,
                    read,
                    keyed_page_bound(read),
                ),
                TotalCandidates::IdPattern(select) => (
                    Condition::all(),
                    select,
                    CandidateRead::Rechecked,
                    CandidateBound::AtMost(ID_PATTERN_CANDIDATE_BOUND),
                ),
            };
            let after: &'static [GatedStep] = if count_bound(read).reads_no_more_than(page_bound) {
                &[GatedStep::ShortWalk, GatedStep::FullWalk]
            } else {
                &[GatedStep::ShortWalk]
            };
            let walked = count_over(relations.clone(), source.schema, whole.clone());
            let filters = KeyedCountFilters { remainder, whole };
            TotalCount {
                count: keyed_count(&relations, source.schema, source.rows, filters, select, read),
                walked: Some(WalkedCount { select: walked, after }),
            }
        }
    };
    Ok(match counted_on_page {
        CountPolicy::Emit => EntityTotal::OnPageRows { uncounted: total },
        CountPolicy::Omit => EntityTotal::Counted(total),
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

#[cfg(test)]
mod tests {
    use crate::{
        entity::build_query,
        entity_page::GatedStep,
        entity_total::{EntityTotal, TotalCount, WalkedCount},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        filter::parser::parse,
        page_window::PageWindow,
    };
    use andromeda_postgresql_query::{
        attribute_name_key::AttributeNameKey,
        dictionary::{query_names::QueryNames, resolved_names::ResolvedNames},
    };
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, PostgresQueryBuilder, Query, SelectStatement};

    fn from(table: &str) -> SelectStatement {
        Query::select().column(Alias::new("x")).from(Alias::new(table)).take()
    }

    fn gated(after: &'static [GatedStep]) -> TotalCount {
        TotalCount {
            count: from("counted"),
            walked: Some(WalkedCount { select: from("walked"), after }),
        }
    }

    fn table_of(count: &SelectStatement) -> String {
        count.to_string(PostgresQueryBuilder)
    }

    #[test]
    fn a_page_its_short_walk_answered_is_counted_by_walking() {
        assert_eq!(
            table_of(&gated(&[GatedStep::ShortWalk]).for_step(Some(GatedStep::ShortWalk))),
            r#"SELECT "x" FROM "walked""#
        );
    }

    #[test]
    fn a_page_another_step_answered_or_an_empty_page_reads_the_candidates() {
        for step in [Some(GatedStep::Candidates), Some(GatedStep::FullWalk), None] {
            assert_eq!(
                table_of(&gated(&[GatedStep::ShortWalk]).for_step(step)),
                r#"SELECT "x" FROM "counted""#,
                "{step:?}"
            );
        }
    }

    #[test]
    fn a_page_its_full_walk_answered_is_walked_when_the_walk_follows_it() {
        let after = &[GatedStep::ShortWalk, GatedStep::FullWalk];
        assert_eq!(table_of(&gated(after).for_step(Some(GatedStep::FullWalk))), r#"SELECT "x" FROM "walked""#);
        assert_eq!(table_of(&gated(after).for_step(Some(GatedStep::Candidates))), r#"SELECT "x" FROM "counted""#);
    }

    fn after_of(q: &str, attribute: &str) -> &'static [GatedStep] {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q_expr = Some(parse(q).expect("valid q"));
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs([(attribute.parse().expect("valid name"), AttributeNameKey::from(12))]),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        };
        let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Emit, &names).expect("valid entity query");
        let EntityTotal::OnPageRows { uncounted } = built.total else {
            panic!("a first keyed page carries its total when its candidates answer");
        };
        uncounted.walked.expect("a keyed page has a walked count").after
    }

    /// A rechecked term's page reads 1,000 candidates and its count 10,000,
    /// so a page past its bound may still be counted from candidates; a
    /// ranged term's page and count read the same bound, so its count walks.
    #[test]
    fn a_full_walk_is_counted_by_walking_only_when_the_count_reads_no_more_candidates_than_the_page() {
        assert_eq!(
            after_of("\"https://example.com/temperature\"~=\"text-71.*\"", "https://example.com/temperature"),
            &[GatedStep::ShortWalk]
        );
        assert_eq!(
            after_of("\"https://example.com/temperature\">2024-01-15T12:00:00Z", "https://example.com/temperature"),
            &[GatedStep::ShortWalk, GatedStep::FullWalk]
        );
    }

    #[test]
    fn a_count_without_a_walk_runs_whatever_the_step() {
        let count = TotalCount {
            count: from("counted"),
            walked: None,
        };
        assert_eq!(table_of(&count.for_step(Some(GatedStep::ShortWalk))), r#"SELECT "x" FROM "counted""#);
    }

    #[test]
    fn each_step_is_read_back_from_the_value_its_branch_names_itself_by() {
        for (value, step) in [(1, GatedStep::ShortWalk), (2, GatedStep::Candidates), (3, GatedStep::FullWalk)] {
            assert_eq!(GatedStep::try_from(value).ok(), Some(step));
        }
        assert!(GatedStep::try_from(0_i16).is_err());
    }
}
