//! The number the `jsonld_contexts.kind` column stores a Context's kind as.
//!
//! The clause 5.2.19 kind vocabulary and the migration-owned registry that
//! numbers it are member-for-member identical, and the translation is written
//! out as exhaustive matches in both directions: a kind a later revision of the
//! specification adds fails to compile here, which is where the decision about
//! its persisted number belongs.

use andromeda_context_registry::creation_kind::ContextCreationKind;
use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;

/// The registry member the store persists for a Context kind.
#[must_use]
pub const fn stored_kind(kind: &ContextKind) -> JsonldContextKindCode {
    match kind {
        ContextKind::Cached => JsonldContextKindCode::Cached,
        ContextKind::Hosted => JsonldContextKindCode::Hosted,
        ContextKind::ImplicitlyCreated => JsonldContextKindCode::ImplicitlyCreated,
    }
}

/// The registry member the store persists for a Context a policy creates.
#[must_use]
pub const fn created_kind(kind: ContextCreationKind) -> JsonldContextKindCode {
    match kind {
        ContextCreationKind::Hosted => JsonldContextKindCode::Hosted,
        ContextCreationKind::ImplicitlyCreated => JsonldContextKindCode::ImplicitlyCreated,
    }
}

/// The Context kind a stored registry member names.
#[must_use]
pub const fn kind_of(code: JsonldContextKindCode) -> ContextKind {
    match code {
        JsonldContextKindCode::Cached => ContextKind::Cached,
        JsonldContextKindCode::Hosted => ContextKind::Hosted,
        JsonldContextKindCode::ImplicitlyCreated => ContextKind::ImplicitlyCreated,
    }
}

#[cfg(test)]
mod tests {
    use crate::jsonld_context::kind_code::{created_kind, kind_of, stored_kind};
    use andromeda_context_registry::creation_kind::ContextCreationKind;
    use andromeda_ngsi_ld_model::jsonld_context::ContextKind;
    use andromeda_postgresql_codes::jsonld_context_kind::JsonldContextKindCode;
    use pretty_assertions::assert_eq;
    use strum::IntoEnumIterator;

    #[test]
    fn every_context_kind_survives_a_round_trip_through_its_stored_number() {
        for kind in ContextKind::iter() {
            assert_eq!(kind_of(stored_kind(&kind)), kind);
        }
    }

    /// The registry's label is the clause 5.2.19 token the domain enum renders.
    #[test]
    fn the_stored_number_of_a_kind_is_the_one_its_token_is_registered_under() {
        for code in JsonldContextKindCode::ALL {
            assert_eq!(code.label(), kind_of(*code).to_string());
        }
    }

    #[test]
    fn a_created_context_is_stored_under_the_kind_its_policy_names() {
        assert_eq!(created_kind(ContextCreationKind::Hosted), JsonldContextKindCode::Hosted);
        assert_eq!(created_kind(ContextCreationKind::ImplicitlyCreated), JsonldContextKindCode::ImplicitlyCreated);
    }
}
