//! The most one forwarded query may gather from its context sources: distinct
//! entities, and the rows the sources send to name them.
//!
//! Clause 4.3.6.7 asks the broker to "initially make a broad request to each
//! registered Context Source whose registration is matching the request" so it
//! can paginate the merged result consistently, and clause 5.5.9.1 makes the
//! whole of a source's answer the pages it links with `next`. Gathering all of
//! it is therefore the conformant behaviour, and it has to be bounded: clause
//! 4.12 recommends "defining a hard limit to the size of generated response
//! payload body while paginating", naming `TooManyResults` as the refusal, and
//! the error type's definition leaves the threshold "up to each
//! implementation".
//!
//! The merged result holds each entity once however many sources answer it
//! (clause 5.7.2.4), so the entity limit counts distinct identifiers across
//! every source of the request, local answers included, which the caller checks
//! against the same limit. An entity only an auxiliary source answers is an
//! entity of the result (clauses 5.7.2.4 and 4.3.6.2), so an auxiliary source
//! is charged as every other; an identifier another holder answers too counts
//! once. The transfer
//! itself grows with every source answering the same entities again, so the
//! rows the sources send, duplicates included, are bounded by a second limit.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use dashmap::DashSet;
use std::{
    num::NonZeroU32,
    sync::atomic::{AtomicU64, Ordering},
};
use thiserror::Error;

/// The limits of one forwarded query's [`ResultBudget`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResultLimits {
    /// The most distinct entities the query may gather.
    pub distinct_entities: NonZeroU32,
    /// The most elements the sources may send, duplicates included.
    pub source_rows: NonZeroU32,
}

/// What one forwarded query may still gather, shared by every source it
/// contacts concurrently.
#[derive(Debug)]
pub struct ResultBudget {
    /// The limits the budget holds the query to.
    limits: ResultLimits,
    /// Every element every source sent, duplicates and discarded ones included.
    rows: AtomicU64,
    /// The identifiers every source answered.
    distinct: DashSet<Identifier>,
    /// The elements naming no entity: none can be matched against another, so
    /// each is an entity of its own.
    anonymous: AtomicU64,
}

/// A forwarded query gathered more than its budget allows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum ResultBudgetExceeded {
    /// The distinct entities exceed their limit.
    #[error("the query matches more than the {maximum} distinct entities this broker gathers for one request (federation.forwarding.max_query_entities)")]
    DistinctEntities {
        /// The limit that was exceeded.
        maximum: NonZeroU32,
    },
    /// The elements the sources sent, duplicates included, exceed their limit.
    #[error(
        "the context sources answered more than the {maximum} entities this broker fetches for one request, duplicates included (federation.forwarding.max_query_source_rows)"
    )]
    SourceRows {
        /// The limit that was exceeded.
        maximum: NonZeroU32,
    },
}

impl ResultBudget {
    /// A budget holding a query to `limits`.
    #[must_use]
    pub fn new(limits: ResultLimits) -> ResultBudget {
        ResultBudget {
            limits,
            rows: AtomicU64::new(0),
            distinct: DashSet::new(),
            anonymous: AtomicU64::new(0),
        }
    }

    /// The limits the budget holds the query to.
    #[must_use]
    pub const fn limits(&self) -> ResultLimits {
        self.limits
    }

