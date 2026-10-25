//! What a pattern fixes about the text its subject begins with.
//!
//! A store holding the subjects in order reads a pattern that fixes this text
//! as the one range of subjects beginning with it: PostgreSQL derives that
//! range from a `^` followed by a literal. A pattern that fixes nothing has no
//! range, so every subject is tested, or a store reads it through an index of
//! the text's pieces instead.

use regex_syntax::hir::{Hir, HirKind, Look};

/// Whether every match of a pattern begins its subject with the same text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubjectStart {
    /// The pattern is anchored at the subject's start, and a literal follows
    /// the anchor.
    Fixed,
    /// A match may begin the subject with any text: no anchor, a line anchor
    /// of `(?m)`, an alternation, or a class (a case-folded letter among
    /// them) right after the anchor.
    Open,
}

impl SubjectStart {
    /// The start `hir` fixes.
    pub(crate) fn of(hir: &Hir) -> SubjectStart {
        match hir.kind() {
            HirKind::Concat(parts) => match parts.as_slice() {
                [anchor, literal, ..] if matches!(anchor.kind(), HirKind::Look(Look::Start)) && matches!(literal.kind(), HirKind::Literal(_)) => {
                    SubjectStart::Fixed
                }
                _ => SubjectStart::Open,
            },
            HirKind::Empty
            | HirKind::Literal(_)
            | HirKind::Class(_)
            | HirKind::Look(_)
            | HirKind::Repetition(_)
            | HirKind::Capture(_)
            | HirKind::Alternation(_) => SubjectStart::Open,
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{pattern::Pattern, subject_start::SubjectStart};
    use pretty_assertions::assert_eq;

    fn start(raw: &str) -> SubjectStart {
        Pattern::parse(raw).expect("valid pattern").subject_start()
    }

    #[test]
    fn an_anchor_followed_by_a_literal_fixes_the_start() {
        for raw in ["^urn:ngsi-ld:Vehicle:.*", "^urn:ngsi-ld:Vehicle:42$", "^u"] {
            assert_eq!(start(raw), SubjectStart::Fixed, "{raw}");
        }
    }

    #[test]
    fn a_pattern_without_a_leading_anchor_and_literal_leaves_the_start_open() {
        for raw in [
            ".*_1196287$",
            "urn:ngsi-ld:Vehicle:.*",
            "^.*Vehicle",
            "^[ab]c",
            "^(urn|did):.*",
            "^urn:a|^urn:b",
            "(?m)^urn",
            "(?i)^urn",
            "^",
            "",
        ] {
            assert_eq!(start(raw), SubjectStart::Open, "{raw}");
        }
    }
}
