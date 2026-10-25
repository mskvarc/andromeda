//! How a fragment's `scope` lands on an entity, for every entity-wide write
//! that carries one: Update Attributes (clause 5.6.2.4), Append Attributes
//! (clause 5.6.3.4) and Merge Entity (clause 5.6.17.4).
//!
//! Clause 4.18 makes `scope` one non-reified member with one value: "There
//! shall at most be one instance of the scope property per Entity". It is
//! never merged instance by instance: it is either replaced as a whole, which
//! is also what IETF RFC 7396 section 2 does with any member that is not an
//! object, or, when overwriting is not allowed, extended with the Scopes it
//! lacks.

use crate::{document::slot::Slot, entity_state::EntityState, scopes::Scopes};

/// Replace the entity's scope with the fragment's.
///
/// Clauses 5.6.2.4, 5.6.3.4 and 5.6.17.4 all say the scope of the target
/// "will become the one included in the Fragment" when overwriting is
/// allowed. The NGSI-LD Null that clause 4.18 reserves for a deleted scope
/// removes the member instead (clause 5.5.12), and removes nothing from an
/// entity without one.
pub(crate) fn replace_scope(entity: &mut EntityState, written: &Slot<Scopes>) {
    // clone: the entity owns its copy of the borrowed fragment's scope.
    entity.set_scope(written.as_set().cloned());
}

/// Append the fragment's Scopes the entity does not already carry, in
/// fragment order.
///
/// Clause 5.6.3.4 under `noOverwrite`: "the Scopes in the Fragment that are
/// not part of the value of scope of the target Entity will be appended to the
/// value of the scope of the target Entity". An entity without a scope takes
/// the fragment's whole. A deleted scope would overwrite, so `noOverwrite`
/// leaves the entity's scope as it is.
pub(crate) fn append_missing_scopes(entity: &mut EntityState, written: &Slot<Scopes>) {
    let Slot::Set(written) = written else {
        return;
    };
    let scopes = match entity.scope() {
        Some(held) if written.iter().all(|path| held.contains(path)) => return,
        Some(held) => {
            // clone: the extended scope replaces the held one, which the
            // entity only lends.
            let mut extended = held.clone();
            extended.extend(written.iter().cloned());
            extended
        }
        // clone: the entity owns its copy of the borrowed fragment's scope.
        None => written.clone(),
    };
    entity.set_scope(Some(scopes));
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        document::slot::Slot,
        entity_state::EntityState,
        entity_types::EntityTypes,
        operation::scope_write::{append_missing_scopes, replace_scope},
        scopes::Scopes,
    };
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    /// The scope holding the given paths.
    fn scope(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path")
    }

    /// A camera entity, with the given scope paths when there are any.
    fn entity(paths: Option<&[&str]>) -> EntityState {
        let mut entity = EntityState::new(
            "urn:ngsi-ld:Camera:1".parse().expect("a valid identifier"),
            EntityTypes::one("https://example.org/Camera".parse().expect("a valid type")),
        );
        entity.set_scope(paths.map(scope));
        entity
    }

    /// Clause 5.6.17.4: the scope of the target "will become the one included
    /// in the Fragment", so no stored path outlives the replacement.
    #[test]
    fn replacing_a_scope_installs_the_fragment_value_whole() {
        let mut target = entity(Some(&["/X/A", "/Y/A", "/Z/A"]));
        replace_scope(&mut target, &Slot::Set(scope(&["/A/B", "/C/D"])));
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D"])));
    }

    #[test]
    fn replacing_a_scope_array_with_one_path_leaves_only_that_path() {
        let mut target = entity(Some(&["/X/A", "/Y/A", "/Z/A"]));
        replace_scope(&mut target, &Slot::Set(scope(&["/G/H"])));
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/G/H"])));
    }

    /// A path the fragment shares with the target keeps the position the
    /// fragment gives it, and a stored path the fragment leaves out is gone.
    #[test]
    fn replacing_a_scope_keeps_a_path_the_fragment_repeats_in_its_fragment_position() {
        let mut target = entity(Some(&["/X/A", "/Y/A"]));
        replace_scope(&mut target, &Slot::Set(scope(&["/X/A", "/Q/R"])));
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/X/A", "/Q/R"])));
    }

    /// Clause 5.5.12: members of the Fragment that do not appear within the
    /// target "are added to the target".
    #[test]
    fn replacing_an_absent_scope_adds_it() {
        let mut target = entity(None);
        replace_scope(&mut target, &Slot::Set(scope(&["/A/B", "/C/D"])));
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D"])));
    }

    /// Clause 4.18 reserves the NGSI-LD Null for a deleted scope, and clause
    /// 5.5.12 removes the target member a fragment sets to it.
    #[test]
    fn replacing_a_scope_with_the_ngsi_ld_null_removes_the_member() {
        let mut target = entity(Some(&["/X/A", "/Y/A"]));
        replace_scope(&mut target, &Slot::Remove);
        pretty_assertions::assert_eq!(target.scope(), None);
    }

    /// Clause 5.6.3.4 under `noOverwrite`: only the Scopes the target lacks
    /// are appended, after the stored ones and in fragment order.
    #[test]
    fn appending_missing_scopes_adds_only_paths_the_target_lacks_in_fragment_order() {
        let mut target = entity(Some(&["/A/B", "/C/D"]));
        append_missing_scopes(&mut target, &Slot::Set(scope(&["/A/B", "/E/F", "/G/H"])));
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B", "/C/D", "/E/F", "/G/H"])));
    }

    #[test]
    fn appending_missing_scopes_to_an_entity_without_scope_installs_the_fragment() {
        let mut target = entity(None);
        append_missing_scopes(&mut target, &Slot::Set(scope(&["/A/B"])));
        pretty_assertions::assert_eq!(target.scope(), Some(&scope(&["/A/B"])));
    }
}
