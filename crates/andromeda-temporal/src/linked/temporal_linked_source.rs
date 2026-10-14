//! Where the temporal operations read their Linked Entities from, behind the
//! `temporal_linked_retrieval` extension (clause 4.5.23 applied over a window).
//!
//! The traversal, its order and its bounds belong to the Linked Entity engine;
//! this source answers its questions over the request's window. A temporal
//! answer is a slice of history, and every Linked Entity is rendered over the
//! same window the roots were: joining a January root to a February link would
//! produce a response no client could interpret. Only the window carries over:
//! the request's `attrs` and `datasetId` select the history of the roots, and
//! the list in force narrows every entity of the payload when it is written.
//!
//! This broker's history answers for the entities it recorded, and the
//! remainder of the distributed system through [`RemoteTemporalLinkedSource`]
//! for an `objectType`-annotated identifier it did not (clause 4.5.23.1). The
//! temporal join is an extension, so a holder is asked for more than the
//! entity itself only when it advertised that extension.

use crate::{
    error::TemporalError,
    port::read_scope::ScopeReach,
    read::{compacted_history::compact_history, rendered_entity::HistoryPresence},
    remote_linked::{RemoteTemporalLinkedEntities, TemporalLinkedWindow},
    service::TemporalService,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::{
    candidate::LinkedCandidate,
    representation::LinkedRepresentation,
    source::{LevelRead, LinkedSource, RemoteReach, RemoteRead},
};
use andromeda_ngsi_ld_compaction::entity_compactor::EntityCompactor;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::temporal::temporal_body::TemporalBody;
use andromeda_ngsi_ld_query::temporal::compiled::CompiledTemporalQuery;
use andromeda_ngsi_ld_render::temporal::temporal_evolutions::TemporalEvolutions;
use andromeda_ngsi_ld_representation::linked_resolution_failure::{LinkedResolutionFailure, LinkedResolutionFailureKind};
use indexmap::IndexMap;
use std::{future::Future, num::NonZeroU32};

/// The Linked Entities of one temporal read: the branch they are read from,
/// the window they are rendered over, and the context they are compacted
/// under.
#[derive(Clone, Copy)]
pub(crate) struct TemporalLinkedSource<'s> {
    /// The service holding the history and the remote half.
    service: &'s TemporalService,
    /// The branch the history is read from.
    target: &'s ContextTarget,
    /// The window as this broker's own history answers it.
    compiled: &'s CompiledTemporalQuery,
    /// The same window as a remote holder is asked for it.
    window: &'s TemporalLinkedWindow,
    /// The context every Linked Entity is compacted under.
    context: &'s ContextSpec,
}

