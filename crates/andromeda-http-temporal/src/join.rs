//! Applying a clause 4.5.23 join to the compact roots of a temporal read,
//! behind the `temporal_linked_retrieval` extension.
//!
//! All three temporal read endpoints -- `GET /temporal/entities/{id}`,
//! `GET /temporal/entities` and `POST /temporal/entityOperations/query` -- run
//! the Linked Entity engine the same way once they hold their roots: the
//! temporal service traverses over the request's window and the engine plans
//! the payload, which the temporal writer spells (`crate::temporal_answer`).
//! None of the handlers decides anything about Linked Entities itself.

use crate::history_read::TemporalHistoryRead;
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_http_response::linked_resolution_warning::with_linked_resolution_warnings;
use andromeda_linked_retrieval::plan::JoinPlan;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_body::TemporalBody;
use andromeda_ngsi_ld_render::temporal::temporal_evolutions::TemporalEvolutions;
use andromeda_temporal::{error::TemporalError, linked::TemporalLinkedJoin};

/// What the traversal of a clause 4.5.23 join reached, or nothing when the
/// request asked for no join that retrieves.
pub(crate) struct ResolvedJoin(Option<TemporalLinkedJoin>);

impl ResolvedJoin {
    /// How the Linked Entities the traversal reached join the payload, or
    /// `None` when none was retrieved.
    pub(crate) fn plan(&self) -> Option<JoinPlan<'_, TemporalEvolutions>> {
        self.0.as_ref().map(TemporalLinkedJoin::plan)
    }

    /// The identifiers of the Linked Entities the payload carries, in
    /// first-encounter order. Clause 5.5.14 asks an `EntityMap` created
    /// alongside a joined query to include them.
    pub(crate) fn linked_ids(&self) -> Vec<Identifier> {
        self.plan()
            .map(|plan| plan.linked_ids().into_iter().cloned().collect()) // clone: the Entity Map outlives the plan
            .unwrap_or_default()
    }
}

/// Resolve the request's join over `roots`, and the response warnings with one
/// appended per Linked Entity clause 6.3.17 leaves unresolved.
///
/// `read` is the history the roots came out of: the traversal reaches the
/// Linked Entities through the same service, inside the same target, over the
/// same temporal window and under the same JSON-LD context. A root holds every
/// instance of its attributes whatever representation the response is written
/// in, so the edges it names are read off it directly: a join over
/// `temporalValues` finds what it would find over the normalized form.
///
/// The returned warnings are the ones passed in plus one per Linked Entity the
/// traversal could not reach: clause 6.3.17 makes the `NGSILD-Warning` header
/// the only way a client learns the body is incomplete, since clause 4.5.23
/// leaves an unresolvable Relationship as a plain URI rather than failing.
///
/// # Errors
///
/// Returns [`TemporalError`] when the window or the projection cannot be
/// compiled, or the base entity store cannot be read.
pub(crate) async fn apply_temporal_join(
    read: &TemporalHistoryRead<'_>,
    roots: &[TemporalBody],
    warnings: Vec<NgsiLdWarning>,
) -> Result<(ResolvedJoin, Vec<NgsiLdWarning>), TemporalError> {
    let joined = read
        .service
        .join_linked_temporal(read.target, roots, read.params, read.context, read.attrs_admission)
        .await?;
    let warnings = match &joined {
        Some(joined) => with_linked_resolution_warnings(warnings, joined.failures()),
        None => warnings,
    };
    Ok((ResolvedJoin(joined), warnings))
}
