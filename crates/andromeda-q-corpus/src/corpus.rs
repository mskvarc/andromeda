//! The filters of the corpus and the Entities each selects.

use crate::corpus_entity::CorpusEntity::{self, E1, E2, E3, E4, E5, E7, E8, E9, E10, E11, E12, E13, E14, E15, E16, E17, E18, E19};

/// Whether a filter reads a Linked Entity, which Query Entities admits only
/// with `join` and `joinLevel` 1 (clause 5.7.2.4) and a Subscription with no
/// bound.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Traversal {
    /// The filter reads the Entity's own Attributes.
    Local,
    /// The filter reads through one Relationship.
    OneHop,
}

/// One filter of the corpus and the Entities it selects.
#[derive(Clone, Copy, Debug)]
pub struct CorpusCase {
    /// The case's name, `C-` and two digits.
    pub id: &'static str,
    /// The filter as a client writes it.
    pub q: &'static str,
    /// The Attributes whose operands are expanded (`expandValues`).
    pub expand_values: &'static [&'static str],
    /// The Attributes whose compound members stay literal (`jsonKeys`).
    pub json_keys: &'static [&'static str],
    /// Whether the filter reads through a Relationship.
    pub traversal: Traversal,
    /// The Entities the filter selects, in any order.
    pub selects: &'static [CorpusEntity],
}

/// A case reading the Entity's own Attributes with no `expandValues` or
/// `jsonKeys`.
const fn local(id: &'static str, q: &'static str, selects: &'static [CorpusEntity]) -> CorpusCase {
    CorpusCase {
        id,
        q,
        expand_values: &[],
        json_keys: &[],
        traversal: Traversal::Local,
        selects,
    }
}

/// A case reading through one Relationship.
const fn hop(id: &'static str, q: &'static str, selects: &'static [CorpusEntity]) -> CorpusCase {
    CorpusCase {
        traversal: Traversal::OneHop,
        ..local(id, q, selects)
    }
}

