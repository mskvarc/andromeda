//! The order a visit probes its indexes in.
//!
//! An index unprobed for a day comes first, so every probeable index, the
//! `GiST` included, is measured at least daily. The rest follow by their B-tree
//! estimate, highest first, then the indexes without one. The estimate only
//! orders: no index is dropped for a low estimate.

use crate::{
    btree_estimate::BtreeEstimate,
    candidate::index_target::IndexTarget,
    inspection::IndexSize,
    policy::PolicyProfile,
    state::target_record::IndexProbeState,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use std::cmp::Reverse;

/// How long an index may go unprobed before it is probed ahead of every
/// estimate.
const PROBE_STALENESS_FLOOR: ChronoDuration = ChronoDuration::hours(24);

/// Where an index stands in a visit's probe order, most urgent first.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum ProbeRank {
    /// Never probed, or last probed at least [`PROBE_STALENESS_FLOOR`] ago.
    Stale,
    /// Probed within the floor, with its estimated free percent, the highest
    /// first.
    Estimated(Reverse<u8>),
    /// Probed within the floor and without an estimate: the `GiST`, or a B-tree
    /// the catalogs do not fully describe.
    Unestimated,
}

/// Every probeable index large enough and out of cooldown, in probe order.
///
/// Within a rank the least recently probed index comes first, then the
/// larger one.
pub(crate) fn probe_order(
    sizes: &[IndexSize],
    states: &[IndexProbeState],
    estimates: &[BtreeEstimate],
    profile: PolicyProfile,
    now: DateTime<Utc>,
) -> Vec<IndexSize> {
    let state = |target: IndexTarget| states.iter().find(|state| state.target == target);
    let mut ranked = sizes
        .iter()
        .copied()
        .filter(|size| size.bytes >= profile.minimum_relation_bytes && IndexTarget::PROBEABLE.contains(&size.target))
        .filter(|size| {
            state(size.target)
                .and_then(|state| state.cooldown_until)
                .is_none_or(|cooldown_until| cooldown_until <= now)
        })
        .map(|size| {
            let last_sampled_at = state(size.target).and_then(|state| state.last_sampled_at);
            let rank = if last_sampled_at.is_none_or(|sampled_at| sampled_at <= now - PROBE_STALENESS_FLOOR) {
                ProbeRank::Stale
            } else {
                estimates
                    .iter()
                    .find(|estimate| estimate.target == size.target)
                    .map_or(ProbeRank::Unestimated, |estimate| ProbeRank::Estimated(Reverse(estimate.free_percent)))
            };
            ((rank, last_sampled_at, Reverse(size.bytes)), size)
        })
        .collect::<Vec<_>>();
    ranked.sort_by_key(|(order, _)| *order);
    ranked.into_iter().map(|(_, size)| size).collect()
}

#[cfg(test)]
mod tests {
    use crate::{
        branch::Branch,
        btree_estimate::BtreeEstimate,
        candidate::index_target::IndexTarget,
        controller::probe_selection::probe_order,
        inspection::IndexSize,
        policy::PolicyProfile,
        state::target_record::IndexProbeState,
    };
    use andromeda_configuration::schema::database::maintenance::MaintenanceProfile;
    use chrono::{Duration, Utc};
    use pretty_assertions::assert_eq;

    fn targets(order: &[IndexSize]) -> Vec<IndexTarget> {
        order.iter().map(|size| size.target).collect()
    }

    #[test]
    fn no_gin_is_ever_selected_for_direct_probe() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let sizes = [
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueStringTrgm,
                bytes: profile.minimum_relation_bytes * 10,
            },
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaSubEqKeys,
                bytes: profile.minimum_relation_bytes * 20,
            },
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueString,
                bytes: profile.minimum_relation_bytes,
            },
        ];
        assert_eq!(targets(&probe_order(&sizes, &[], &[], profile, Utc::now())), [IndexTarget::IdxCaValueString]);
    }

    #[test]
    fn detailed_probes_rotate_and_skip_successful_cooldowns() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let sizes = [
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueNumber,
                bytes: profile.minimum_relation_bytes * 2,
            },
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueString,
                bytes: profile.minimum_relation_bytes,
            },
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueBoolean,
                bytes: profile.minimum_relation_bytes,
            },
        ];
        let states = [
            IndexProbeState {
                target: IndexTarget::IdxCaValueNumber,
                last_sampled_at: None,
                cooldown_until: Some(now + Duration::hours(1)),
            },
            IndexProbeState {
                target: IndexTarget::IdxCaValueString,
                last_sampled_at: Some(now - Duration::minutes(1)),
                cooldown_until: None,
            },
            IndexProbeState {
                target: IndexTarget::IdxCaValueBoolean,
                last_sampled_at: Some(now - Duration::minutes(2)),
                cooldown_until: None,
            },
        ];
        assert_eq!(
            targets(&probe_order(&sizes, &states, &[], profile, now)),
            [IndexTarget::IdxCaValueBoolean, IndexTarget::IdxCaValueString]
        );
    }

    #[test]
    fn probe_candidates_put_the_staleness_floor_first_then_the_estimate() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let size = |target| IndexSize {
            branch: Branch::Live,
            target,
            bytes: profile.minimum_relation_bytes,
        };
        let probed = |target, hours| IndexProbeState {
            target,
            last_sampled_at: Some(now - Duration::hours(hours)),
            cooldown_until: None,
        };
        let sizes = [
            size(IndexTarget::IdxCaValueString),
            size(IndexTarget::IdxCaValueGeometry),
            size(IndexTarget::IdxCaValueNumber),
        ];
        let states = [
            probed(IndexTarget::IdxCaValueString, 1),
            probed(IndexTarget::IdxCaValueGeometry, 25),
            probed(IndexTarget::IdxCaValueNumber, 1),
        ];
        let estimates = [
            BtreeEstimate {
                target: IndexTarget::IdxCaValueString,
                free_percent: 20,
            },
            BtreeEstimate {
                target: IndexTarget::IdxCaValueNumber,
                free_percent: 60,
            },
        ];
        assert_eq!(
            targets(&probe_order(&sizes, &states, &estimates, profile, now)),
            [IndexTarget::IdxCaValueGeometry, IndexTarget::IdxCaValueNumber, IndexTarget::IdxCaValueString]
        );
    }

    #[test]
    fn an_index_without_an_estimate_follows_the_estimated_ones_and_is_never_dropped() {
        let profile = PolicyProfile::from_profile(MaintenanceProfile::Responsive);
        let now = Utc::now();
        let sizes = [
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueGeometry,
                bytes: profile.minimum_relation_bytes,
            },
            IndexSize {
                branch: Branch::Live,
                target: IndexTarget::IdxCaValueUri,
                bytes: profile.minimum_relation_bytes,
            },
        ];
        let states = [IndexTarget::IdxCaValueGeometry, IndexTarget::IdxCaValueUri].map(|target| IndexProbeState {
            target,
            last_sampled_at: Some(now - Duration::hours(2)),
            cooldown_until: None,
        });
        let estimates = [BtreeEstimate {
            target: IndexTarget::IdxCaValueUri,
            free_percent: 0,
        }];
        assert_eq!(
            targets(&probe_order(&sizes, &states, &estimates, profile, now)),
            [IndexTarget::IdxCaValueUri, IndexTarget::IdxCaValueGeometry]
        );
    }
}
