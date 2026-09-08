//! Resolving the entities a temporal result set links to (clause 4.5.23
//! `join`, applied to the temporal API).
//!
//! Clause 4.5.23 is written for entities, but nothing in it is specific to the
//! non-temporal representation: a Relationship names an entity, and `join` asks
//! for that entity to accompany the response. The temporal API answers the same
//! question about the same graph, so the traversal is the same walk — with one
//! addition that changes everything about the result. A temporal answer is a
//! slice of history, and every Linked Entity is rendered over *the same window*
//! the roots were rendered over. Joining a January root to a February link
//! would produce a response no client could interpret.
//!
//! The walk is breadth-first: the entities the roots name are resolved first,
//! then the entities those name, one level at a time. Each level is resolved in
//! two halves — this broker's own history answers for what it recorded, and the
//! remainder of the distributed system answers for the `objectType`-annotated
//! ids it did not (clause 4.5.23.1).
//!
//! There is no depth cap. Clause 4.5.23 admits any positive `joinLevel`, and
//! termination is guaranteed by the visited set instead: every candidate is
//! resolved at most once, so the work is bounded by the number of reachable
//! entities rather than by the client's number.
//!
//! # A deliberate extension beyond ETSI GS CIM 009 v1.9.1
//!
//! The specification does not define `join` for the Temporal API. Clauses
//! 5.7.3.4 and 5.7.4.4 list the parameters the temporal retrieval and query
//! operations accept, `join`, `joinLevel` and `containedBy` are not among them,
//! and a strictly conformant broker would answer a request carrying them with
//! `BadRequestData`. Andromeda deliberately accepts them instead and applies
//! the clause 4.5.23 semantics unchanged, because the question `join` answers —
//! "give me the entities this result points at" — is exactly as meaningful over
//! a slice of history as over the current state, and answering it here saves a
//! client one round trip per Linked Entity.
//!
//! This is an extension, not an interpretation: nothing in clauses 5.7.3.4 or
//! 5.7.4.4 can be read as permitting it. A client that needs strict conformance
//! must simply not send the parameters, in which case nothing here runs.
//!
//! Clauses 5.7.3.4 and 5.7.4.4 go further than omitting the parameters: they
//! refuse Linked Entity retrieval on the temporal operations outright, raising
//! an error where projection attributes or filter conditions indicate one. This
//! extension does not inherit that refusal — it accepts the references and
//! bounds them instead, by the rule clause 5.7.2.4 sets on the entity bindings.
//! A `pick`/`omit` nesting and a clause 4.9 `rel{…}` hop in `q` each require a
//! join that retrieves (`flat` or `inline`; `@none` is the parameter's default,
//! not a mode) and may reach no further than `joinLevel` walks.

use crate::{
    compilation::compile_projection,
    error::TemporalError,
    linked::candidates::{RemoteEligibility, TemporalLinkedCandidate, collect_candidates, identifier_of},
    read::rendered_entity::HistoryPresence,
    remote_linked::{RemoteTemporalLinkedSource, TemporalLinkedWindow},
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, term_expansion::ContextualExpander};
use andromeda_ngsi_ld_lexicon::{entity_type_name::EntityTypeName, uri::Uri};
use andromeda_ngsi_ld_model::expanded::entity_member::extract_types;
use andromeda_ngsi_ld_query::{projection::compiled::Projection, temporal::compiled::CompiledTemporalQuery};
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use indexmap::{IndexMap, IndexSet};
use serde_json::Value;
use std::{
    collections::{HashMap, HashSet},
    num::NonZeroU32,
};

/// Everything a temporal join traversal produced: the temporal
/// representations it resolved, and the Linked Entities it could not.
///
/// The two travel together because a caller that renders the first has to
/// account for the second — an HTTP binding turns each failure into a clause
/// 6.3.17 response warning — and neither of them is an error that should fail
/// the operation.
#[derive(Debug, Default)]
pub struct ResolvedLinkedTemporalEntities {
    /// The linked temporal representations, keyed by identifier in the
    /// traversal's append order.
    pub entities: IndexMap<Uri, Value>,
    /// Every Linked Entity the traversal left unresolved, in the order the
    /// attempts were made.
    pub failures: Vec<LinkedResolutionFailure>,
}

/// One level of the traversal, as resolved.
struct ResolvedTemporalLevel {
    /// The level's representations in append order: the locally held ones in
    /// candidate order, then the ones fetched from elsewhere.
    entities: IndexMap<Uri, Value>,
    /// What the level could not resolve.
    failures: Vec<LinkedResolutionFailure>,
    /// What the locally rendered entities of this level named, reported before
    /// any format projection erased it.
    candidates: Vec<TemporalLinkedCandidate>,
}

