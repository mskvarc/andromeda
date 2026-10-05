//! Whether a clause 4.5.23 join retrieves anything, and the bounds it walks
//! under.
//!
//! Every operation that joins reads the same three parameters (`join`,
//! `joinLevel`, `containedBy`) and has to answer the same two questions before
//! it can start: is this a join that retrieves anything, and what bounds does
//! it run under. Both answers are properties of the request, so they are
//! derived once here.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
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
    #[must_use]
    pub const fn from_mode(mode: Option<JoinMode>) -> Option<RetrievingJoin> {
        match mode {
            Some(JoinMode::Flat) => Some(RetrievingJoin::Flat),
            Some(JoinMode::Inline) => Some(RetrievingJoin::Inline),
            Some(JoinMode::None) | None => None,
        }
    }

    /// The spec-level mode this join is.
    #[must_use]
    pub const fn mode(self) -> JoinMode {
        match self {
            RetrievingJoin::Flat => JoinMode::Flat,
            RetrievingJoin::Inline => JoinMode::Inline,
        }
    }
}

/// How far a traversal goes and which identifiers it must not retrieve.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraversalBounds {
    /// How many Relationship hops the traversal walks from the roots.
    pub depth: NonZeroU32,
    /// The entities the client already holds, which the traversal neither
    /// reads nor returns.
    pub contained_by: HashSet<Identifier>,
}

impl TraversalBounds {
    /// The bounds a request's `joinLevel` and `containedBy` set.
    ///
    /// Clause 4.5.23 walks one hop when `join` is present without `joinLevel`.
    /// Tables 6.4.3.2-1 and 6.5.3.1-1 make `containedBy` "Only applicable if
    /// joinLevel is present", so without a `joinLevel` it excludes nothing.
    #[must_use]
    pub fn of(join_level: Option<NonZeroU32>, contained_by: Option<&[Identifier]>) -> TraversalBounds {
        let contained_by = match (join_level, contained_by) {
            // clone: the bounds own the identifiers they exclude
            (Some(_), Some(ids)) => ids.iter().cloned().collect(),
            (Some(_), None) | (None, Some(_) | None) => HashSet::new(),
        };
        TraversalBounds {
            depth: traversal_depth(join_level),
            contained_by,
        }
    }
}

/// How many hops a traversal walks, defaulting the absent `joinLevel` to one
/// (clause 4.5.23).
///
/// Held apart from [`TraversalBounds::of`] because admission needs the depth
/// before any traversal exists, and must neither pay for the `containedBy` set
/// to get it nor restate the default and risk admitting against a different
/// depth than the traversal runs at.
#[must_use]
pub const fn traversal_depth(join_level: Option<NonZeroU32>) -> NonZeroU32 {
    match join_level {
        Some(level) => level,
        None => NonZeroU32::MIN,
    }
}

#[cfg(test)]
mod tests {
    use crate::join::{RetrievingJoin, TraversalBounds};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use pretty_assertions::assert_eq;
    use std::num::NonZeroU32;

    fn ids(raw: &[&str]) -> Vec<Identifier> {
        raw.iter().map(|id| id.parse().expect("valid URI")).collect()
    }

    /// Clause 4.5.23: `join=@none` is the absence of a join, so it never
    /// becomes a [`RetrievingJoin`] and the traversal is never reached.
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
    fn flat_and_inline_retrieve_and_round_trip() {
        for (mode, expected) in [(JoinMode::Flat, RetrievingJoin::Flat), (JoinMode::Inline, RetrievingJoin::Inline)] {
            let retrieving = RetrievingJoin::from_mode(Some(mode)).expect("a retrieving join");
            assert_eq!(retrieving, expected);
            assert_eq!(retrieving.mode(), mode);
        }
    }

    /// Clause 4.5.23: a `join` without a `joinLevel` walks exactly one hop.
    #[test]
    fn bounds_default_to_one_level() {
        let bounds = TraversalBounds::of(None, None);
        assert_eq!(bounds.depth, NonZeroU32::MIN);
        assert!(bounds.contained_by.is_empty());
    }

    /// Tables 6.4.3.2-1 and 6.5.3.1-1: `containedBy` is "Only applicable if
    /// joinLevel is present".
    #[test]
    fn contained_by_without_join_level_excludes_nothing() {
        let held = ids(&["urn:ngsi-ld:T:a"]);
        let bounds = TraversalBounds::of(None, Some(&held));
        assert_eq!(bounds.depth, NonZeroU32::MIN);
        assert!(bounds.contained_by.is_empty());
    }

    /// With a `joinLevel`, the stated depth is walked and every `containedBy`
    /// identifier is excluded.
    #[test]
    fn contained_by_with_join_level_is_excluded() {
        let held = ids(&["urn:ngsi-ld:T:a", "urn:ngsi-ld:T:b"]);
        let bounds = TraversalBounds::of(NonZeroU32::new(2), Some(&held));
        assert_eq!(bounds.depth.get(), 2);
        assert_eq!(bounds.contained_by, held.into_iter().collect());
    }
}
