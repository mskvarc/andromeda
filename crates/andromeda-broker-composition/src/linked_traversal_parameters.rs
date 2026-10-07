//! The clause 4.5.23 traversal parameters a forwarded linked retrieval carries.
//!
//! Both the entity and the temporal Linked Entity sources ask a remote broker
//! to continue a traversal this broker started, and both express that in the
//! same three parameters: how much further to walk (`join`, `joinLevel`) and
//! which entities the requester already holds (`containedBy`). The window a
//! temporal retrieval adds is its own concern; this is the part that is the
//! traversal itself.

use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_representation::join_mode::JoinMode;
use std::num::NonZeroU32;

/// The traversal continuation of a forwarded retrieval, as the three clause
/// 4.5.23 parameters it is written with.
///
/// `depth` counts the traversal levels still to resolve, the fetched entity's
/// own level included, so the remote broker is asked to join one level less
/// than that. When nothing is left beyond the entity itself the retrieval
/// carries no `join` at all: the clause 4.5.23 `joinLevel` parameter admits
/// only a positive value, and a zero-hop join is the absence of one.
///
/// `visited` becomes `containedBy`: the clause 4.5.23 `containedBy` parameter
/// names the entities the requester already holds, which the remote broker
/// must not send back. It is "only applicable if joinLevel is present"
/// (Tables 6.4.3.2-1 and 6.5.3.1-1), so the entity alone is asked for without
/// it, and an empty list is omitted rather than sent empty.
pub(crate) fn traversal_continuation(depth: NonZeroU32, visited: &[Identifier]) -> (Option<JoinMode>, Option<NonZeroU32>, Option<Vec<Identifier>>) {
    let join_level = NonZeroU32::new(depth.get() - 1);
    let join = join_level.map(|_| JoinMode::Flat);
    let contained_by = (join_level.is_some() && !visited.is_empty()).then(|| visited.to_vec()); // clone: the forwarded parameters own the identifiers they carry
    (join, join_level, contained_by)
}

#[cfg(test)]
mod tests {
    use crate::linked_traversal_parameters::traversal_continuation;
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;
    use andromeda_ngsi_ld_representation::join_mode::JoinMode;
    use std::num::NonZeroU32;

    fn uri(value: &str) -> Identifier {
        value.parse().expect("valid URI")
    }

    fn depth(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("positive depth")
    }

    /// Three levels left means the fetched entity plus two more hops, so the
    /// remote broker is asked for a flat join two levels deep, and the
    /// already-walked ids travel as `containedBy`.
    #[test]
    fn the_continuation_joins_one_level_less_than_the_depth_that_remains() {
        let visited = [uri("urn:ngsi-ld:Building:1"), uri("urn:ngsi-ld:Building:2")];
        let (join, join_level, contained_by) = traversal_continuation(depth(3), &visited);
        pretty_assertions::assert_eq!(join, Some(JoinMode::Flat));
        pretty_assertions::assert_eq!(join_level.map(NonZeroU32::get), Some(2));
        pretty_assertions::assert_eq!(contained_by.as_deref(), Some(visited.as_slice()));
    }

    /// The last level asks for the entity alone, and a traversal that has
    /// visited nothing sends no `containedBy` at all.
    #[test]
    fn the_last_level_and_an_empty_visited_list_carry_nothing() {
        let (join, join_level, contained_by) = traversal_continuation(depth(1), &[]);
        assert!(join.is_none());
        assert!(join_level.is_none());
        assert!(contained_by.is_none());
    }

    /// Clause 4.5.23: `containedBy` applies only beside a `joinLevel`, so
    /// the entity alone is asked for without it, however much was visited.
    #[test]
    fn the_last_level_carries_no_contained_by_whatever_was_visited() {
        let (join, _, contained_by) = traversal_continuation(depth(1), &[uri("urn:ngsi-ld:Building:1")]);
        assert!(join.is_none());
        assert!(contained_by.is_none());
    }
}