impl TemporalService {
    /// Resolve the temporal evolution of the entities `rendered` links to, up
    /// to `join_level` hops away.
    ///
    /// Every resolved entity is rendered over the same window `params` selects,
    /// so the roots and their links describe one consistent slice of history.
    /// Only the window carries over: the request's `attrs` and `datasetId`
    /// narrow the roots, not the entities they link to.
    ///
    /// `seed` is what the roots named, read off their normalized form before
    /// any format projection erased it. `rendered` is walked as well, which is
    /// what covers roots that arrived already rendered from a context source.
    ///
    /// The result is keyed by identifier in breadth-first encounter order and
    /// never contains a root, an id listed in `contained_by` (clause 4.5.23
    /// excludes the entities the client already holds), or the same id twice.
    /// An id that nothing backs is simply absent — an unresolvable Relationship
    /// leaves the plain URI in place rather than failing the request — and,
    /// when something was reachable but unhelpful, it is named in
    /// [`ResolvedLinkedTemporalEntities::failures`] so the caller can say so.
    ///
    /// # Ordering
    ///
    /// Clause 4.5.23.3 requires the linked entities to accompany the roots but
    /// does not fix their order. Andromeda fixes it, so that the same request
    /// against the same data always renders the same body: the map's
    /// first-encounter order *is* the append order, and within one level this
    /// broker's own answers come first, in candidate order, followed by the
    /// answers from the remainder of the system (clause 4.5.23.1).
    ///
    /// # Errors
    ///
    /// Returns [`TemporalError`] when the query cannot be compiled or the base
    /// entity store cannot be read. A single Linked Entity whose history cannot
    /// be rendered, and a remote source that cannot answer, are reported rather
    /// than propagated.
    pub async fn resolve_linked_temporal(
        &self,
        target: &ContextTarget,
        rendered: &[Value],
        seed: &[TemporalLinkedCandidate],
        params: &TemporalQueryParameters,
        join_level: NonZeroU32,
        contained_by: &HashSet<Uri>,
        context: &ContextSpec,
    ) -> Result<ResolvedLinkedTemporalEntities, TemporalError> {
        let window = TemporalLinkedWindow::of_request(params);
        // A Linked Entity is read over the window alone. The request's `attrs`
        // and `datasetId` narrow what the *roots* show; carrying them into the
        // traversal would hide a linked entity whose own attributes the client
        // never named, and would contradict what the forwarded half already
        // does — a remote broker answers a linked retrieval in full.
        let compiled = self.compile_temporal_query(&window.to_parameters(), context).await?;

        let mut resolved = ResolvedLinkedTemporalEntities::default();
        // The roots and the client-held ids are excluded from the first level
        // and from every level after it, so a cycle back into them terminates.
        // The guard keeps insertion order because it doubles as the list of
        // already-held ids a non-local fetch hands to the remote broker.
        let mut visited: IndexSet<Uri> = contained_by.iter().cloned().collect(); // clone: the guard owns the ids it excludes
        visited.extend(rendered.iter().filter_map(identifier_of));

        // The seed was read off the roots' normalized form, before any format
        // projection erased the `object` members; walking the rendered roots as
        // well is what covers the ones that arrived already rendered from a
        // context source.
        let mut candidates = merge_candidates(seed, collect_candidates(rendered.iter(), &visited), &visited);
        // Kept for the nested-projection pass below: `candidates` itself is
        // reassigned to each later level as the loop walks deeper, but a
        // nested projection only ever applies to what the *roots* themselves
        // named, read here before any format projection could erase the
        // attribute association it carries.
        let root_candidates = candidates.clone(); // clone: outlives the loop that reassigns `candidates`
        let mut remaining = Some(join_level);

        while let Some(depth) = remaining
            && !candidates.is_empty()
        {
            visited.extend(candidates.iter().map(|candidate| candidate.id.clone())); // clone: the guard outlives this level's candidates
            let level = self.resolve_level(target, &candidates, depth, &visited, &compiled, &window, context).await?;
            // Everything the level produced — including the sub-graph a remote
            // broker returned around a candidate — is now held, so no later
            // level asks for it again.
            visited.extend(level.entities.keys().cloned()); // clone: the guard outlives this level's entities
            remaining = NonZeroU32::new(depth.get() - 1);
            // The next level's candidates are read before this level is folded
            // into the result, so the result's insertion order stays exactly
            // the order the levels were resolved in.
            candidates = match remaining {
                // The locally rendered entities reported their own links before
                // projection; the ones a context source returned are walked as
                // rendered, which is all there is to walk them with.
                Some(_) => merge_candidates(&level.candidates, collect_candidates(expandable(&candidates, &level), &visited), &visited),
                None => Vec::new(),
            };
            resolved.entities.extend(level.entities);
            resolved.failures.extend(level.failures);
        }

        // Clause 4.21: a root's `pick=rel{a}` / `omit=rel{a}` narrows whatever
        // `rel` links to. The relationship itself was kept or dropped on the
        // root before this traversal even ran (`TemporalEntityRenderer`); what
        // is left is narrowing the resolved targets themselves.
        //
        // This reads `root_candidates` rather than `rendered`: `rendered` may
        // have been projected to `temporalValues` or `aggregatedValues`
        // (clause 4.5.8, 4.5.19), which collapse an attribute's instances into
        // a value array and erase the `object` / `objectList` members a
        // per-attribute walk would need. `root_candidates` was read off the
        // roots' normalized form before that erasure -- the same reason the
        // traversal itself takes a `seed` rather than re-deriving candidates
        // from `rendered` alone -- and each candidate already carries which
        // attribute named it.
        let root_projection = self.compile_root_projection(params, context).await?;
        if !root_projection.pick.is_empty() || !root_projection.omit.is_empty() {
            let index = self.temporal_attribute_iri_index(&root_candidates, &resolved.entities, context).await;
            projection::apply_nested_projections(&root_candidates, &root_projection, &index, &mut resolved.entities);
        }

        Ok(resolved)
    }

