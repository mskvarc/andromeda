//! How far a clause 4.5.23 join walks, and whether it walks at all.
//!
//! Every endpoint that joins — entity retrieval and query, temporal retrieval
//! and query — reads the same three parameters (`join`, `joinLevel`,
//! `containedBy`) and has to answer the same two questions before it can start:
//! is this a join that retrieves anything, and what bounds does it run under.
//! Both answers are properties of the request, so they are derived once here
//! rather than at each binding.

use andromeda_ngsi_ld_lexicon::uri::Uri;
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::{collections::HashSet, num::NonZeroU32};

/// The two clause 4.5.23 join modes that actually retrieve linked entities.
///
/// `@none` is the absence of a join rather than a third way of rendering one,
/// and an absent `join` parameter means the same thing (clause 4.5.23). Naming
/// the retrieving modes apart is what lets every rendering match stay total
/// without an arm that can never be taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievingJoin {
    /// Roots and linked entities side by side in one collection.
    Flat,
    /// Linked entities nested under the Relationships that name them.
    Inline,
}

impl RetrievingJoin {
    /// The retrieving join the request asked for, or `None` when it asked for
    /// no join at all.
    pub const fn from_mode(mode: Option<JoinMode>) -> Option<RetrievingJoin> {
        match mode {
            Some(JoinMode::Flat) => Some(RetrievingJoin::Flat),
            Some(JoinMode::Inline) => Some(RetrievingJoin::Inline),
            Some(JoinMode::None) | None => None,
        }
    }

    /// The spec-level mode this join is.
    pub const fn mode(self) -> JoinMode {
        match self {
            RetrievingJoin::Flat => JoinMode::Flat,
            RetrievingJoin::Inline => JoinMode::Inline,
        }
    }
}

/// How far a traversal goes and which identifiers it must not retrieve.
///
/// Clause 4.5.23: `joinLevel` defaults to one hop when `join` is present
/// without it, and `containedBy` names the entities the client already holds,
/// which the traversal must not retrieve again.
pub fn traversal_bounds(join_level: Option<NonZeroU32>, contained_by: Option<&[Uri]>) -> (NonZeroU32, HashSet<Uri>) {
    let held = contained_by.map(|ids| ids.iter().cloned().collect()).unwrap_or_default(); // clone: the guard owns the ids it excludes
    (traversal_depth(join_level), held)
}

/// How many hops a traversal walks, defaulting the absent `joinLevel` to one
/// (clause 4.5.23).
///
/// Held apart from [`traversal_bounds`] because validation needs the depth
/// before any traversal exists, and must not pay for the `containedBy` set to
/// get it -- nor restate the default and risk validating against a different
/// depth than the traversal runs at.
pub const fn traversal_depth(join_level: Option<NonZeroU32>) -> NonZeroU32 {
    match join_level {
        Some(level) => level,
        None => NonZeroU32::MIN,
    }
}

#[cfg(test)]
mod tests {
    use crate::join_traversal::{RetrievingJoin, traversal_bounds};
    use andromeda_ngsi_ld_lexicon::uri::Uri;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    /// Clause 4.5.23: `join=@none` is the absence of a join. It must not
    /// retrieve a linked entity and must not render one, so it never becomes a
    /// [`RetrievingJoin`] and the traversal is never reached.
    #[test]
    fn the_none_join_mode_retrieves_nothing() {
        assert_eq!(RetrievingJoin::from_mode(Some(JoinMode::None)), None);
    }

    /// A request that carries no `join` parameter at all behaves exactly as
    /// `@none` (clause 4.5.23: `join` defaults to `@none`).
    #[test]
    fn an_absent_join_parameter_retrieves_nothing() {
        assert_eq!(RetrievingJoin::from_mode(None), None);
    }

    /// The two retrieving modes map across unchanged, in both directions.
    #[test]
    fn the_retrieving_modes_round_trip_through_the_spec_level_mode() {
        for (mode, expected) in [(JoinMode::Flat, RetrievingJoin::Flat), (JoinMode::Inline, RetrievingJoin::Inline)] {
            let retrieving = RetrievingJoin::from_mode(Some(mode)).expect("a retrieving join");
            assert_eq!(retrieving, expected);
            assert_eq!(retrieving.mode(), mode);
        }
    }

    /// Clause 4.5.23: a `join` without a `joinLevel` walks exactly one hop.
    #[test]
    fn an_absent_join_level_defaults_to_one_hop() {
        let (level, held) = traversal_bounds(None, None);

        assert_eq!(level, NonZeroU32::MIN);
        assert!(held.is_empty());
    }

    /// A stated `joinLevel` is taken as it stands, and `containedBy` becomes
    /// the set of identifiers the traversal must not retrieve.
    #[test]
    fn a_stated_depth_and_contained_by_become_the_traversal_bounds() {
        let held_ids = ["urn:ngsi-ld:T:1".parse::<Uri>().expect("valid URI")];

        let (level, held) = traversal_bounds(NonZeroU32::new(3), Some(&held_ids));

        assert_eq!(level.get(), 3);
        assert!(held.contains(&held_ids[0]));
    }
}
