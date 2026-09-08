//! Candidate-funnel accounting for subscription matching.

use crate::{
    names::{LABEL_STAGE, SUBSCRIPTION_CANDIDATES},
    sample_value::SampleValue,
};
use metrics::histogram;
use strum::{EnumIter, IntoStaticStr};

/// One narrowing stage of the subscription candidate funnel.
///
/// Each stage records how many subscriptions were still in play once it ran,
/// so `evaluated` far below `active` shows the routing index is doing its job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, EnumIter, IntoStaticStr)]
#[strum(serialize_all = "kebab-case")]
pub enum FunnelStage {
    /// Active subscriptions held by the cache when the event arrived.
    Active,
    /// Subscriptions whose entity selectors could match the event.
    Routed,
    /// Subscriptions whose watched attributes or trigger set admit the event.
    Watched,
    /// Subscriptions handed to the authoritative matcher.
    Evaluated,
    /// Subscriptions the authoritative matcher accepted.
    Matched,
}

/// The per-event counts of one funnel pass.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CandidateFunnel {
    /// Subscriptions active when the event arrived.
    pub active: usize,
    /// Subscriptions surviving selector routing.
    pub routed: usize,
    /// Subscriptions surviving attribute and trigger narrowing.
    pub watched: usize,
    /// Subscriptions evaluated by the authoritative matcher.
    pub evaluated: usize,
    /// Subscriptions that matched.
    pub matched: usize,
}

impl CandidateFunnel {
    /// Record every stage count on the candidates histogram.
    pub fn record(self) {
        for (stage, count) in [
            (FunnelStage::Active, self.active),
            (FunnelStage::Routed, self.routed),
            (FunnelStage::Watched, self.watched),
            (FunnelStage::Evaluated, self.evaluated),
            (FunnelStage::Matched, self.matched),
        ] {
            let stage: &'static str = stage.into();
            histogram!(SUBSCRIPTION_CANDIDATES, LABEL_STAGE => stage).record(SampleValue::from(count));
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        error::TelemetryError,
        names::SUBSCRIPTION_CANDIDATES,
        recorder::TelemetryRecorder,
        subscription::{CandidateFunnel, FunnelStage},
    };
    use strum::IntoEnumIterator;

    #[test]
    fn every_stage_is_rendered_under_its_kebab_case_label() -> Result<(), TelemetryError> {
        let recorder = TelemetryRecorder::detached()?;
        recorder.record_locally(|| {
            CandidateFunnel {
                active: 10,
                routed: 4,
                watched: 3,
                evaluated: 3,
                matched: 1,
            }
            .record();
        });
        let rendered = recorder.render();
        for stage in FunnelStage::iter() {
            let label: &'static str = stage.into();
            assert!(
                rendered.contains(&format!("{SUBSCRIPTION_CANDIDATES}_count{{stage=\"{label}\"}} 1")),
                "{rendered}"
            );
        }
        Ok(())
    }
}