    /// Expand the root request's `pick` / `omit` into a [`Projection`],
    /// independent of the window-scoped query compiled for the Linked
    /// Entities themselves.
    async fn compile_root_projection(&self, params: &TemporalQueryParameters, context: &ContextSpec) -> Result<Projection, TemporalError> {
        let prepared = self.engine.prepare_term_context(context);
        let expander = ContextualExpander::new(&self.engine, &prepared);
        compile_projection(params, &expander).await
    }

    /// Resolve one level from this broker's own history and, for what it does
    /// not hold, from the remainder of the system.
    ///
    /// `depth` is the number of levels still to resolve, this one included.
    async fn resolve_level(
        &self,
        target: &ContextTarget,
        candidates: &[TemporalLinkedCandidate],
        depth: NonZeroU32,
        visited: &IndexSet<Uri>,
        compiled: &CompiledTemporalQuery,
        window: &TemporalLinkedWindow,
        context: &ContextSpec,
    ) -> Result<ResolvedTemporalLevel, TemporalError> {
        let (entities, named) = self.resolve_level_locally(target, candidates, compiled, context).await?;
        let mut level = ResolvedTemporalLevel {
            entities,
            failures: Vec::new(),
            candidates: named,
        };
        let Some(source) = target.permits_federation().then_some(self.remote_temporal_linked.as_ref()).flatten() else {
            return Ok(level);
        };
        for candidate in candidates {
            if candidate.eligibility == RemoteEligibility::LocalOnly || level.entities.contains_key(&candidate.id) {
                continue;
            }
            fetch_remote_candidate(source.as_ref(), candidate, depth, visited, window, context, &mut level).await;
        }
        Ok(level)
    }

    /// Render one level's candidates from this broker's own history, in
    /// candidate order.
    ///
    /// A candidate the window selects no history for, or whose representation
    /// cannot be rendered, is dropped from the level: clause 4.5.23 leaves such
    /// a Relationship as a plain URI rather than failing the whole retrieval.
    async fn resolve_level_locally(
        &self,
        target: &ContextTarget,
        candidates: &[TemporalLinkedCandidate],
        compiled: &CompiledTemporalQuery,
        context: &ContextSpec,
    ) -> Result<(IndexMap<Uri, Value>, Vec<TemporalLinkedCandidate>), TemporalError> {
        let ids: Vec<Uri> = candidates.iter().map(|candidate| candidate.id.clone()).collect(); // clone: the store reads an owned id slice
        let payloads = self
            .entity_reader
            .get_batch(target, &ids)
            .await
            .map_err(|error| TemporalError::persistence(error.to_string()))?;
        let types: HashMap<&Uri, Vec<EntityTypeName>> = payloads.iter().map(|(id, payload)| (id, extract_types(payload))).collect();

        let mut level = IndexMap::with_capacity(candidates.len());
        let mut named: Vec<TemporalLinkedCandidate> = Vec::new();
        for candidate in candidates {
            // An id the base store does not know is not a temporal entity here,
            // so its history is not this broker's to render.
            let Some(entity_types) = types.get(&candidate.id) else {
                continue;
            };
            match self.render_one_temporal_entity(target, &candidate.id, entity_types, compiled, context).await {
                Ok(rendered) => {
                    if rendered.presence == HistoryPresence::Present {
                        level.insert(candidate.id.clone(), rendered.value); // clone: the result map owns its keys
                        named.extend(rendered.candidates);
                    }
                }
                Err(error) => {
                    tracing::debug!("Failed to render the temporal evolution of linked entity {}: {}", candidate.id, error);
                }
            }
        }
        Ok((level, named))
    }
}