/// The corpus.
///
/// An array target is not ordered, and an existential reading over instances
/// holds for negated operators too, so `n!=10` selects the Entity whose second
/// instance is 60. A target of another data type than the operand is unequal
/// to it and matches nothing else. A Relationship target answers equality and
/// inequality only. With no target language, any one value of a `languageMap`
/// is the target value, and `!=` holds only when no value matches. A member
/// that resolves to nothing makes the target element non-existent, so `!=`
/// fails too. A pattern searches anywhere in the string. Not-a-number is
/// unordered and unequal to every number, and an infinity orders past every
/// finite one. A Relationship object compares with an operand in its RFC 3986
/// normalised spelling, a String with the spelling written.
pub const CORPUS: &[CorpusCase] = &[
    local("C-01", "n==10", &[E1, E4, E5]),
    local("C-02", "n!=10", &[E2, E3, E5]),
    local("C-03", "n>15", &[E2, E5]),
    local("C-04", "n<=10", &[E1, E5]),
    local("C-05", "n==10..20", &[E1, E2, E5]),
    local("C-06", "n!=10..20", &[E3, E4, E5]),
    local("C-07", "n==10,20", &[E1, E2, E4, E5]),
    local("C-08", "n!=10,20", &[E3, E5]),
    local("C-09", "n", &[E1, E2, E3, E4, E5]),
    local("C-10", "s==\"abc\"", &[E1]),
    local("C-11", "s!=\"abc\"", &[E2, E3]),
    local("C-12", "s>\"abc\"", &[E2]),
    local("C-13", "s~=\"ab.*\"", &[E1, E2]),
    local("C-14", "s!~=\"abc\"", &[E2]),
    local("C-15", "b==true", &[E1]),
    local("C-16", "b!=true", &[E2]),
    local("C-17", "r==\"urn:x:1\"", &[E7]),
    local("C-18", "r>\"urn:a\"", &[]),
    local("C-19", "r~=\"urn:.*\"", &[]),
    local("C-20", "r!=\"urn:x:9\"", &[E7]),
    local("C-21", "rl==\"urn:x:2\"", &[E7]),
    local("C-22", "rl!=\"urn:x:2\"", &[]),
    local("C-23", "l==\"red\"", &[E8]),
    local("C-24", "l!=\"red\"", &[]),
    local("C-25", "l!=\"blue\"", &[E8]),
    local("C-26", "l[en]==\"red\"", &[E8]),
    local("C-27", "l[fr]==\"red\"", &[]),
    local("C-28", "l[de]!=\"red\"", &[E8]),
    local("C-29", "l~=\"r.*\"", &[E8]),
    local("C-30", "l!~=\"x\"", &[E8]),
    local("C-31", "l!~=\"rou\"", &[]),
    local("C-32", "d==2024-01-15", &[E9]),
    local("C-33", "d==\"2024-01-15\"", &[]),
    local("C-34", "d!=\"2024-01-15\"", &[E9]),
    local("C-35", "d~=\"2024.*\"", &[]),
    local("C-36", "tm>2024-01-01T00:00:00Z", &[E9]),
    local("C-37", "f>1", &[]),
    local("C-38", "f!=1", &[E10]),
    local("C-39", "g>1", &[E10]),
    local("C-40", "g==1", &[]),
    local("C-41", "m.observedAt>=2024-01-01T00:00:00Z", &[E11]),
    local("C-42", "m.unitCode==\"CEL\"", &[E11]),
    local("C-43", "m.unitCode~=\"C.*\"", &[E11]),
    local("C-44", "k.src==\"a\"", &[E12]),
    local("C-45", "k.src", &[E12]),
    local("C-46", "k.missing", &[]),
    local("C-47", "addr[city]==\"X\"", &[E13]),
    local("C-48", "addr[zip]>0", &[E13]),
    local("C-49", "n==10;s==\"abc\"", &[E1]),
    local("C-50", "n>15|b==true", &[E1, E2, E5]),
    local("C-51", "addr[missing]", &[]),
    local("C-52", "addr[missing]!=\"x\"", &[]),
    local("C-53", "r!=1..5", &[E7]),
    local("C-54", "l>\"m\"", &[E8]),
    local("C-55", "l!=\"a\"..\"m\"", &[E8]),
    local("C-56", "h==\"HTTP://Example.com/a\"", &[E14]),
    CorpusCase {
        expand_values: &["vp"],
        ..local("C-57", "vp==\"Male\"", &[E15])
    },
    local("C-58", "vp==\"Male\"", &[]),
    local("C-59", "lp==2", &[E16]),
    local("C-60", "lp>1", &[]),
    CorpusCase {
        json_keys: &["jp"],
        ..local("C-61", "jp[value]==\"x\"", &[E16])
    },
    local("C-62", "location[type]==\"Point\"", &[E14]),
    local("C-63", "hp==\"HTTP://Example.com/a\"", &[]),
    local("C-64", "h==\"HTTP://Example.com/a\",\"urn:x:9\"", &[E14]),
    hop("C-65", "link{n}==10", &[E17]),
    hop("C-66", "link{n}!=10", &[]),
    hop("C-67", "link2{n}", &[]),
    hop("C-68", "link{T:n}>5", &[E17]),
    hop("C-69", "link{Other:n}==10", &[]),
    local("C-70", "n1>5", &[E18]),
    local("C-71", "tm==2024-01-15T12:00:00.000Z", &[E9]),
    local("C-72", "t<13:00:00Z", &[E19]),
    local("C-73", "tm>2024-01-01T00:00:00,5Z", &[E9]),
    local("C-74", "k.missing!=\"a\"", &[]),
    local("C-75", "d!~=\"^a\"", &[]),
    local("C-76", "rl!=1..5", &[E7]),
    local("C-77", "r!=\"a\"..\"b\"", &[E7]),
    local("C-78", "addr!=\"X\"", &[E13]),
    local("C-79", "location!=\"x\"", &[E14]),
    hop("C-80", "llink{n}==10", &[E17]),
    local("C-81", "hm==\"HTTP://Example.com/a\"", &[E17]),
    CorpusCase {
        json_keys: &["jn"],
        ..local("C-82", "jn[a]==1", &[E16])
    },
    CorpusCase {
        json_keys: &["jn"],
        ..local("C-83", "jn[a]==2", &[])
    },
    CorpusCase {
        json_keys: &["jn"],
        ..local("C-84", "jn[a]!=2", &[E16])
    },
    local("C-85", "e0!=\"x\"", &[E18]),
    local("C-86", "e0==\"x\"", &[]),
    local("C-87", "e0", &[E18]),
];

#[cfg(test)]
mod tests {
    use crate::corpus::CORPUS;
    use std::collections::HashSet;

    #[test]
    fn every_case_has_a_name_of_its_own() {
        let mut names = HashSet::new();
        for case in CORPUS {
            assert!(names.insert(case.id), "{} is named twice", case.id);
        }
    }
}
