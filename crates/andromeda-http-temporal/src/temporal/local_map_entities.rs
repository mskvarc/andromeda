//! Where the entities a temporal `EntityMap` attributes to this broker come
//! from (clauses 5.2.39 and 5.5.14).
//!
//! The two temporal reads that run under a map — Query Temporal Evolution of
//! Entities and Retrieve Temporal Evolution of an Entity — differ only in how
//! the histories this broker holds itself are listed, so that difference alone
//! is what this module owns; the recording built on it is the same for both and
//! belongs to the map policy.

use crate::{error::TemporalResultExt, history_read::TemporalHistoryRead};
use andromeda_entity_map::recording::{LocalMapEntities, MapListing, PageFiltering};
use andromeda_http_response::problem_response::ProblemResponse;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::page_window::PageWindow;
use andromeda_temporal::requested_identifiers::RequestedIdentifiers;
use axum::http::Uri;

/// Where the histories a temporal map attributes to this broker come from.
///
/// Listing them is not free for a query — it costs a pass over the temporal
/// catalogue — so the source is described here and read only when the recording
/// asks for it.
pub(crate) enum LocalHistoryEntities<'a> {
    /// The single history a retrieval held locally, if it held one.
    Retrieved(Option<&'a Identifier>),
    /// Every history a query matched locally, which is the whole result and not
    /// merely the page (clause 5.5.14), together with the Linked Entities the
    /// clause 4.5.23 join reached, which belong to the same unitary request.
    Queried {
        /// The read the identifiers are listed through.
        read: &'a TemporalHistoryRead<'a>,
        /// The Linked Entities the join resolved.
        linked: &'a [Identifier],
        /// The request URI a failed listing names.
        instance: &'a Uri,
    },
}

impl LocalHistoryEntities<'_> {
    /// Whether preparing this read's page applied the request's filters to the
    /// entities of the map it ran under (clause 4.3.6.7).
    ///
    /// A query applies its query, geoquery, scope query and attribute filters
    /// to an entity before it joins a result page; a retrieval applies none of
    /// them, reading the map only to learn which context sources hold the one
    /// history it asked for.
    pub(crate) const fn page_filtering(&self) -> PageFiltering {
        match self {
            LocalHistoryEntities::Retrieved(_) => PageFiltering::NotApplied,
            LocalHistoryEntities::Queried { .. } => PageFiltering::Applied,
        }
    }
}

impl LocalMapEntities for LocalHistoryEntities<'_> {
    type Error = ProblemResponse;

    async fn identifiers(&self, listing: MapListing) -> Result<Vec<Identifier>, ProblemResponse> {
        match *self {
            LocalHistoryEntities::Retrieved(entity_id) => Ok(entity_id.cloned().into_iter().collect()), // clone: the map owns its identifiers
            LocalHistoryEntities::Queried { read, linked, instance } => {
                let requested = match listing {
                    MapListing::Minted => RequestedIdentifiers::Narrow,
                    // Clause 4.3.6.7 rechecks the filters of the request the map
                    // was created for, and clause 5.5.14 lets a follow-up name
                    // its own identifiers: a request asking about one history of
                    // the map has not checked the others, so it must not shed
                    // them.
                    MapListing::Rechecked => RequestedIdentifiers::Ignore,
                };
                // Clause 5.5.14: the map fixes every entity the request matched,
                // whatever page -- offset or cursor -- the query itself answers
                // with, so the identifier pass reads the whole result.
                let mut identifiers = read
                    .service
                    .query_ids(read.target, read.params, &PageWindow::unbounded(), read.context, requested)
                    .await
                    .with_instance(instance)?;
                identifiers.extend(linked.iter().cloned()); // clone: the map owns its identifiers
                identifiers.sort_unstable_by(|left, right| left.as_str().cmp(right.as_str()));
                identifiers.dedup();
                Ok(identifiers)
            }
        }
    }
}
