//! Reading the items of a `scope` member as its paths or its deletion (clause
//! 4.18).
//!
//! The same rule holds wherever a Scope is written or stored: the entity
//! member of a Core write, the stored entity, and the `value` of a reified
//! Scope instance of a Temporal Evolution (clause 4.5.6). Each item is a scope
//! path, or the NGSI-LD Null that clause 4.18 admits "only in case of deleted
//! scopes", so the Null stands alone or not at all.

use crate::{document::slot::Slot, scope_refusal::ScopeRefusal, scopes::Scopes};
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

/// One item of a `scope` member, as its holder reads it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScopeItem<'a> {
    /// The NGSI-LD Null.
    Null,
    /// A string, which has to be a scope path.
    Text(&'a str),
    /// Anything that is not a string.
    NotAString,
}

/// The paths `items` name, or the deletion the Null alone states.
///
/// # Errors
///
/// Returns [`ScopeRefusal::NotAString`] or [`ScopeRefusal::NotAScopePath`]
/// for the first item that is no scope path, [`ScopeRefusal::NullBesidePath`]
/// for the Null beside a path, [`ScopeRefusal::RepeatedNull`] for the Null
/// stated twice, and [`ScopeRefusal::Empty`] for no item at all.
pub fn scope_slot<'a>(items: impl IntoIterator<Item = ScopeItem<'a>>) -> Result<Slot<Scopes>, ScopeRefusal> {
    let mut paths = Vec::new();
    let mut deletions = 0_usize;
    for item in items {
        match item {
            ScopeItem::Null => deletions += 1,
            ScopeItem::Text(text) => paths.push(text.parse::<ScopePath>().map_err(|source| ScopeRefusal::NotAScopePath { source })?),
            ScopeItem::NotAString => return Err(ScopeRefusal::NotAString),
        }
    }
    match (deletions, paths.is_empty()) {
        (0, _) => Scopes::try_from_paths(paths).map(Slot::Set).map_err(|_| ScopeRefusal::Empty),
        (_, false) => Err(ScopeRefusal::NullBesidePath),
        (1, true) => Ok(Slot::Remove),
        (_, true) => Err(ScopeRefusal::RepeatedNull),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        document::slot::Slot,
        scope_refusal::ScopeRefusal,
        scope_slot::{ScopeItem, scope_slot},
        scopes::Scopes,
    };
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    fn scopes(paths: &[&str]) -> Scopes {
        Scopes::try_from_paths(paths.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path"))).expect("at least one path")
    }

    /// Clause 4.18: one or more paths, each kept once.
    #[test]
    fn paths_are_the_scopes_they_name() {
        pretty_assertions::assert_eq!(
            scope_slot([ScopeItem::Text("/Madrid"), ScopeItem::Text("Company"), ScopeItem::Text("/Madrid")]),
            Ok(Slot::Set(scopes(&["/Madrid", "Company"])))
        );
    }

    /// Clause 4.18: the NGSI-LD Null "shall be only used and only appear in
    /// case of deleted scopes".
    #[test]
    fn the_null_alone_deletes_the_scope() {
        pretty_assertions::assert_eq!(scope_slot([ScopeItem::Null]), Ok(Slot::Remove));
    }

    #[test]
    fn the_null_beside_a_path_or_twice_over_is_refused() {
        pretty_assertions::assert_eq!(scope_slot([ScopeItem::Text("/Madrid"), ScopeItem::Null]), Err(ScopeRefusal::NullBesidePath));
        pretty_assertions::assert_eq!(scope_slot([ScopeItem::Null, ScopeItem::Null]), Err(ScopeRefusal::RepeatedNull));
    }

    /// Clause 4.18 grammar: a `ScopeLevel` starts with a letter and holds only
    /// letters, digits and `_`.
    #[test]
    fn a_string_outside_the_scope_grammar_is_refused() {
        for text in ["/a-b", "", "/", "//Madrid", "/1st"] {
            assert!(
                matches!(scope_slot([ScopeItem::Text(text)]), Err(ScopeRefusal::NotAScopePath { .. })),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_value_that_is_not_a_string_is_refused() {
        pretty_assertions::assert_eq!(scope_slot([ScopeItem::Text("/Madrid"), ScopeItem::NotAString]), Err(ScopeRefusal::NotAString));
    }

    #[test]
    fn no_item_is_refused() {
        pretty_assertions::assert_eq!(scope_slot([]), Err(ScopeRefusal::Empty));
    }
}
