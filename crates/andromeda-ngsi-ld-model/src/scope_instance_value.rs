//! The value the reified temporal Scope of one write carries (clause 4.18).
//!
//! Clause 4.18 gives an Entity at most one instance of the scope Property and
//! says "The value of scope is represented as a JSON array in case there is
//! more than one Scope", so one write is one instance whose value is either the
//! single Scope it named or the array of the several it named. Annex C, clause
//! C.5.16 renders both shapes side by side: Vehicle B9211's bare
//! `"/Madrid/Centro"` and Vehicle A8311's
//! `["/Madrid/Centro", "/Company123/UnitA"]`.
//!
//! Two readers rebuild that instance — the stored scope history and the overlay
//! over intents a deferred sink has not applied yet — and they have to agree on
//! the shape byte for byte, so the shaping is written once here.

use crate::temporal::TemporalValue;
use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

/// The value one write's Scope paths amount to, in the order the write recorded
/// them.
///
/// `None` when the write named no Scope at all, which is not an instance: a
/// write that ends an Entity's Scope records the removal clause 4.18 reserves
/// the NGSI-LD Null for, and that carries its own deletion instant rather than
/// an empty list of paths.
#[must_use]
pub fn scope_instance_value(paths: &[ScopePath]) -> Option<TemporalValue> {
    match paths {
        [] => None,
        // clone: the instance owns the paths, which outlive the rows or entries
        // they were read from.
        several => Some(TemporalValue::Scopes(several.to_vec())),
    }
}

#[cfg(test)]
mod tests {
    use crate::{scope_instance_value::scope_instance_value, temporal::TemporalValue};
    use andromeda_ngsi_ld_lexicon::scope::path::ScopePath;

    fn paths(raw: &[&str]) -> Vec<ScopePath> {
        raw.iter().map(|path| path.parse::<ScopePath>().expect("a valid scope path")).collect()
    }

    /// Annex C clause C.5.16 renders Vehicle B9211's single Scope as the bare
    /// path, which the one-element list compacts back to.
    #[test]
    fn one_scope_is_the_one_path_it_names() {
        pretty_assertions::assert_eq!(
            scope_instance_value(&paths(&["/Madrid/Centro"])),
            Some(TemporalValue::Scopes(paths(&["/Madrid/Centro"])))
        );
    }

    /// Clause 4.18: "The value of scope is represented as a JSON array in case
    /// there is more than one Scope", which annex C clause C.5.16 renders for
    /// Vehicle A8311 in the order the write named them.
    #[test]
    fn several_scopes_are_kept_in_the_order_they_were_recorded() {
        pretty_assertions::assert_eq!(
            scope_instance_value(&paths(&["/Madrid/Centro", "/Company123/UnitA"])),
            Some(TemporalValue::Scopes(paths(&["/Madrid/Centro", "/Company123/UnitA"])))
        );
    }

    /// A write that named no Scope records a removal rather than an instance
    /// holding nothing.
    #[test]
    fn no_scope_is_no_instance() {
        assert!(scope_instance_value(&paths(&[])).is_none());
    }
}
