//! The Entities the parity corpus is asked of.

use strum::{Display, EnumIter};

/// The entity type every corpus Entity has, as a hop guard writes it.
pub const CORPUS_TYPE: &str = "T";

/// One Entity of the corpus, of type [`CORPUS_TYPE`]. Every value is the
/// default instance of its Attribute unless stated otherwise.
#[derive(Clone, Copy, Debug, Display, EnumIter, Eq, Hash, Ord, PartialEq, PartialOrd)]
#[strum(serialize_all = "lowercase")]
pub enum CorpusEntity {
    /// `n` 10, `s` "abc", `b` true.
    E1,
    /// `n` 20, `s` "abd", `b` false.
    E2,
    /// `n` the String "10", `s` the Number 10.
    E3,
    /// `n` the array 10, 30.
    E4,
    /// `n` 10, and a second instance of `n`, 60, with a `datasetId`.
    E5,
    /// `other` 1, and none of `n`, `s` and `b`.
    E6,
    /// `r` a Relationship to `urn:x:1`, `rl` a `ListRelationship` to `urn:x:1`
    /// and `urn:x:2`.
    E7,
    /// `l` a `LanguageProperty` with `en` "red" and `fr` "rouge".
    E8,
    /// `d` "2024-01-15" declared a Date, `tm` "2024-01-15T12:00:00Z" declared a
    /// `DateTime`.
    E9,
    /// `f` "NaN" and `g` "INF", each declared an `xsd:double`.
    E10,
    /// `m` 5 with `observedAt` 2024-06-01T00:00:00Z and `unitCode` CEL.
    E11,
    /// `k` 1 with the sub-property `src` "a".
    E12,
    /// `addr` the object `{city: "X", zip: 1}`.
    E13,
    /// `h` a Relationship to `http://example.com/a`, `hp` the String
    /// `"http://example.com/a"`, and `location` a Point at 13.4, 52.5.
    E14,
    /// `vp` a `VocabProperty` holding the term `Male`.
    E15,
    /// `lp` a `ListProperty` 1, 2, `jp` a `JsonProperty` `{value: "x"}` and
    /// `jn` a `JsonProperty` `{a: [1, [2]]}`.
    E16,
    /// `link` a Relationship to [`CorpusEntity::E1`], `link2` one to
    /// [`CorpusEntity::E6`], `llink` a `ListRelationship` to
    /// [`CorpusEntity::E1`], created after both, and `hm` a Relationship to
    /// `http://example.com/a` and `urn:x:9`.
    E17,
    /// `n1` the one-element array 10, which reads back as 10, and `e0` the
    /// empty array.
    E18,
    /// `t` "12:00:00Z" declared a Time.
    E19,
}
