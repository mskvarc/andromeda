//! The type-membership relation of one side, in one branch: the relation a
//! type selection walks and a write that sets an Entity's types moves.

use andromeda_postgresql_schema::{current::EntityTypeMembers, type_membership::MembershipSide};
use andromeda_postgresql_seaquery::branch::qualified;
use sea_query::{DynIden, TableRef};

/// The memberships of one side a walk or a probe reads, in its branch.
#[derive(Clone, Copy, Debug)]
pub struct Memberships<'a> {
    /// Which side.
    pub side: MembershipSide,
    /// The branch schema the relation is named in, `None` for the live branch.
    pub schema: Option<&'a DynIden>,
}

impl Memberships<'_> {
    /// The relation, qualified by its branch schema.
    #[must_use]
    pub fn table(self) -> TableRef {
        qualified(self.schema, EntityTypeMembers::Table)
    }
}
