//! RFC 6901 JSON Pointer locations tracked during a document walk.
//!
//! A walk keeps the location of the node it visits as a stack of borrowed
//! [`PathSegment`]s and takes an owned [`JsonPointer`] only when a node is
//! rejected, so a document that validates never builds a pointer. Every walk
//! that reports where in a JSON document it refused something shares this one
//! pointer type.

use std::fmt;

/// One step from a JSON value into one of its children, borrowed from the
/// document being walked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathSegment<'a> {
    /// The member of an object with this name.
    Member(&'a str),
    /// The element of an array at this index.
    Index(usize),
}

/// One owned step of a [`JsonPointer`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonPointerSegment {
    /// The member of an object with this name. A JSON member name is arbitrary
    /// client text with no structure of its own.
    Member(Box<str>),
    /// The element of an array at this index.
    Index(usize),
}

/// An RFC 6901 JSON Pointer locating one node of a JSON document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonPointer(Vec<JsonPointerSegment>);

impl JsonPointer {
    /// The pointer the borrowed walk segments spell.
    #[must_use]
    pub fn from_walk(segments: &[PathSegment<'_>]) -> JsonPointer {
        JsonPointer(
            segments
                .iter()
                .map(|segment| match segment {
                    PathSegment::Member(name) => JsonPointerSegment::Member(Box::from(*name)),
                    PathSegment::Index(index) => JsonPointerSegment::Index(*index),
                })
                .collect(),
        )
    }

    /// The segments from the document root to the located node.
    #[must_use]
    pub fn segments(&self) -> &[JsonPointerSegment] {
        &self.0
    }
}

// Hand-written because the rendering escapes each member name character by
// character as RFC 6901 section 3 requires, and renders the root as `/` so a
// reader always sees a location; no derive expresses either.
impl fmt::Display for JsonPointer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.0.is_empty() {
            return f.write_str("/");
        }
        self.0.iter().try_for_each(|segment| {
            f.write_str("/")?;
            match segment {
                JsonPointerSegment::Member(name) => name.chars().try_for_each(|character| match character {
                    '~' => f.write_str("~0"),
                    '/' => f.write_str("~1"),
                    other => write!(f, "{other}"),
                }),
                JsonPointerSegment::Index(index) => write!(f, "{index}"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::json_value::json_pointer::{JsonPointer, JsonPointerSegment, PathSegment};

    #[test]
    fn the_root_renders_as_a_single_slash() {
        pretty_assertions::assert_eq!(JsonPointer::from_walk(&[]).to_string(), "/");
    }

    #[test]
    fn members_and_indexes_render_in_walk_order() {
        let segments = [PathSegment::Member("property"), PathSegment::Member("value"), PathSegment::Index(12)];
        pretty_assertions::assert_eq!(JsonPointer::from_walk(&segments).to_string(), "/property/value/12");
    }

    #[test]
    fn tilde_and_slash_in_member_names_are_escaped_per_rfc_6901() {
        let segments = [PathSegment::Member("a~/b"), PathSegment::Member("~1"), PathSegment::Member("")];
        pretty_assertions::assert_eq!(JsonPointer::from_walk(&segments).to_string(), "/a~0~1b/~01/");
    }

    #[test]
    fn an_owned_pointer_keeps_each_walk_segment_typed() {
        let segments = [PathSegment::Member("value"), PathSegment::Index(3)];
        pretty_assertions::assert_eq!(
            JsonPointer::from_walk(&segments).segments(),
            [JsonPointerSegment::Member(Box::from("value")), JsonPointerSegment::Index(3)]
        );
    }
}
