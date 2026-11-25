//! Deciding a split temporal capture's values filter, geoquery and Scope query
//! over the gathered parts of every entity a Context Source answered a pass
//! with (clauses 5.7.4.4 and 5.16.1.4).
//!
//! Under `splitEntities` the query forwarded to the sources carries none of
//! those filters, and clause 5.7.4.4 applies them "after the Entity
//! information from different Context Sources and local information, if there
//! is any, has been aggregated". A pass of the capture is a request of its
//! own, as Query Temporal Evolution of Entities answers it: every entity its
//! sources answered for its window, or for the Scope in force when the window
//! opens, is contributed. Every local read of the pass exempts the contributed
//! entities from the filters, and the gathered parts of each decide them,
//! judged by the same engine Query Temporal Evolution of Entities judges them
//! by. An entity no source answered for is decided by the local read, as an
//! unsplit query decides it.

use crate::{
    error::SnapshotError,
    executor::{
        SnapshotExecutor,
        source_pass::SourcePass,
        source_shortfall::SourceShortfall,
        temporal_capture::TemporalCapture,
        temporal_failure::{TemporalStep, temporal_failure},
    },
};
use andromeda_federation::split_entities::SplitEntities;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{snapshot::ExecutionResultDetails, temporal::temporal_history::TemporalHistory};
use andromeda_ngsi_ld_query::temporal::contributed_entities::{ContributedEntities, ContributedRendering};
use andromeda_ngsi_ld_request::query::temporal::{TemporalQueryParameters, admitted::AdmittedTemporalRequest};
use andromeda_temporal::{
    query_reading::QueryReading,
    split_settlement::{AnsweredParts, SplitSettlement},
};
use tracing::warn;

/// Where a captured temporal query's values filter, geoquery and Scope query
/// are decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum FilterGathering {
    /// Each holder decides them over its own part: the query is not split,
    /// stays local, or carries none of them.
    EachHolder,
    /// The gathered parts of each entity a source contributed to decide them
    /// (clause 5.7.4.4).
    GatheredParts,
}

impl FilterGathering {
    /// Where `params`, asked under `split`, decides its filters.
    pub(super) fn of(params: &TemporalQueryParameters, split: SplitEntities) -> FilterGathering {
        match split {
            SplitEntities::Split if !params.is_local() && params.carries_split_filters() => FilterGathering::GatheredParts,
            SplitEntities::Split | SplitEntities::Complete => FilterGathering::EachHolder,
        }
    }
}

/// What one pass of a capture decided of its split filters: the entities its
/// sources contributed to, and which of them their gathered parts keep. A
/// pass whose filters each holder decides contributes none and keeps every
/// entity.
#[derive(Debug, Default)]
pub(super) struct SplitVerdict {
    /// The entities a source answered the pass with, for its window or for
    /// the Scope in force when it opens.
    contributed: ContributedEntities,
    /// Which contributed entities the filters keep.
    settlement: SplitSettlement,
}

impl SplitVerdict {
    /// The verdict keeping, of `contributed`, the entities `settlement`
    /// retained.
    pub(super) const fn new(contributed: ContributedEntities, settlement: SplitSettlement) -> SplitVerdict {
        SplitVerdict { contributed, settlement }
    }

    /// The entities a source contributed to, which every local read of the
    /// pass exempts from the filters.
    pub(super) const fn contributed(&self) -> &ContributedEntities {
        &self.contributed
    }

    /// Whether the entity `entity_id` stays in the pass's answer.
    pub(super) fn keeps(&self, entity_id: &Identifier) -> bool {
        self.settlement.keeps(&self.contributed, entity_id)
    }

    /// The contributed entities among `entity_ids`, which a read of those
    /// entities alone exempts.
    pub(super) fn contributed_among(&self, entity_ids: &[Identifier]) -> ContributedEntities {
        ContributedEntities::new(
            entity_ids.iter().filter(|entity_id| self.contributed.contains(entity_id)).cloned(),
            ContributedRendering::Local,
        )
    }
}

/// How judging the split filters of a pass ended.
pub(super) enum PassJudgement {
    /// The pass's verdict.
    Judged(SplitVerdict),
    /// The query's failure: a probe exceeded its budget or failed in this
    /// broker's own store, or the gathered parts could not be judged.
    Ended(ExecutionResultDetails),
}

