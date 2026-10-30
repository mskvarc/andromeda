//! The entities this broker holds itself for one page of an entity query.
//!
//! The local store answers for each matching entity on its own, so an entity
//! whose stored rows no longer read back is taken out of the page and named,
//! for the response to report, while the others are answered.

use crate::entity::{
    EntityResultExt,
    ProblemResponse,
    query_page::{LocalReach, QueryPage},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_entity::{
    port::stored_entry::StoredEntry,
    query::{EntityQueryService, entity_versions::held_version},
};
use andromeda_jsonld::user_context_admission::AdmittedContext;
use andromeda_ngsi_ld_expansion::received_state::ReceivedState;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use axum::http::Uri as HttpUri;

/// The entities this broker holds itself for a query page.
pub(crate) struct LocalVersions {
    /// The versions a merge builds on, in the order the store read them.
    pub(crate) versions: Vec<ReceivedState>,
    /// The matching entities whose stored rows no longer read back, left out
    /// of the page and reported by a warning.
    pub(crate) unreadable: Vec<Identifier>,
    /// The count the store took.
    pub(crate) total: Option<i64>,
    /// Whether the store cut the page, and whether its collection continues.
    pub(crate) reach: LocalReach,
}

/// The page of entities this broker holds itself, read as the versions a
/// merge builds on, with the count the store took.
///
/// A page the store cut is settled before an entity that does not read back
/// is taken out of it, so the entity still counts towards whether the
/// collection continues past the page.
///
/// # Errors
///
/// The classified failure of the local query.
pub(crate) async fn local_versions(
    service: &EntityQueryService,
    target: &ContextTarget,
    query: &EntityQueryParameters,
    page: &QueryPage,
    context: &AdmittedContext,
    targeting: LanguageTargeting,
    instance: &HttpUri,
) -> Result<LocalVersions, ProblemResponse> {
    let (local_window, local_count) = page.local_read();
    let (rows, total) = service
        .query_states(target, query, &local_window, local_count, context, targeting)
        .await
        .with_entity_instance(instance)?;
    let (rows, reach) = page.settle_local(rows);
    let mut versions = Vec::with_capacity(rows.len());
    let mut unreadable = Vec::new();
    for row in rows {
        match row {
            StoredEntry::Read(row) => versions.push(held_version(row)),
            StoredEntry::Unreadable(id) => unreadable.push(id),
        }
    }
    Ok(LocalVersions {
        versions,
        unreadable,
        total,
        reach,
    })
}
