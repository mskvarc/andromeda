//! What a string value of a temporal observation carries beside its text.

use andromeda_ngsi_ld_lexicon::reference::NgsiLdReference;
use langtag::LangTagBuf;

/// The JSON-LD 1.1 value object member a string carries: none, the `@type` of
/// a typed literal, or the `@language` of a language-tagged string. A value
/// object holds at most one of the two, so this is a choice rather than two
/// optional members.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextAnnotation {
    /// A plain string.
    Plain,
    /// A typed literal and its datatype IRI.
    Datatype(NgsiLdReference),
    /// A language-tagged string and its tag.
    Language(LangTagBuf),
}
