//! The total a counted entity query reports (clause 6.3.13): how the store
//! reads it, and the statement that counts it when the page cannot carry it.

use crate::{
    census_total::{census_counted_type, census_total},
    entity::{GeoFilter, QFilter, build_entity_conditions},
    entity_page::GatedStep,
    entity_relations::EntityRelations,
    keyed_count::{CountedRead, count_bound, keyed_count},
    keyed_page::page_bound as keyed_page_bound,
};
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, entity::compiled::CompiledEntityQuery, standing_instant::StandingInstant};
use andromeda_postgresql_query::{
    entity_row_refs::EntityRowRefs,
    error::RenderError,
    id_pattern_trigrams::ID_PATTERN_CANDIDATE_BOUND,
    one_time_gate::CandidateBound,
    q_predicate::keyed_term::{CandidateRead, KeyedCandidates},
    query_names::QueryNames,
};
use sea_query::{Asterisk, Condition, DynIden, Expr, Func, Query, SelectStatement};

/// How the store reads the total a counted entity query reports (clause
/// 6.3.13).
pub enum EntityTotal {
    /// The query asked for no total.
    Omitted,
    /// The rows of an offset page carry the total, so the common case runs
    /// in one round-trip: every row of a plain page, the rows of a gated page
    /// when its index branch answered. An empty page, or a gated page another
    /// branch answered, carries none, and only then is the count run.
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
/// steps of the page after which it is the count run.
///
/// A short walk that fills the page found the matches dense where the
/// selection starts. A keyed page's short walk of 1 000 fills a page of a
/// hundred for a term about a tenth of the selection holds, which among
/// 500 000 Entities passes the count's candidate bound, so reading the
/// candidates first would only find too many of them. In a smaller selection, or behind the 5 000 identifiers
/// of an `idPattern` page, the term can stay within the bound, and the walk
/// then costs what the candidates would save. A pattern 5 000 of 1 000 000
/// identifiers match counts in 86 ms walked against 21 ms from its
/// candidates, and a value pattern a fifth of 10 000 Entities match in 11 ms
/// against 3 ms. A term whose few matches cluster at the start is walked too,
/// at the cost the walk has without the candidates. A page cut by its full
/// walk found more candidates than its bound, which passes the count's own
/// bound whenever that is no larger. A page its continued walk filled found
/// the matches a few in a hundred, and its term can stay within the count's
/// bound. Among 80 000 Vehicles `speed>107;status=="moving"` keeps 3.6%, and
/// its 8 672 `speed` candidates count without walking the selection.
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
            (Some(GatedStep::ShortWalk | GatedStep::Candidates | GatedStep::FullWalk | GatedStep::ContinuedWalk) | None, _) => self.count,
        }
    }
}

/// The candidates a total reads its matches from, in place of walking the
/// selection.
pub(crate) enum TotalCandidates<'q> {
    /// The terms of `q` an index answers, in the order the page reads them,
    /// and their candidates.
    Terms(Vec<KeyedCandidates<'q>>),
    /// The trigram matches of an `idPattern`, the selection's one filter,
    /// each rechecked against the pattern as it is read.
    IdPattern(Box<SelectStatement>),
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
            count: selection_count(source, relations, whole),
            walked: None,
        },
        Some(candidates) => {
            let (reads, after) = match candidates {
                TotalCandidates::Terms(terms) => {
                    let after = walked_after(terms.iter().map(|keyed| keyed.read), keyed_page_bound);
                    let reads = terms
                        .into_iter()
                        .map(|KeyedCandidates { term, select, read, .. }| {
                            Ok(CountedRead {
                                remainder: build_entity_conditions(
                                    source.query,
                                    source.names,
                                    source.rows,
                                    &relations.source,
                                    QFilter::Except(term),
                                    GeoFilter::Probed,
                                )?,
                                candidates: select,
                                read,
                            })
                        })
                        .collect::<Result<Vec<CountedRead>, RenderError>>()?;
                    (reads, after)
                }
                TotalCandidates::IdPattern(select) => {
                    let read = CountedRead {
                        remainder: Condition::all(),
                        candidates: *select,
                        read: CandidateRead::Rechecked,
                    };
                    let after = walked_after([read.read], |_| CandidateBound::AtMost(ID_PATTERN_CANDIDATE_BOUND));
                    (vec![read], after)
                }
            };
            let walked = count_over(relations.clone(), source.schema, source.rows.standing(), whole.clone());
            TotalCount {
                count: keyed_count(&relations, source.schema, source.rows, reads, whole),
                walked: Some(WalkedCount { select: walked, after }),
            }
        }
    };
    Ok(match counted_on_page {
        CountPolicy::Emit => EntityTotal::OnPageRows { uncounted: total },
        CountPolicy::Omit => EntityTotal::Counted(total),
    })
}

