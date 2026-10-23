//! Which side of an Entity a type membership belongs to.
//!
//! `entity_type_members` holds one row per type an Entity carries in its
//! current state or its Temporal Evolution, keyed by type and then
//! identifier. The two sides hold the same types unless a core write changed
//! the current ones (clauses 5.6.11.4 and 5.6.12.4 only ever add a type to the
//! Evolution), so one row serves both and carries one flag per side. A core
//! statement reads and moves the current side, a temporal statement the
//! Evolution's, over a [`MembershipSide`].

use crate::current::EntityTypeMembers;

/// The side of an Entity a type-membership statement reads or writes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MembershipSide {
    /// The current state, which core reads serve.
    Current,
    /// The Temporal Evolution, which temporal reads serve.
    Evolution,
}

impl MembershipSide {
    /// The flag a membership row of this side carries.
    #[must_use]
    pub const fn flag(self) -> EntityTypeMembers {
        match self {
            MembershipSide::Current => EntityTypeMembers::IsCurrentMember,
            MembershipSide::Evolution => EntityTypeMembers::IsEvolutionMember,
        }
    }

    /// The other side, whose flag keeps a row this side gives up.
    #[must_use]
    pub const fn other(self) -> MembershipSide {
        match self {
            MembershipSide::Current => MembershipSide::Evolution,
            MembershipSide::Evolution => MembershipSide::Current,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::type_membership::MembershipSide;
    use pretty_assertions::assert_eq;
    use sea_query::Iden;

    #[test]
    fn each_side_reads_its_own_flag() {
        assert_eq!(MembershipSide::Current.flag().to_string(), "is_current_member");
        assert_eq!(MembershipSide::Evolution.flag().to_string(), "is_evolution_member");
    }

    #[test]
    fn each_side_is_the_other_of_its_other() {
        for side in [MembershipSide::Current, MembershipSide::Evolution] {
            assert_eq!(side.other().other(), side);
            assert_ne!(side.other(), side);
        }
    }
}