/// Fold two candidate lists into one, keeping first-encounter order, dropping
/// anything already walked, and letting one annotated sighting make an
/// identifier remotely eligible (clause 4.5.23.1).
fn merge_candidates(first: &[TemporalLinkedCandidate], second: Vec<TemporalLinkedCandidate>, visited: &IndexSet<Uri>) -> Vec<TemporalLinkedCandidate> {
    let mut seen: IndexSet<Uri> = IndexSet::new();
    let mut merged: Vec<TemporalLinkedCandidate> = Vec::new();
    for candidate in first.iter().cloned().chain(second) {
        // clone: the merged list owns its candidates, and `first` is borrowed
        if visited.contains(&candidate.id) {
            continue;
        }
        if let Some(index) = seen.get_index_of(&candidate.id) {
            if candidate.eligibility == RemoteEligibility::Eligible
                && let Some(existing) = merged.get_mut(index)
            {
                existing.eligibility = RemoteEligibility::Eligible;
            }
        } else {
            seen.insert(candidate.id.clone()); // clone: the duplicate guard owns the ids it records
            merged.push(candidate);
        }
    }
    merged
}

/// The level's entities whose own Relationships the next level walks.
///
/// A candidate this level resolved — locally, or by being fetched from a remote
/// broker — is a next-level root like any other. The extra entities that came
/// back around a fetched candidate are not: they are the remote broker's
/// already-resolved sub-tree, walked with the depth budget it was given, and
/// walking them again here would spend levels twice and reach further than the
/// client's `joinLevel` allows.
fn expandable<'a>(candidates: &'a [TemporalLinkedCandidate], level: &'a ResolvedTemporalLevel) -> impl Iterator<Item = &'a Value> {
    candidates.iter().filter_map(|candidate| level.entities.get(&candidate.id))
}

/// Ask the remainder of the system for one candidate and fold what it answers
/// into the level it belongs to.
///
/// A source that cannot answer degrades the join rather than the request
/// (clause 4.5.23): the candidate stays unresolved, leaving the Relationship
/// that named it a plain URI, and the level carries a description of what is
/// missing back to the caller.
async fn fetch_remote_candidate(
    source: &dyn RemoteTemporalLinkedSource,
    candidate: &TemporalLinkedCandidate,
    depth: NonZeroU32,
    visited: &IndexSet<Uri>,
    window: &TemporalLinkedWindow,
    context: &ContextSpec,
    level: &mut ResolvedTemporalLevel,
) {
    // The candidate itself is already in the guard; the remainder of the system
    // is told about everything except the entity it is being asked for.
    let held: Vec<Uri> = visited.iter().filter(|id| **id != candidate.id).cloned().collect(); // clone: the port takes the ids as an owned slice
    match source.fetch_linked_temporal(&candidate.id, depth, &held, window, context).await {
        Ok(fetched) => {
            for entity in fetched.entities {
                let Some(id) = identifier_of(&entity) else {
                    continue;
                };
                // The remote broker resolved the deeper levels in the same
                // round trip. Whatever it returns that the requester already
                // holds is dropped: clause 4.5.23 `containedBy` keeps those out
                // of the result.
                if id != candidate.id && visited.contains(&id) {
                    continue;
                }
                level.entities.entry(id).or_insert(entity);
            }
            for failure in &fetched.failures {
                tracing::warn!("Linked entity {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
            }
            level.failures.extend(fetched.failures);
        }
        Err(error) => {
            tracing::warn!(
                "Failed to resolve the temporal evolution of linked entity {} outside this broker: {}",
                candidate.id,
                error
            );
            level.failures.push(LinkedResolutionFailure {
                id: candidate.id.clone(), // clone: the failure outlives this level's candidates
                kind: LinkedResolutionFailureKind::Unreachable,
                detail: error.to_string(),
            });
        }
    }
}

/// Which Linked Entities a rendered temporal representation names.
pub mod candidates;

/// Applying the root request's clause 4.21 nested projection to resolved
/// Linked Entities.
mod projection;

#[cfg(test)]
mod tests;
