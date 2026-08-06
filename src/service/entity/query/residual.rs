//! Finishing a query a store did not finish.
//!
//! A store that declined a predicate returns the rows its remaining predicates
//! admitted, plus the predicates it left behind. The clause 5.7.2 answer is the
//! intersection, so the caller evaluates the remainder in memory and only then
//! takes the clause 5.5.6 window.

use crate::{
    core::error::AndromedaError,
    service::entity::{pagination_outcome::PaginationOutcome, query::EntityQueryService, query_outcome::QueryOutcome, residual_filter::ResidualFilter},
};
use andromeda_ngsi_ld_query::pagination::{CountPolicy, Pagination};
use serde_json::Value;

/// The rows a query answers with, and the clause 6.3.13 count when one was
/// asked for.
#[derive(Debug)]
pub(super) struct FinishedPage {
    /// The page of matching entities.
    pub rows: Vec<Value>,
    /// Total number of matches, present when the query asked to be counted.
    pub total_count: Option<i64>,
}

/// Finish an outcome the store may not have finished.
///
/// # Errors
///
/// Returns [`AndromedaError::internal`] when a store reports both an applied
/// window and an outstanding predicate. Those cannot both be true: a window
/// taken over rows a later filter will thin leaves the page short and every
/// subsequent page misaligned, so the combination is a broken store rather than
/// a case to compensate for.
pub(super) fn finish_page(outcome: QueryOutcome<Value>, pagination: Pagination) -> Result<FinishedPage, AndromedaError> {
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
            let admitted: Vec<Value> = rows.into_iter().filter(|row| unapplied.matches(row)).collect();
            Ok(window(admitted, pagination))
        }
    }
}

/// Take the clause 5.5.6 window over rows that have already passed the
/// residual, counting the survivors first.
///
/// Clause 6.3.13 counts matching entities rather than returned ones, so the
/// count is taken before the window and after the filter.
fn window(admitted: Vec<Value>, pagination: Pagination) -> FinishedPage {
    let total_count = match pagination.count {
        CountPolicy::Emit => Some(i64::try_from(admitted.len()).unwrap_or(i64::MAX)),
        CountPolicy::Omit => None,
    };
    let rows = admitted.into_iter().skip(pagination.offset as usize).take(pagination.limit as usize).collect();
    FinishedPage { rows, total_count }
}

/// The error a store earns by reporting an applied window alongside a residual.
fn incoherent(unapplied: &ResidualFilter) -> AndromedaError {
    let kinds: Vec<String> = unapplied.kinds().iter().map(ToString::to_string).collect();
    AndromedaError::internal(format!(
        "store applied the result window but left {} unapplied; the page would be short and every later page misaligned",
        kinds.join(", ")
    ))
}

impl EntityQueryService {
    /// Finish an identifier-only outcome (clause 5.2.39 entity maps).
    ///
    /// A predicate cannot be evaluated against an identifier, so an outstanding
    /// one costs a payload read for every identifier the store returned. That is
    /// the honest price of an entity-map query against a store that could not
    /// push the predicate down.
    pub(super) async fn finish_ids(&self, outcome: QueryOutcome<String>, pagination: Pagination) -> Result<Vec<String>, AndromedaError> {
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
                    return Ok(rows.into_iter().skip(pagination.offset as usize).take(pagination.limit as usize).collect());
                }
                let payloads = self.repo.get_batch(&rows).await?;
                Ok(rows
                    .into_iter()
                    .filter(|id| payloads.get(id).is_some_and(|payload| unapplied.matches(payload)))
                    .skip(pagination.offset as usize)
                    .take(pagination.limit as usize)
                    .collect())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::service::entity::{
        pagination_outcome::PaginationOutcome,
        query::residual::finish_page,
        query_outcome::QueryOutcome,
        residual_filter::ResidualFilter,
    };
    use andromeda_ngsi_ld_query::{
        entity_selection::EntitySelection,
        filter::parser,
        pagination::{CountPolicy, Pagination},
    };
    use serde_json::{Value, json};

    const TEMPERATURE: &str = "https://example.com/temperature";

    fn entity(temperature: f64) -> Value {
        json!([{
            "@id": format!("urn:ngsi-ld:Building:{temperature}"),
            TEMPERATURE: [{"https://uri.etsi.org/ngsi-ld/hasValue": [{"@value": temperature}]}]
        }])
    }

    /// Five entities at 10, 20, 30, 40 and 50 degrees.
    fn five() -> Vec<Value> {
        [10.0, 20.0, 30.0, 40.0, 50.0].into_iter().map(entity).collect()
    }

    /// A residual admitting the three entities above 25 degrees.
    fn above_25() -> ResidualFilter {
        ResidualFilter::over(
            EntitySelection::builder()
                .q_expr(Some(parser::parse("\"https://example.com/temperature\">25").expect("valid q filter")))
                .build(),
        )
    }

    fn page(limit: u32, offset: u32, count: CountPolicy) -> Pagination {
        Pagination { limit, offset, count }
    }

    #[test]
    fn an_applied_page_with_no_residual_is_returned_unchanged() {
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: ResidualFilter::default(),
            pagination: PaginationOutcome::Applied { total: Some(5) },
        };
        let finished = finish_page(outcome, page(20, 0, CountPolicy::Emit)).expect("coherent outcome");
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
        let error = finish_page(outcome, page(20, 0, CountPolicy::Emit)).expect_err("must refuse");
        assert!(error.to_string().contains('q'), "the message names the outstanding predicate: {error}");
    }

    #[test]
    fn a_deferred_page_is_windowed_after_the_residual_runs() {
        // Clause 5.5.6: the window is over matching entities, and only three of
        // the five match once the residual has run.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, page(2, 1, CountPolicy::Omit)).expect("coherent outcome");
        pretty_assertions::assert_eq!(finished.rows, vec![entity(40.0), entity(50.0)]);
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
        let finished = finish_page(outcome, page(2, 0, CountPolicy::Emit)).expect("coherent outcome");
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
        let finished = finish_page(outcome, page(2, 0, CountPolicy::Omit)).expect("coherent outcome");
        pretty_assertions::assert_eq!(finished.total_count, None);
    }

    #[test]
    fn a_deferred_offset_past_the_surviving_set_returns_no_rows_and_still_counts() {
        // Clause 5.5.6: an offset beyond the result set is an empty page, not
        // an error, and clause 6.3.13's count is unaffected by it.
        let outcome = QueryOutcome {
            rows: five(),
            unapplied: above_25(),
            pagination: PaginationOutcome::Deferred,
        };
        let finished = finish_page(outcome, page(20, 10, CountPolicy::Emit)).expect("coherent outcome");
        assert!(finished.rows.is_empty());
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
        let finished = finish_page(outcome, page(2, 0, CountPolicy::Emit)).expect("coherent outcome");
        pretty_assertions::assert_eq!(finished.rows, vec![entity(10.0), entity(20.0)]);
        pretty_assertions::assert_eq!(finished.total_count, Some(5));
    }
}