impl SnapshotExecutor {
    /// Judge the split filters of the pass the sources answered `params`
    /// with, over the gathered parts of every entity they contributed to; the
    /// failed sources a probe met are recorded in `shortfall`.
    ///
    /// The probes and the judgement are boxed: inline, the capture loop's
    /// future would carry them across every await.
    ///
    /// # Errors
    ///
    /// Returns a transient failure reading the parts held here met, on which
    /// the preparation restarts.
    pub(super) async fn judge_pass(
        &self,
        capture: &TemporalCapture<'_>,
        params: &AdmittedTemporalRequest,
        pass: &SourcePass,
        shortfall: &mut SourceShortfall,
    ) -> Result<PassJudgement, SnapshotError> {
        if capture.gathering == FilterGathering::EachHolder {
            return Ok(PassJudgement::Judged(SplitVerdict::default()));
        }
        let probed = match Box::pin(self.in_effect_answers(capture, params)).await {
            Ok(probed) => probed,
            Err(failure) => return Ok(PassJudgement::Ended(failure)),
        };
        shortfall.record(probed.iter().flat_map(|answer| answer.warnings.iter()));
        let probed_parts = || probed.iter().flat_map(|answer| answer.entities.iter());
        let contributed = ContributedEntities::new(
            pass.identifiers().into_iter().chain(probed_parts().filter_map(TemporalHistory::id)).cloned(),
            ContributedRendering::Local,
        );
        if contributed.is_empty() {
            return Ok(PassJudgement::Judged(SplitVerdict::default()));
        }
        let answered = AnsweredParts {
            in_window: pass.answered_documents().collect(),
            in_effect: probed_parts().filter_map(TemporalHistory::instances).collect(),
        };
        let temporal = &self.sources.temporal;
        let reading = QueryReading {
            context: capture.context,
            language_targeting: temporal.language_targeting(),
        };
        let settled = Box::pin(async {
            let local = temporal
                .read_local_in_effect(&self.sources.live_target, params, reading, contributed.as_slice())
                .await?;
            temporal
                .settle_split_filters(&self.sources.live_target, params, reading, &answered, &local)
                .await
        })
        .await;
        match settled {
            Ok(settlement) => {
                for unread in &settlement.incomplete {
                    warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, entity_id = %unread.id, kind = %unread.kind, detail = %unread.detail, "Snapshot temporal query decided its split filters without a Relationship target that could not be read");
                }
                Ok(PassJudgement::Judged(SplitVerdict::new(contributed, settlement)))
            }
            Err(error) => {
                warn!(snapshot_id = %capture.snapshot_id, query_index = capture.index, error = %error, "Snapshot temporal query failed");
                temporal_failure(error, TemporalStep::Query).map(PassJudgement::Ended)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::executor::split_verdict::{FilterGathering, SplitVerdict};
    use andromeda_federation::split_entities::SplitEntities;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_query::{
        scope::scope_query::ScopeQuery,
        temporal::contributed_entities::{ContributedEntities, ContributedRendering},
    };
    use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
    use andromeda_temporal::split_settlement::SplitSettlement;
    use pretty_assertions::assert_eq;

    fn filtered() -> TemporalQueryParameters {
        TemporalQueryParameters {
            q: Some("speed>50".parse().expect("a valid q")),
            ..TemporalQueryParameters::default()
        }
    }

    fn id(number: u32) -> Identifier {
        format!("urn:ngsi-ld:Vehicle:{number}").parse().expect("a valid identifier")
    }

    /// Clause 5.7.4.4 leaves the filters to the aggregated Entities only
    /// under `splitEntities`, for a distributed query, and only the values
    /// filter, the geoquery and the Scope query among them.
    #[test]
    fn only_a_distributed_split_query_carrying_a_filter_gathers_the_parts() {
        let scoped = TemporalQueryParameters {
            scope_q: Some("/Madrid".parse::<ScopeQuery>().expect("a valid scopeQ")),
            ..TemporalQueryParameters::default()
        };
        let local = TemporalQueryParameters {
            local: Some(true),
            ..filtered()
        };
        assert_eq!(FilterGathering::of(&filtered(), SplitEntities::Split), FilterGathering::GatheredParts);
        assert_eq!(FilterGathering::of(&scoped, SplitEntities::Split), FilterGathering::GatheredParts);
        assert_eq!(FilterGathering::of(&filtered(), SplitEntities::Complete), FilterGathering::EachHolder);
        assert_eq!(FilterGathering::of(&local, SplitEntities::Split), FilterGathering::EachHolder);
        assert_eq!(
            FilterGathering::of(&TemporalQueryParameters::default(), SplitEntities::Split),
            FilterGathering::EachHolder
        );
    }

    /// A verdict drops only the contributed entities their gathered parts
    /// failed, and a read of some entities alone exempts the contributed
    /// ones among them.
    #[test]
    fn a_verdict_drops_only_the_contributed_entities_the_filters_failed() {
        let verdict = SplitVerdict::new(
            ContributedEntities::new([id(1), id(2)], ContributedRendering::Local),
            SplitSettlement {
                retained: [id(2)].into_iter().collect(),
                incomplete: Vec::new(),
                narrowing: None,
            },
        );
        assert!(!verdict.keeps(&id(1)));
        assert!(verdict.keeps(&id(2)));
        assert!(verdict.keeps(&id(3)));
        assert_eq!(
            verdict.contributed_among(&[id(2), id(3)]),
            ContributedEntities::new([id(2)], ContributedRendering::Local)
        );
    }

    /// A pass whose holders decided their own filters keeps every entity and
    /// exempts none.
    #[test]
    fn a_verdict_of_holders_deciding_their_own_filters_keeps_every_entity() {
        let verdict = SplitVerdict::default();
        assert!(verdict.keeps(&id(1)));
        assert!(verdict.contributed().is_empty());
    }
}
