//! What a listing of the identifiers a temporal query matches holds them to
//! (clauses 5.5.14, 5.7.4.4 and 5.14.5.4).

use crate::requested_identifiers::RequestedIdentifiers;
use andromeda_ngsi_ld_query::{candidate_filters::CandidateFilters, temporal::contributed_entities::ContributedEntities};

/// What a listing of the identifiers a temporal query matches holds them to.
#[derive(Debug)]
pub struct CandidateListing {
    /// Whether the identifiers the request specified narrow the listing.
    pub identifiers: RequestedIdentifiers,
    /// Whether the filters split entities leave to the gathered Entity hold
    /// the listed entities.
    pub filters: CandidateFilters,
    /// The entities a Context Source contributed a part of to a split query.
    /// The value, geo and scope filters do not hold them in the listing: their
    /// gathered parts decide those filters (clause 5.7.4.4).
    pub contributed: ContributedEntities,
}
