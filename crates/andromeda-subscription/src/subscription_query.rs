//! A Subscription's `q`, compiled once and prepared once.

use andromeda_ngsi_ld_query::filter::compiled_filter::CompiledFilter;
use andromeda_ngsi_ld_query_evaluation::q::{prepared_filter::PreparedFilter, term_memo::TermInterner};

/// The `q` of a Subscription: the compiled filter every execution reads, and
/// its preparation for the matching hot path, which runs it over every change
/// its entity type sees.
#[derive(Debug, Clone)]
pub struct SubscriptionQuery {
    /// The expanded expression and its readings.
    compiled: CompiledFilter,
    /// The reading in normal form, prepared to run.
    prepared: PreparedFilter,
}

impl SubscriptionQuery {
    /// The query over `compiled`, prepared.
    #[must_use]
    pub fn new(compiled: CompiledFilter) -> SubscriptionQuery {
        SubscriptionQuery {
            prepared: PreparedFilter::new(compiled.reading()),
            compiled,
        }
    }

    /// Numbers the terms of the query with `interner`, so a matching pass
    /// decides a term the Subscriptions of a tenant share once.
    pub fn number_terms(&mut self, interner: &TermInterner) {
        self.prepared.number_terms(interner);
    }

    /// The compiled filter.
    #[must_use]
    pub const fn compiled(&self) -> &CompiledFilter {
        &self.compiled
    }

    /// The filter prepared to run.
    #[must_use]
    pub const fn prepared(&self) -> &PreparedFilter {
        &self.prepared
    }
}