    /// Charge one page a source answered: `sent` elements as the source counts
    /// them, of which `ids` are the ones the walk kept, each the entity it
    /// names when it names one.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded::SourceRows`] once every source of the
    /// request together sent more elements than allowed, and
    /// [`ResultBudgetExceeded::DistinctEntities`] once they answered more
    /// distinct entities than allowed.
    pub fn charge_page(&self, sent: usize, ids: impl Iterator<Item = Option<Identifier>>) -> Result<(), ResultBudgetExceeded> {
        let sent = u64::try_from(sent).unwrap_or(u64::MAX);
        // Relaxed is enough: each counter is checked on its own, and every
        // charge sees every earlier one through the atomic addition itself.
        let rows = self.rows.fetch_add(sent, Ordering::Relaxed).saturating_add(sent);
        if rows > u64::from(self.limits.source_rows.get()) {
            return Err(ResultBudgetExceeded::SourceRows {
                maximum: self.limits.source_rows,
            });
        }
        let anonymous = ids.fold(0_u64, |anonymous, id| match id {
            Some(id) => {
                self.distinct.insert(id);
                anonymous
            }
            None => anonymous.saturating_add(1),
        });
        let anonymous = self.anonymous.fetch_add(anonymous, Ordering::Relaxed).saturating_add(anonymous);
        let distinct = u64::try_from(self.distinct.len()).unwrap_or(u64::MAX);
        self.admit_collection(distinct.saturating_add(anonymous))
    }

    /// Whether everything charged so far, by every source of the request,
    /// still fits both limits.
    ///
    /// A walk asks before it requests another page: once any walk has crossed
    /// a limit the query is refused whatever the others gather, so the pages
    /// still to come would be fetched for an answer already decided.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded::SourceRows`] when the rows charged
    /// exceed their limit, and [`ResultBudgetExceeded::DistinctEntities`] when
    /// the entities charged do.
    pub fn check(&self) -> Result<(), ResultBudgetExceeded> {
        if self.rows.load(Ordering::Relaxed) > u64::from(self.limits.source_rows.get()) {
            return Err(ResultBudgetExceeded::SourceRows {
                maximum: self.limits.source_rows,
            });
        }
        let distinct = u64::try_from(self.distinct.len()).unwrap_or(u64::MAX);
        self.admit_collection(distinct.saturating_add(self.anonymous.load(Ordering::Relaxed)))
    }

