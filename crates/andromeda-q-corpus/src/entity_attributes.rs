//! The Attributes of every corpus Entity.

use crate::{
    attribute_description::{
        CorpusAttribute,
        CorpusContent::{self, GeoProperty, JsonProperty, LanguageProperty, ListProperty, ListRelationship, Property, Relationship, VocabProperty},
        CorpusDatatype,
        CorpusInstance,
        CorpusTarget::{Entity, Identifier},
        CorpusTranslation,
        CorpusValue::{Boolean, Declared, Integer, Members, Text},
    },
    corpus_entity::CorpusEntity,
};
use strum::IntoEnumIterator;

/// An instance holding `content`, with no `datasetId`, no members and no
/// sub-Attributes.
const fn instance(content: CorpusContent) -> CorpusInstance {
    CorpusInstance {
        content,
        dataset_id: None,
        observed_at: None,
        unit_code: None,
        sub_attributes: &[],
    }
}

/// An Attribute with the one default instance `content`.
macro_rules! attribute {
    ($term:literal, $content:expr) => {
        CorpusAttribute {
            term: $term,
            instances: &[instance($content)],
        }
    };
}

/// `n` 10, `s` "abc", `b` true.
const E1_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!("n", Property(&[Integer(10)])),
    attribute!("s", Property(&[Text("abc")])),
    attribute!("b", Property(&[Boolean(true)])),
];
/// `n` 20, `s` "abd", `b` false.
const E2_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!("n", Property(&[Integer(20)])),
    attribute!("s", Property(&[Text("abd")])),
    attribute!("b", Property(&[Boolean(false)])),
];
/// `n` the String "10", `s` the Number 10.
const E3_ATTRIBUTES: &[CorpusAttribute] = &[attribute!("n", Property(&[Text("10")])), attribute!("s", Property(&[Integer(10)]))];
/// `n` the array 10, 30.
const E4_ATTRIBUTES: &[CorpusAttribute] = &[attribute!("n", Property(&[Integer(10), Integer(30)]))];
/// `n` 10 and a second instance 60 with a `datasetId`.
const E5_ATTRIBUTES: &[CorpusAttribute] = &[CorpusAttribute {
    term: "n",
    instances: &[
        instance(Property(&[Integer(10)])),
        CorpusInstance {
            dataset_id: Some("urn:ngsi-ld:Dataset:corpus"),
            ..instance(Property(&[Integer(60)]))
        },
    ],
}];
/// `other` 1.
const E6_ATTRIBUTES: &[CorpusAttribute] = &[attribute!("other", Property(&[Integer(1)]))];
/// `r` to `urn:x:1`, `rl` to `urn:x:1` and `urn:x:2`.
const E7_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!("r", Relationship(&[Identifier("urn:x:1")])),
    attribute!("rl", ListRelationship(&[Identifier("urn:x:1"), Identifier("urn:x:2")])),
];
/// `l` with `en` "red" and `fr` "rouge".
const E8_ATTRIBUTES: &[CorpusAttribute] = &[attribute!(
    "l",
    LanguageProperty(&[
        CorpusTranslation { language: "en", text: "red" },
        CorpusTranslation { language: "fr", text: "rouge" },
    ])
)];
/// `d` a Date and `tm` a `DateTime`.
const E9_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!(
        "d",
        Property(&[Declared {
            datatype: CorpusDatatype::Date,
            lexical: "2024-01-15"
        }])
    ),
    attribute!(
        "tm",
        Property(&[Declared {
            datatype: CorpusDatatype::DateTime,
            lexical: "2024-01-15T12:00:00Z"
        }])
    ),
];
/// `f` "NaN" and `g` "INF", each an `xsd:double`.
const E10_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!(
        "f",
        Property(&[Declared {
            datatype: CorpusDatatype::Double,
            lexical: "NaN"
        }])
    ),
    attribute!(
        "g",
        Property(&[Declared {
            datatype: CorpusDatatype::Double,
            lexical: "INF"
        }])
    ),
];
/// `m` 5 observed on 2024-06-01 in `CEL`.
const E11_ATTRIBUTES: &[CorpusAttribute] = &[CorpusAttribute {
    term: "m",
    instances: &[CorpusInstance {
        observed_at: Some("2024-06-01T00:00:00Z"),
        unit_code: Some("CEL"),
        ..instance(Property(&[Integer(5)]))
    }],
}];
/// `k` 1 with the sub-Attribute `src` "a".
const E12_ATTRIBUTES: &[CorpusAttribute] = &[CorpusAttribute {
    term: "k",
    instances: &[CorpusInstance {
        sub_attributes: &[attribute!("src", Property(&[Text("a")]))],
        ..instance(Property(&[Integer(1)]))
    }],
}];
/// `addr` the object `{city: "X", zip: 1}`.
const E13_ATTRIBUTES: &[CorpusAttribute] = &[attribute!("addr", Property(&[Members(&[("city", Text("X")), ("zip", Integer(1))])]))];
/// `h` to `http://example.com/a`, `hp` the same text, `location` a Point.
const E14_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!("h", Relationship(&[Identifier("http://example.com/a")])),
    attribute!("hp", Property(&[Text("http://example.com/a")])),
    attribute!(
        "location",
        GeoProperty {
            longitude: 13.4,
            latitude: 52.5
        }
    ),
];
/// `vp` the term `Male`.
const E15_ATTRIBUTES: &[CorpusAttribute] = &[attribute!("vp", VocabProperty("Male"))];
/// `lp` the list 1, 2, `jp` the JSON `{"value":"x"}` and `jn` the JSON
/// `{"a":[1,[2]]}`.
const E16_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!("lp", ListProperty(&[Integer(1), Integer(2)])),
    attribute!("jp", JsonProperty("{\"value\":\"x\"}")),
    attribute!("jn", JsonProperty("{\"a\":[1,[2]]}")),
];
/// `link` to E1, `link2` to E6, `llink` a `ListRelationship` to E1 and `hm`
/// a Relationship to `http://example.com/a` and `urn:x:9`.
const E17_ATTRIBUTES: &[CorpusAttribute] = &[
    attribute!("link", Relationship(&[Entity(CorpusEntity::E1)])),
    attribute!("link2", Relationship(&[Entity(CorpusEntity::E6)])),
    attribute!("llink", ListRelationship(&[Entity(CorpusEntity::E1)])),
    attribute!("hm", Relationship(&[Identifier("http://example.com/a"), Identifier("urn:x:9")])),
];
/// `n1` the one-element array 10.
const E18_ATTRIBUTES: &[CorpusAttribute] = &[attribute!("n1", Property(&[Integer(10)]))];
/// `t` a Time.
const E19_ATTRIBUTES: &[CorpusAttribute] = &[attribute!(
    "t",
    Property(&[Declared {
        datatype: CorpusDatatype::Time,
        lexical: "12:00:00Z"
    }])
)];

