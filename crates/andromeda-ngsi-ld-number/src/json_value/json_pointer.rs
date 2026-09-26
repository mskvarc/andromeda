//! RFC 6901 JSON Pointer locations tracked during a document walk.
//!
//! A walk keeps the location of the node it visits as a stack of borrowed
//! segments and renders the pointer only when a node is rejected, so a
//! document that validates never builds a pointer string.

/// One step from a JSON value into one of its children.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PathSegment<'a> {
    /// The member of an object with this name.
    Member(&'a str),
    /// The element of an array at this index.
    Index(usize),
}

/// Render the JSON Pointer the segments spell, or `/` for the root.
pub(crate) fn render_pointer(segments: &[PathSegment<'_>]) -> String {
    if segments.is_empty() {
        return "/".to_owned();
    }
    segments.iter().fold(String::new(), |mut pointer, segment| {
        pointer.push('/');
        match segment {
            PathSegment::Member(name) => name.chars().for_each(|character| push_escaped(&mut pointer, character)),
            PathSegment::Index(index) => pointer.push_str(&index.to_string()),
        }
        pointer
    })
}

/// Append one member-name character, escaped as RFC 6901 section 3 requires.
fn push_escaped(pointer: &mut String, character: char) {
    match character {
        '~' => pointer.push_str("~0"),
        '/' => pointer.push_str("~1"),
        other => pointer.push(other),
    }
}

#[cfg(test)]
mod tests {
    use crate::json_value::json_pointer::{PathSegment, render_pointer};

    #[test]
    fn the_root_renders_as_a_single_slash() {
        pretty_assertions::assert_eq!(render_pointer(&[]), "/");
    }

    #[test]
    fn members_and_indexes_render_in_walk_order() {
        let segments = [PathSegment::Member("property"), PathSegment::Member("value"), PathSegment::Index(12)];
        pretty_assertions::assert_eq!(render_pointer(&segments), "/property/value/12");
    }

    #[test]
    fn tilde_and_slash_in_member_names_are_escaped_per_rfc_6901() {
        let segments = [PathSegment::Member("a~/b"), PathSegment::Member("~1"), PathSegment::Member("")];
        pretty_assertions::assert_eq!(render_pointer(&segments), "/a~0~1b/~01/");
    }
}