    /// Whether a collection of `entities` distinct entities fits the entity
    /// limit.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded::DistinctEntities`] when it does not.
    pub fn admit_collection(&self, entities: u64) -> Result<(), ResultBudgetExceeded> {
        if entities > u64::from(self.limits.distinct_entities.get()) {
            Err(ResultBudgetExceeded::DistinctEntities {
                maximum: self.limits.distinct_entities,
            })
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::result_budget::{ResultBudget, ResultBudgetExceeded, ResultLimits};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use std::{num::NonZeroU32, ops::Range};

    fn positive(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("a positive limit")
    }

    fn budget(distinct_entities: u32, source_rows: u32) -> ResultBudget {
        ResultBudget::new(ResultLimits {
            distinct_entities: positive(distinct_entities),
            source_rows: positive(source_rows),
        })
    }

    fn ids(range: Range<u32>) -> Vec<Option<Identifier>> {
        range
            .map(|number| Some(format!("urn:ngsi-ld:Vehicle:{number:04}").parse().expect("a valid identifier")))
            .collect()
    }

    fn charge(budget: &ResultBudget, ids: Vec<Option<Identifier>>) -> Result<(), ResultBudgetExceeded> {
        budget.charge_page(ids.len(), ids.into_iter())
    }

    #[test]
    fn three_sources_answering_the_same_ids_charge_them_once() {
        let budget = budget(1_000, 3_000);
        for _ in 0..3 {
            pretty_assertions::assert_eq!(charge(&budget, ids(0..400)), Ok(()));
        }
    }

    #[test]
    fn distinct_ids_past_the_limit_are_distinct_entities_exceeded() {
        let budget = budget(1_000, 3_000);
        pretty_assertions::assert_eq!(
            charge(&budget, ids(0..1_001)),
            Err(ResultBudgetExceeded::DistinctEntities { maximum: positive(1_000) })
        );
    }

    #[test]
    fn the_entity_limit_is_reached_exactly_without_refusal() {
        let budget = budget(10, 100);
        pretty_assertions::assert_eq!(charge(&budget, ids(0..4)), Ok(()));
        pretty_assertions::assert_eq!(charge(&budget, ids(4..10)), Ok(()));
        pretty_assertions::assert_eq!(
            charge(&budget, ids(10..11)),
            Err(ResultBudgetExceeded::DistinctEntities { maximum: positive(10) })
        );
    }

    #[test]
    fn duplicated_rows_past_the_row_ceiling_are_source_rows_exceeded() {
        let budget = budget(1_000, 1_000);
        pretty_assertions::assert_eq!(charge(&budget, ids(0..400)), Ok(()));
        pretty_assertions::assert_eq!(charge(&budget, ids(0..400)), Ok(()));
        pretty_assertions::assert_eq!(charge(&budget, ids(0..400)), Err(ResultBudgetExceeded::SourceRows { maximum: positive(1_000) }));
    }

    /// Rows count what the source sent, so elements the walk discarded still
    /// count against the row ceiling.
    #[test]
    fn discarded_elements_count_as_rows() {
        let budget = budget(10, 5);
        pretty_assertions::assert_eq!(
            budget.charge_page(6, ids(0..1).into_iter()),
            Err(ResultBudgetExceeded::SourceRows { maximum: positive(5) })
        );
    }

    /// Clause 5.7.2.4: an entity only an auxiliary source answers is part of
    /// the result, so its identifier is charged as an entity.
    #[test]
    fn an_auxiliary_only_id_is_charged_as_an_entity() {
        let budget = budget(2, 10);
        pretty_assertions::assert_eq!(charge(&budget, ids(0..2)), Ok(()));
        pretty_assertions::assert_eq!(charge(&budget, ids(5..6)), Err(ResultBudgetExceeded::DistinctEntities { maximum: positive(2) }));
    }

    #[test]
    fn elements_naming_no_entity_each_count_as_one() {
        let budget = budget(2, 10);
        pretty_assertions::assert_eq!(charge(&budget, vec![None, None]), Ok(()));
        pretty_assertions::assert_eq!(
            charge(&budget, vec![None]),
            Err(ResultBudgetExceeded::DistinctEntities { maximum: positive(2) })
        );
    }

    #[test]
    fn a_budget_checks_clean_until_a_charge_crosses_a_limit_and_then_stays_refused() {
        let budget = budget(3, 100);
        pretty_assertions::assert_eq!(budget.check(), Ok(()));
        pretty_assertions::assert_eq!(charge(&budget, ids(0..3)), Ok(()));
        pretty_assertions::assert_eq!(budget.check(), Ok(()));
        assert!(charge(&budget, ids(3..4)).is_err());
        pretty_assertions::assert_eq!(budget.check(), Err(ResultBudgetExceeded::DistinctEntities { maximum: positive(3) }));
    }

    #[test]
    fn a_budget_past_its_row_ceiling_checks_as_source_rows_exceeded() {
        let budget = budget(100, 2);
        assert!(charge(&budget, ids(0..3)).is_err());
        pretty_assertions::assert_eq!(budget.check(), Err(ResultBudgetExceeded::SourceRows { maximum: positive(2) }));
    }

    #[test]
    fn a_collection_is_admitted_up_to_the_entity_limit() {
        let budget = budget(3, 30);
        pretty_assertions::assert_eq!(budget.admit_collection(3), Ok(()));
        pretty_assertions::assert_eq!(budget.admit_collection(4), Err(ResultBudgetExceeded::DistinctEntities { maximum: positive(3) }));
    }

    #[test]
    fn each_refusal_names_its_bound_and_setting() {
        let distinct = ResultBudgetExceeded::DistinctEntities { maximum: positive(5) }.to_string();
        let rows = ResultBudgetExceeded::SourceRows { maximum: positive(15) }.to_string();
        assert!(
            distinct.contains("5 distinct entities") && distinct.contains("federation.forwarding.max_query_entities"),
            "{distinct}"
        );
        assert!(
            rows.contains("15 entities") && rows.contains("duplicates included") && rows.contains("federation.forwarding.max_query_source_rows"),
            "{rows}"
        );
    }
}
