//! The identifier half of an entity selector, read once.

use crate::pattern::Pattern;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;

/// What an entity selector constrains about the entity identifier (clause
/// 5.2.33 and clause 5.2.8: "`id` takes precedence over `idPattern`").
///
/// A selector may be written with both an `id` and an `idPattern`; the
/// specification makes the pattern void in that case. Every consumer of a
/// selector reads this type instead of the two members, so no code path can
/// apply both.
#[derive(Debug, Clone, Copy)]
pub enum IdentifierAxis<'selector> {
    /// The entity identifier must be one of these.
    Ids(&'selector [Identifier]),
    /// The entity identifier must match this pattern.
    Pattern(&'selector Pattern),
    /// The selector does not constrain the identifier.
    Any,
}

impl<'selector> IdentifierAxis<'selector> {
    /// Read the identifier members of a selector.
    #[must_use]
    pub const fn of(ids: &'selector [Identifier], id_pattern: Option<&'selector Pattern>) -> IdentifierAxis<'selector> {
        match (ids, id_pattern) {
            ([], None) => IdentifierAxis::Any,
            ([], Some(pattern)) => IdentifierAxis::Pattern(pattern),
            ([_, ..], Some(_) | None) => IdentifierAxis::Ids(ids),
        }
    }

    /// Whether `id` satisfies the axis.
    #[must_use]
    pub fn admits(&self, id: &Identifier) -> bool {
        match self {
            IdentifierAxis::Ids(ids) => ids.contains(id),
            IdentifierAxis::Pattern(pattern) => pattern.is_match(id.as_str()),
            IdentifierAxis::Any => true,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{identifier_axis::IdentifierAxis, pattern::Pattern};
    use andromeda_ngsi_ld_lexicon::identifier::Identifier;

    fn id(value: &str) -> Identifier {
        value.parse().expect("valid identifier")
    }

    #[test]
    fn an_id_takes_precedence_over_an_id_pattern() {
        let ids = [id("urn:ngsi-ld:A:1")];
        let pattern = Pattern::parse("^urn:nomatch").expect("valid pattern");
        let axis = IdentifierAxis::of(&ids, Some(&pattern));
        assert!(matches!(axis, IdentifierAxis::Ids(_)));
        assert!(axis.admits(&id("urn:ngsi-ld:A:1")));
        assert!(!axis.admits(&id("urn:nomatch:1")));
    }

    #[test]
    fn a_pattern_alone_constrains_by_pattern() {
        let pattern = Pattern::parse("^urn:ngsi-ld:A:").expect("valid pattern");
        let axis = IdentifierAxis::of(&[], Some(&pattern));
        assert!(matches!(axis, IdentifierAxis::Pattern(_)));
        assert!(axis.admits(&id("urn:ngsi-ld:A:9")));
        assert!(!axis.admits(&id("urn:ngsi-ld:B:9")));
    }

    #[test]
    fn neither_member_leaves_the_identifier_unconstrained() {
        let axis = IdentifierAxis::of(&[], None);
        assert!(matches!(axis, IdentifierAxis::Any));
        assert!(axis.admits(&id("urn:ngsi-ld:A:1")));
    }
}
