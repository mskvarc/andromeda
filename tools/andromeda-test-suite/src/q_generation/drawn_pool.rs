//! The pools a drawn value and a drawn operand share, small enough that a filter often meets the value it names.

/// The Attribute names an Entity and a filter are drawn over.
pub const NAMES: [&str; 8] = ["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"];

/// The strings a value and an operand are drawn from: plain text, a capital for the case-insensitive option, a URI
/// and two time-shaped strings, which are Strings because nothing declares them a time.
pub const STRINGS: [&str; 8] = ["a", "b", "ab", "ba", "A", "urn:x:1", "2024-01-15", "12:00:00Z"];

/// The URIs a Relationship names beside the Entities of its case, none of which identifies a drawn Entity.
pub const OBJECTS: [&str; 3] = ["urn:x:1", "urn:x:2", "urn:x:3"];

/// The Dates a value and an operand are drawn from.
pub const DATES: [&str; 4] = ["2024-01-15", "2024-03-10", "2024-06-30", "2024-09-01"];

/// The `DateTime`s a value and an operand are drawn from.
pub const DATE_TIMES: [&str; 4] = ["2024-01-15T12:00:00Z", "2024-03-10T06:30:00Z", "2024-06-30T00:00:00Z", "2024-09-01T18:00:00Z"];

/// The Times a value and an operand are drawn from.
pub const TIMES: [&str; 4] = ["08:30:00Z", "10:15:00Z", "12:00:00Z", "18:45:00Z"];

/// The vocabulary terms a `VocabProperty` holds.
pub const TERMS: [&str; 2] = ["Male", "Female"];

/// The member names the JSON object of a Property's value holds.
pub const STRUCTURE_MEMBERS: [&str; 2] = ["a", "b"];

/// The member names the JSON object of a `JsonProperty` holds; `value` is also a core term, which `jsonKeys` keeps
/// literal where the `@context` would expand it.
pub const JSON_MEMBERS: [&str; 3] = ["a", "b", "value"];

/// The members a trailing path names: those of a JSON object, the `type` and `coordinates` of a geometry, two
/// languages and `*`, which names every language.
pub const PATH_MEMBERS: [&str; 8] = ["a", "b", "value", "type", "coordinates", "en", "fr", "*"];

/// The ordinates and geometry type names of the pooled geometries an operand compares a `GeoProperty` member with.
pub const GEOMETRY_OPERANDS: [&str; 6] = ["0", "5", "175", "\"Point\"", "\"LineString\"", "\"Polygon\""];

/// An entity type no drawn Entity has, which a type hint of a Linked Entity path names so that the hop reads nothing.
pub const OTHER_TYPE: &str = "Other";

/// The members that hold an Attribute's value in its own representation, which a dotted path names as if they were
/// sub-Attributes.
pub const CORE_MEMBERS: [&str; 7] = ["value", "object", "languageMap", "valueList", "objectList", "json", "vocab"];

/// The patterns a filter matches strings with, each in IEEE 1003.2 as clause 4.9 mandates: anchors, alternation in a
/// group, bracket expressions with a range and a character class, bounded repetition, an optional atom, and the
/// case-insensitive option both engines share.
pub const PATTERNS: [&str; 12] = [
    "^a",
    "b$",
    "a.*",
    "^(a|b)$",
    "[ab]{2}",
    "^[[:alpha:]]+$",
    "(?i)^a",
    "^urn:x:[1-2]$",
    "^2024-",
    "[[:digit:]]",
    "^.$",
    "a?b",
];