impl CorpusEntity {
    /// The Attributes of the Entity in the order the corpus lists them.
    #[must_use]
    pub const fn attributes(self) -> &'static [CorpusAttribute] {
        match self {
            CorpusEntity::E1 => E1_ATTRIBUTES,
            CorpusEntity::E2 => E2_ATTRIBUTES,
            CorpusEntity::E3 => E3_ATTRIBUTES,
            CorpusEntity::E4 => E4_ATTRIBUTES,
            CorpusEntity::E5 => E5_ATTRIBUTES,
            CorpusEntity::E6 => E6_ATTRIBUTES,
            CorpusEntity::E7 => E7_ATTRIBUTES,
            CorpusEntity::E8 => E8_ATTRIBUTES,
            CorpusEntity::E9 => E9_ATTRIBUTES,
            CorpusEntity::E10 => E10_ATTRIBUTES,
            CorpusEntity::E11 => E11_ATTRIBUTES,
            CorpusEntity::E12 => E12_ATTRIBUTES,
            CorpusEntity::E13 => E13_ATTRIBUTES,
            CorpusEntity::E14 => E14_ATTRIBUTES,
            CorpusEntity::E15 => E15_ATTRIBUTES,
            CorpusEntity::E16 => E16_ATTRIBUTES,
            CorpusEntity::E17 => E17_ATTRIBUTES,
            CorpusEntity::E18 => E18_ATTRIBUTES,
            CorpusEntity::E19 => E19_ATTRIBUTES,
        }
    }
}

/// The terms of `attributes` and of their sub-Attributes, each after the term
/// that holds it, appended to `terms` unless already there.
fn collect_terms(attributes: &[CorpusAttribute], terms: &mut Vec<&'static str>) {
    for attribute in attributes {
        if !terms.contains(&attribute.term) {
            terms.push(attribute.term);
        }
        for instance in attribute.instances {
            collect_terms(instance.sub_attributes, terms);
        }
    }
}

/// Every term a corpus Entity writes as an Attribute or a sub-Attribute, once,
/// in the order the corpus first writes them.
#[must_use]
pub fn written_terms() -> Vec<&'static str> {
    let mut terms = Vec::new();
    for entity in CorpusEntity::iter() {
        collect_terms(entity.attributes(), &mut terms);
    }
    terms
}

#[cfg(test)]
mod tests {
    use crate::entity_attributes::written_terms;

    #[test]
    fn the_written_terms_are_listed_once_in_the_order_of_first_use() {
        assert_eq!(
            written_terms(),
            [
                "n", "s", "b", "other", "r", "rl", "l", "d", "tm", "f", "g", "m", "k", "src", "addr", "h", "hp", "location", "vp", "lp", "jp", "jn", "link",
                "link2", "llink", "hm", "n1", "t"
            ]
        );
    }
}