impl<'s> TemporalLinkedSource<'s> {
    /// The source reading `target` through `service` over `window`, compiled
    /// as `compiled`, compacting under `context`.
    pub(crate) const fn new(
        service: &'s TemporalService,
        target: &'s ContextTarget,
        compiled: &'s CompiledTemporalQuery,
        window: &'s TemporalLinkedWindow,
        context: &'s ContextSpec,
    ) -> TemporalLinkedSource<'s> {
        TemporalLinkedSource {
            service,
            target,
            compiled,
            window,
            context,
        }
    }

    /// Read the candidates this broker recorded history for, in candidate
    /// order, with the edges each names.
    ///
    /// The temporal catalogue is the authority for which entities have a
    /// Temporal Evolution here and for their types, so an entity whose current
    /// state a clause 5.6.6 deletion has removed is still joined while its
    /// history stands. A candidate the catalogue does not know is not a
    /// temporal entity here; one the window selects no history for, or whose
    /// history cannot be read, is left out as well: clause 4.5.23 leaves such a
    /// Relationship a plain URI rather than failing the retrieval.
    async fn read_locally(&self, candidates: &[LinkedCandidate]) -> Result<LevelRead<TemporalBody>, TemporalError> {
        let ids: Vec<Identifier> = candidates.iter().map(|candidate| candidate.id.clone()).collect(); // clone: the store reads an owned id slice
        let scope = self.service.reader.read_scope(self.target, ScopeReach::Every).await?;
        let catalogued = self.service.reader.entity_types(&scope, &ids).await?;
        let compactor = self.service.engine.compactor(self.context);
        let compactor = EntityCompactor::new(&compactor);
        let mut nodes = IndexMap::with_capacity(candidates.len());
        let mut edges = Vec::new();
        for candidate in candidates {
            let Some(types) = catalogued.get(&candidate.id) else {
                continue;
            };
            let rendered = self
                .service
                .render_one_temporal_entity(&scope, &candidate.id, types, self.compiled, &compactor)
                .await;
            let body = match rendered {
                Ok(rendered) if rendered.presence == HistoryPresence::Present => compact_history(&compactor, rendered.history).await,
                Ok(_) => continue,
                Err(error) => Err(error),
            };
            match body {
                Ok(body) => {
                    TemporalEvolutions::edges(&body, &mut edges);
                    nodes.insert(candidate.id.clone(), body); // clone: the level keys its nodes by owned identifiers
                }
                Err(error) => {
                    tracing::debug!("Failed to render the temporal evolution of linked entity {}: {}", candidate.id, error);
                }
            }
        }
        Ok(LevelRead { nodes, edges })
    }

    /// Ask the remainder of the system for one candidate over the window.
    ///
    /// A source that cannot be consulted degrades the join rather than the
    /// request (clause 4.5.23): the candidate stays unresolved, and the read
    /// says what is missing.
    async fn fetch(&self, candidate: &LinkedCandidate, depth: NonZeroU32, held: &[Identifier]) -> RemoteRead<TemporalBody> {
        let Some(remote) = self.service.remote_temporal_linked.as_ref() else {
            return RemoteRead {
                nodes: Vec::new(),
                edges: Vec::new(),
                failures: Vec::new(),
            };
        };
        match remote.fetch_linked_temporal(&candidate.id, depth, held, self.window, self.context).await {
            Ok(RemoteTemporalLinkedEntities { entities, failures }) => {
                for failure in &failures {
                    tracing::warn!("Linked entity {} was not fully resolved: {} ({})", failure.id, failure.detail, failure.kind);
                }
                let compactor = self.service.engine.compactor(self.context);
                let compactor = EntityCompactor::new(&compactor);
                let mut nodes = Vec::with_capacity(entities.len());
                let mut edges = Vec::new();
                for history in entities {
                    match compact_history(&compactor, history).await {
                        Ok(body) => {
                            TemporalEvolutions::edges(&body, &mut edges);
                            nodes.push(body);
                        }
                        Err(error) => {
                            tracing::debug!(
                                "Failed to compact the temporal evolution of linked entity {} outside this broker: {}",
                                candidate.id,
                                error
                            );
                        }
                    }
                }
                RemoteRead { nodes, edges, failures }
            }
            Err(error) => {
                tracing::warn!(
                    "Failed to resolve the temporal evolution of linked entity {} outside this broker: {}",
                    candidate.id,
                    error
                );
                RemoteRead {
                    nodes: Vec::new(),
                    edges: Vec::new(),
                    failures: vec![LinkedResolutionFailure {
                        id: candidate.id.clone(), // clone: the failure outlives this level's candidates
                        kind: LinkedResolutionFailureKind::Unreachable,
                        detail: error.to_string(),
                    }],
                }
            }
        }
    }

    /// How far the holders of `candidate` resolve on their own.
    async fn reach(&self, candidate: &LinkedCandidate) -> RemoteReach {
        match self.service.remote_temporal_linked.as_ref() {
            Some(remote) => remote.remote_reach(&candidate.id, self.context).await,
            None => RemoteReach::SingleEntity,
        }
    }
}

impl LinkedSource for TemporalLinkedSource<'_> {
    type Representation = TemporalEvolutions;
    type Error = TemporalError;

    fn read_level(&self, candidates: &[LinkedCandidate]) -> impl Future<Output = Result<LevelRead<TemporalBody>, TemporalError>> + Send {
        self.read_locally(candidates)
    }

    fn fetch_remote(&self, candidate: &LinkedCandidate, depth: NonZeroU32, held: &[Identifier]) -> impl Future<Output = RemoteRead<TemporalBody>> + Send {
        self.fetch(candidate, depth, held)
    }

    fn federates(&self) -> bool {
        self.target.permits_federation() && self.service.remote_temporal_linked.is_some()
    }

    fn remote_reach(&self, candidate: &LinkedCandidate) -> impl Future<Output = RemoteReach> + Send {
        self.reach(candidate)
    }
}
