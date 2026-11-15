//! A URI operand of an equality, in the two spellings RFC 3986 section 6
//! makes equivalent.

use crate::filter::reading::target_kind::TargetKind;

/// A `quotedStr` or a bare `URI` after `==` or `!=` whose RFC 3986 section 6
/// normalisation is another spelling.
///
/// Clause 4.9 asks that "URI comparison should be performed so that the
/// number of false negatives is minimized", and its URI bullets name a
/// Relationship's object, which the write path stores in its syntax- and
/// scheme-based normal form (RFC 3986 sections 6.2.2 and 6.2.3). A String
/// target holds the text its writer wrote, which clause 4.9 compares as the
/// text it is.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
pub struct UriOperand {
    /// The URI as the client wrote it.
    written: String,
    /// Its normal form, which differs from `written`.
    normalised: String,
}

impl UriOperand {
    /// The operand written as `written`, which normalises to `normalised`.
    pub(crate) const fn new(written: String, normalised: String) -> UriOperand {
        UriOperand { written, normalised }
    }

    /// The URI as the client wrote it, the spelling a stored or forwarded
    /// filter keeps.
    #[must_use]
    pub fn written(&self) -> &str {
        &self.written
    }

    /// The spelling a target of `kind` is compared with: the normal form for
    /// a Relationship's object, the written one for every other target.
    #[must_use]
    pub fn against(&self, kind: TargetKind) -> &str {
        match kind {
            TargetKind::RelationshipObject => &self.normalised,
            TargetKind::Number
            | TargetKind::NotANumber
            | TargetKind::PositiveInfinity
            | TargetKind::NegativeInfinity
            | TargetKind::String
            | TargetKind::Boolean
            | TargetKind::DateTime
            | TargetKind::Date
            | TargetKind::Time
            | TargetKind::Null
            | TargetKind::Object => &self.written,
        }
    }
}