/// The steps of a gated page after which its total is counted by walking,
/// for a page that read `reads` in turn, each under `page_bound`.
///
/// A page cut by its full walk found more candidates than every read's page
/// bound, which passes every count bound no larger, so the count would only
/// read them all to walk; a read whose count bound is larger may still
/// answer the count.
fn walked_after(reads: impl IntoIterator<Item = CandidateRead>, page_bound: impl Fn(CandidateRead) -> CandidateBound) -> &'static [GatedStep] {
    if reads.into_iter().all(|read| count_bound(read).reads_no_more_than(page_bound(read))) {
        &[GatedStep::ShortWalk, GatedStep::FullWalk]
    } else {
        &[GatedStep::ShortWalk]
    }
}

/// The statement counting every match of `source`'s query under `whole` with
/// no candidates to read: the census count of a selection by one type alone
/// ([`census_total`]), `SELECT COUNT(*)` over `relations` otherwise.
pub(crate) fn selection_count(source: &TotalSource<'_>, relations: EntityRelations, whole: Condition) -> SelectStatement {
    match census_counted_type(source.query, source.names) {
        Some(entity_type) => census_total(entity_type, source.schema, source.rows.standing()),
        None => count_over(relations, source.schema, source.rows.standing(), whole),
    }
}

/// `SELECT COUNT(*)` over `relations` in the branch `schema` (`None` for the
/// live branch), the driving Entity standing at `standing`, filtered by
/// `condition`.
///
/// A type walk of several alternatives is counted over the union of its
/// disjoint streams, each read in the order of its range: counted as their
/// merge, the members would be sorted whole first.
pub(crate) fn count_over(relations: EntityRelations, schema: Option<&DynIden>, standing: StandingInstant, condition: Condition) -> SelectStatement {
    let mut count = Query::select();
    count.expr(Func::count(Expr::col(Asterisk)));
    relations.apply_from(&mut count, schema, standing);
    count.cond_where(condition);
    count.take()
}

#[cfg(test)]
mod tests {
    use crate::{
        entity::build_query,
        entity_page::GatedStep,
        entity_total::{EntityTotal, TotalCount, WalkedCount, walked_after},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        entity::compiled::CompiledEntityQuery,
        entity_selection::EntitySelection,
        filter::{Filter, compiled_filter::CompiledFilter, language_matching::LanguageMatching},
        name_stage::Written,
        namespace_expander::EXAMPLE_NAMESPACE,
        page_window::PageWindow,
    };
    use andromeda_postgresql_query::{
        id_pattern_trigrams::ID_PATTERN_CANDIDATE_BOUND,
        one_time_gate::CandidateBound,
        q_predicate::keyed_term::CandidateRead,
        query_names::QueryNames,
    };
    use andromeda_postgresql_seaquery::assert_statement_snapshot;
    use andromeda_postgresql_stored_values::{attribute_name_key::AttributeNameKey, resolved_names::ResolvedNames};
    use pretty_assertions::assert_eq;
    use sea_query::{Alias, Query, SelectStatement};

    fn from(table: &str) -> SelectStatement {
        Query::select().column(Alias::new("x")).from(Alias::new(table)).take()
    }

    fn gated(after: &'static [GatedStep]) -> TotalCount {
        TotalCount {
            count: from("counted"),
            walked: Some(WalkedCount { select: from("walked"), after }),
        }
    }

    #[test]
    fn a_page_its_short_walk_answered_is_counted_by_walking() {
        assert_statement_snapshot!(
            "entity-total-a-page-its-short-walk-answered-is-counted-by-walking",
            gated(&[GatedStep::ShortWalk]).for_step(Some(GatedStep::ShortWalk))
        );
    }

