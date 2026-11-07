//! Finishing a query a store did not finish.
//!
//! A store that declined a predicate returns the rows its remaining predicates
//! admitted, plus the predicates it left behind. The clause 5.7.2 answer is the
//! intersection, so the caller evaluates the remainder in memory and only then
//! takes the clause 5.5.9 window. A store that defers the window reads the
//! whole selection in its order, so the window, offset or cursor, is cut
//! from it exactly as the store would have cut it.

use crate::{
    entity_invariant::EntityInvariant,
    error::EntityError,
    pagination_outcome::PaginationOutcome,
    port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
    query::EntityQueryService,
    query_outcome::QueryOutcome,
    query_residual::ResidualFilter,
    residual_filter,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::{count_policy::CountPolicy, page_window::PageWindow};

/// The rows a query answers with, and the clause 6.3.13 count when one was
/// asked for.
#[derive(Debug)]
pub(super) struct FinishedPage {
    /// The stored states of the page's matching entities, each with its
    /// entity-level system times, or its identifier in its place when it does
    /// not read back.
    pub rows: Vec<StoredEntry<RetrievedEntity>>,
    /// Total number of matches, present when the query asked to be counted.
    pub total_count: Option<i64>,
}

/// Finish an outcome the store may not have finished.
///
/// # Errors
///
/// Returns [`EntityInvariant::IncoherentResidual`] when a store reports both an applied
/// window and an outstanding predicate. Those cannot both be true: a window
/// taken over rows a later filter will thin leaves the page short and every
/// subsequent page misaligned, so the combination is a broken store rather than
/// a case to compensate for.
pub(super) fn finish_page(outcome: QueryOutcome<StoredEntry<RetrievedEntity>>, window: &PageWindow, count: CountPolicy) -> Result<FinishedPage, EntityError> {
    let QueryOutcome {
        rows,
        unapplied,
        pagination: applied,
    } = outcome;

    match applied {
        PaginationOutcome::Applied { total } => {
            if !unapplied.is_empty() {
                return Err(incoherent(&unapplied));
            }
            Ok(FinishedPage { rows, total_count: total })
        }
        PaginationOutcome::Deferred => {
            // An entity that does not read back cannot be judged, so it stays
            // where the store read it and is reported rather than dropped
            // unseen.
            let admitted: Vec<StoredEntry<RetrievedEntity>> = rows
                .into_iter()
                .filter(|row| match row {
                    StoredEntry::Read(row) => residual_filter::matches(&unapplied, &row.state),
                    StoredEntry::Unreadable(_) => true,
                })
                .collect();
            Ok(cut_window(admitted, window, count))
        }
    }
}

/// Take the clause 5.5.9 window over rows that have already passed the
/// residual, counting the survivors first.
///
/// Clause 6.3.13 counts matching entities rather than returned ones, so the
/// count is taken before the window and after the filter.
fn cut_window(admitted: Vec<StoredEntry<RetrievedEntity>>, window: &PageWindow, count: CountPolicy) -> FinishedPage {
    let total_count = match count {
        CountPolicy::Emit => Some(i64::try_from(admitted.len()).unwrap_or(i64::MAX)),
        CountPolicy::Omit => None,
    };
    let rows = window.cut(admitted, |row| row.id(|row| row.state.id()).as_str());
    FinishedPage { rows, total_count }
}

/// The error a store earns by reporting an applied window alongside a residual.
fn incoherent(unapplied: &ResidualFilter) -> EntityError {
    EntityInvariant::IncoherentResidual { unapplied: unapplied.kinds() }.into()
}

impl EntityQueryService {
    /// Finish an identifier-only outcome (clause 5.2.39 entity maps).
    ///
    /// A predicate cannot be evaluated against an identifier, so an outstanding
    /// one costs a state read for every identifier the store returned. That is
    /// the honest price of an entity-map query against a store that could not
    /// push the predicate down.
    pub(super) async fn finish_ids(
        &self,
        target: &ContextTarget,
        outcome: QueryOutcome<Identifier>,
        window: &PageWindow,
    ) -> Result<Vec<Identifier>, EntityError> {
        let QueryOutcome {
            rows,
            unapplied,
            pagination: applied,
        } = outcome;

        match applied {
            PaginationOutcome::Applied { total: _ } => {
                if unapplied.is_empty() {
                    Ok(rows)
                } else {
                    Err(incoherent(&unapplied))
                }
            }
            PaginationOutcome::Deferred => {
                if unapplied.is_empty() {
                    return Ok(window.cut(rows, Identifier::as_str));
                }
                let states = self.reader.get_batch(target, &rows).await?;
                let admitted = rows
                    .into_iter()
                    .filter(|id| states.get(id).is_some_and(|retrieved| residual_filter::matches(&unapplied, &retrieved.state)))
                    .collect();
                Ok(window.cut(admitted, Identifier::as_str))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        pagination_outcome::PaginationOutcome,
        port::{retrieved_entity::RetrievedEntity, stored_entry::StoredEntry},
        query::residual::{FinishedPage, finish_page},
        query_outcome::QueryOutcome,
        query_residual::ResidualFilter,
    };
    use andromeda_ngsi_ld_entity::entity_state::EntityState;
    use andromeda_ngsi_ld_model::notification::SysAttrsSnapshot;
    use andromeda_ngsi_ld_query::{
        count_policy::CountPolicy,
        cursor::PageCursor,
        entity_selection::EntitySelection,
        filter::parser,
        page_window::{PageStart, PageWindow},
    };
    use chrono::{DateTime, Utc};
    use serde_json::json;
    use std::sync::Arc;

    const TEMPERATURE: &str = "https://example.com/temperature";

    fn entity(temperature: f64) -> EntityState {
        let document = json!([{
            "@id": format!("urn:ngsi-ld:Building:{temperature}"),
            "@type": ["https://example.com/Building"],
            TEMPERATURE: [{"@type": ["https://uri.etsi.org/ngsi-ld/Property"], "https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}]
        }]);
        EntityState::from_slice(&serde_json::to_vec(&document).expect("JSON")).expect("a stored state")
    }

    fn retrieved(temperature: f64) -> StoredEntry<RetrievedEntity> {
        StoredEntry::Read(RetrievedEntity {
            state: Arc::new(entity(temperature)),
            sys_attrs: SysAttrsSnapshot {
                created_at: DateTime::<Utc>::UNIX_EPOCH,
                modified_at: DateTime::<Utc>::UNIX_EPOCH,
                expires_at: None,
            },
        })
    }

    /// Five entities at 10, 20, 30, 40 and 50 degrees, as a store reads them.
    fn five() -> Vec<StoredEntry<RetrievedEntity>> {
        [10.0, 20.0, 30.0, 40.0, 50.0].into_iter().map(retrieved).collect()
    }

    /// The states a finished page holds, in page order.
    fn states(finished: &FinishedPage) -> Vec<&EntityState> {
        finished
            .rows
            .iter()
            .filter_map(|row| match row {
                StoredEntry::Read(row) => Some(row.state.as_ref()),
                StoredEntry::Unreadable(_) => None,
            })
            .collect()
    }

    /// A residual admitting the three entities above 25 degrees.
    fn above_25() -> ResidualFilter {
        ResidualFilter::over(
            EntitySelection::builder()
                .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q filter")))
                .build(),
        )
    }

    fn page(limit: u32, offset: u32) -> PageWindow {
        PageWindow::offset(limit, offset)
    }

    #[test]
    fn an_applied_page_with_no_residual_is_returned_unchanged() {
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Applied { total: Some(5) },
        };
        let finished = finish_page(outcome, &page(20, 0), CountPolicy::Emit).expect("coherent outcome");
        pretty_assertions::assert_eq!(finished.rows.len(), 5);
        pretty_assertions::assert_eq!(finished.total_count, Some(5));
    }

    #[test]
    fn an_applied_window_alongside_an_outstanding_predicate_is_refused() {
        // The window was taken over rows the residual will thin, so the page is
        // short and every later page skips entities that should have appeared.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Applied { total: Some(5) },
        };
        let error = finish_page(outcome, &page(20, 0), CountPolicy::Emit).expect_err("must refuse");
        assert!(error.to_string().contains('q'), "the message names the outstanding predicate: {error}");
    }

    #[test]
    fn a_deferred_page_is_windowed_after_the_residual_runs() {
        // Clause 5.5.9: the window is over matching entities, and only three of
        // the five match once the residual has run.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, &page(2, 1), CountPolicy::Omit).expect("coherent outcome");
        pretty_assertions::assert_eq!(states(&finished), vec![&entity(40.0), &entity(50.0)]);
    }

    #[test]
    fn a_deferred_count_is_the_number_that_survived_not_the_number_returned() {
        // Clause 6.3.13 counts matches, so the count is three even though the
        // page holds two and the store returned five.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, &page(2, 0), CountPolicy::Emit).expect("coherent outcome");
        pretty_assertions::assert_eq!(finished.rows.len(), 2);
        pretty_assertions::assert_eq!(finished.total_count, Some(3));
    }

    #[test]
    fn a_deferred_page_emits_no_count_when_none_was_asked_for() {
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, &page(2, 0), CountPolicy::Omit).expect("coherent outcome");
        pretty_assertions::assert_eq!(finished.total_count, None);
    }

    #[test]
    fn a_deferred_offset_past_the_surviving_set_returns_no_rows_and_still_counts() {
        // Clause 5.5.9: an offset beyond the result set is an empty page, not
        // an error, and clause 6.3.13's count is unaffected by it.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, &page(20, 10), CountPolicy::Emit).expect("coherent outcome");
        assert!(finished.rows.is_empty(), "the page holds {} rows", finished.rows.len());
        pretty_assertions::assert_eq!(finished.total_count, Some(3));
    }

    #[test]
    fn a_deferred_page_with_an_empty_residual_is_still_windowed() {
        // A store may defer the window without declining a predicate, which is
        // what a backend with no `OFFSET` would do.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, &page(2, 0), CountPolicy::Emit).expect("coherent outcome");
        pretty_assertions::assert_eq!(states(&finished), vec![&entity(10.0), &entity(20.0)]);
        pretty_assertions::assert_eq!(finished.total_count, Some(5));
    }

    /// Ten entities at 0 to 9 degrees, as a store reads them in identifier
    /// order (`urn:ngsi-ld:Building:0` to `urn:ngsi-ld:Building:9`).
    fn ten() -> Vec<StoredEntry<RetrievedEntity>> {
        (0..10).map(|temperature| retrieved(f64::from(temperature))).collect()
    }

    /// Clause 5.5.9.1: a deferred backward cursor page is cut from the whole
    /// selection the residual left, the rows closest before its boundary, and
    /// the count is still the whole selection's.
    #[test]
    fn a_deferred_backward_cursor_page_is_cut_from_the_whole_selection() {
        let outcome = QueryOutcome {
            rows: ten(),
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Deferred,
        };
        let window = PageWindow {
            limit: 3,
            start: PageStart::Cursor(Some(PageCursor::Before("urn:ngsi-ld:Building:6".parse().expect("valid URI")))),
        };
        let finished = finish_page(outcome, &window, CountPolicy::Emit).expect("coherent outcome");
        pretty_assertions::assert_eq!(states(&finished), vec![&entity(3.0), &entity(4.0), &entity(5.0)]);
        pretty_assertions::assert_eq!(finished.total_count, Some(10));
    }

    /// An entity that does not read back cannot be judged by a residual, so a
    /// deferred page keeps it in its place, where it is reported.
    #[test]
    fn a_deferred_page_keeps_an_unreadable_entity_in_its_place() {
        let mut rows = five();
        rows.insert(2, StoredEntry::Unreadable("urn:ngsi-ld:Building:broken".parse().expect("a valid URI")));
        let outcome = QueryOutcome {
            rows,
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, &page(20, 0), CountPolicy::Emit).expect("coherent outcome");
        assert!(matches!(
            finished.rows.as_slice(),
            [StoredEntry::Unreadable(id), StoredEntry::Read(_), StoredEntry::Read(_), StoredEntry::Read(_)] if id.as_str() == "urn:ngsi-ld:Building:broken"
        ));
        pretty_assertions::assert_eq!(states(&finished), vec![&entity(30.0), &entity(40.0), &entity(50.0)]);
    }
}
