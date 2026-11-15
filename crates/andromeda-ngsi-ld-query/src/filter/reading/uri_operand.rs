//! A URI operand of an equality, in the two spellings RFC 3986 section 6
//! makes equivalent.

use crate::filter::reading::target_kind::TargetKind;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use derive_where::derive_where;

/// A `quotedStr` or a bare `URI` after `==` or `!=` whose RFC 3986 section 6
/// normalisation is another spelling.
///
/// Clause 4.9 asks that "URI comparison should be performed so that the
/// number of false negatives is minimized", and its URI bullets name a
/// Relationship's object, which the write path stores in its syntax- and
/// scheme-based normal form (RFC 3986 sections 6.2.2 and 6.2.3). A String
/// target holds the text its writer wrote, which clause 4.9 compares as the
/// text it is.
#[derive(Debug, Clone, serde::Serialize)]
#[derive_where(PartialEq, Eq, PartialOrd, Ord)]
pub struct UriOperand {
    /// The URI as the client wrote it.
    written: String,
    /// Its normal form, which differs from `written`, boxed since an
    /// identifier is far larger than the other operands. It is a function of
    /// `written`, so equality and order read `written` alone.
    #[derive_where(skip)]
    normalised: Box<Identifier>,
}

impl UriOperand {
    /// The operand written as `written`, which normalises to `normalised`.
    pub(crate) fn new(written: String, normalised: Identifier) -> UriOperand {
        UriOperand {
            written,
            normalised: Box::new(normalised),
        }
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
            TargetKind::RelationshipObject => self.normalised.as_str(),
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