    #[test]
    fn a_page_another_step_answered_or_an_empty_page_reads_the_candidates() {
        for (index, step) in [Some(GatedStep::Candidates), Some(GatedStep::FullWalk), None, Some(GatedStep::ContinuedWalk)]
            .into_iter()
            .enumerate()
        {
            assert_statement_snapshot!(
                format!("entity-total-a-page-another-step-answered-or-an-empty-page-reads-the-candidates-{}", index + 1),
                gated(&[GatedStep::ShortWalk]).for_step(step)
            );
        }
    }

    #[test]
    fn a_page_its_full_walk_answered_is_walked_when_the_walk_follows_it() {
        let after = &[GatedStep::ShortWalk, GatedStep::FullWalk];
        assert_statement_snapshot!(
            "entity-total-a-page-its-full-walk-answered-is-walked-when-the-walk-follows-it-1",
            gated(after).for_step(Some(GatedStep::FullWalk))
        );
        assert_statement_snapshot!(
            "entity-total-a-page-its-full-walk-answered-is-walked-when-the-walk-follows-it-2",
            gated(after).for_step(Some(GatedStep::Candidates))
        );
    }

    fn after_of(q: &str) -> &'static [GatedStep] {
        let mut query = CompiledEntityQuery::builder().selection(EntitySelection::builder().build()).build();
        query.selection.q = Some(CompiledFilter::new(
            EXAMPLE_NAMESPACE
                .expanded(q.parse::<Filter<Written>>().expect("valid q"))
                .expect("the names expand"),
            LanguageMatching::TagEquality,
        ));
        let names = QueryNames {
            attributes: ResolvedNames::from_pairs(
                [("temperature", 12), ("observedOn", 13)]
                    .map(|(name, key)| (format!("https://example.com/{name}").parse().expect("valid name"), AttributeNameKey::from(key))),
            ),
            types: ResolvedNames::default(),
            ..QueryNames::default()
        };
        let built = build_query(&ContextTarget::Live, &query, &PageWindow::offset(100, 0), CountPolicy::Emit, &names).expect("valid entity query");
        let EntityTotal::OnPageRows { uncounted } = built.total else {
            panic!("a first keyed page carries its total when its candidates answer");
        };
        uncounted.walked.expect("a keyed page has a walked count").after
    }

    /// Every keyed read counts as many candidates as its page reads, so a page
    /// past every bound is counted by walking. An identifier pattern's page
    /// reads 1,000 matches and its count 10,000, so a page past its bound may
    /// still be counted from them.
    #[test]
    fn a_full_walk_is_counted_by_walking_only_when_every_count_reads_no_more_candidates_than_its_page() {
        for q in [
            "\"https://example.com/temperature\"~=\"text-71.*\"",
            "\"https://example.com/temperature\">2024-01-15T12:00:00Z",
            "\"https://example.com/temperature\"~=\"text-71.*\";\"https://example.com/observedOn\">2024-01-15T12:00:00Z",
        ] {
            assert_eq!(after_of(q), &[GatedStep::ShortWalk, GatedStep::FullWalk], "{q}");
        }
        assert_eq!(
            walked_after([CandidateRead::Rechecked], |_| CandidateBound::AtMost(ID_PATTERN_CANDIDATE_BOUND)),
            &[GatedStep::ShortWalk]
        );
    }

    #[test]
    fn a_count_without_a_walk_runs_whatever_the_step() {
        let count = TotalCount {
            count: from("counted"),
            walked: None,
        };
        assert_statement_snapshot!(
            "entity-total-a-count-without-a-walk-runs-whatever-the-step",
            count.for_step(Some(GatedStep::ShortWalk))
        );
    }

    #[test]
    fn each_step_is_read_back_from_the_value_its_branch_names_itself_by() {
        for (value, step) in [
            (1, GatedStep::ShortWalk),
            (2, GatedStep::Candidates),
            (3, GatedStep::FullWalk),
            (4, GatedStep::ContinuedWalk),
        ] {
            assert_eq!(GatedStep::try_from(value).ok(), Some(step));
        }
        assert!(GatedStep::try_from(0_i16).is_err());
    }
}
