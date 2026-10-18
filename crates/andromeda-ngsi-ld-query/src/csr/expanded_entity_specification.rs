//! The entity half of a clause 5.12 specification, with its type selector
//! expanded.
//!
//! Registrations hold the entity types they cover expanded, so a request is
//! matched against them with its clause 4.17 selector expanded too. Discovery
//! (clause 5.10.2) and every distributed operation (clause 4.3.6) specify the
//! entities they concern in these three members.

use crate::{pattern::Pattern, type_selection::grammar::TypeExpr};
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// The entities a request specifies for registration matching (clause 5.12):
/// an expanded type selector, entity identifiers and an identifier pattern,
/// each optional.
///
/// Clause 5.2.8 gives `id` precedence over `idPattern`; a reader takes the
/// identifier members through `IdentifierAxis`, which applies it.
#[derive(Debug, Clone, Copy, Default)]
pub struct ExpandedEntitySpecification<'spec> {
    /// The expanded clause 4.17 type selector, if any.
    pub entity_type: Option<&'spec TypeExpr>,
    /// The entity identifiers asked for; empty when none were.
    pub ids: &'spec [Identifier],
    /// The identifier pattern asked for, if any.
    pub id_pattern: Option<&'spec Pattern>,
}
