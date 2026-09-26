//! What taking the attributes proxied registrations own (clause 4.3.6.3) out of
//! a write payload left of it.

/// What remains of a write payload once the attributes another source owns
/// are taken out of it.
///
/// The local phase of a distributed write (clauses 5.6.1.4 and following)
/// runs on "any remaining input data": nothing when no attribute is left, and
/// the payload the client sent when the strip took nothing out of it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AttributeStrip {
    /// None of the named attributes was present, so the payload is unchanged.
    Untouched,
    /// Some attributes were taken out and at least one remains.
    Narrowed,
    /// No attribute remains.
    Emptied,
}

impl AttributeStrip {
    /// Classify a strip by how many attributes the payload held before and
    /// after it.
    #[must_use]
    pub const fn from_counts(before: usize, after: usize) -> AttributeStrip {
        if after == 0 {
            AttributeStrip::Emptied
        } else if after < before {
            AttributeStrip::Narrowed
        } else {
            AttributeStrip::Untouched
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::attribute_strip::AttributeStrip;

    #[test]
    fn a_strip_that_removed_nothing_leaves_the_payload_untouched() {
        pretty_assertions::assert_eq!(AttributeStrip::from_counts(2, 2), AttributeStrip::Untouched);
    }

    #[test]
    fn a_strip_that_removed_some_attributes_narrows_the_payload() {
        pretty_assertions::assert_eq!(AttributeStrip::from_counts(3, 1), AttributeStrip::Narrowed);
    }

    #[test]
    fn a_payload_left_without_attributes_is_emptied_whether_or_not_anything_was_removed() {
        pretty_assertions::assert_eq!(AttributeStrip::from_counts(2, 0), AttributeStrip::Emptied);
        pretty_assertions::assert_eq!(AttributeStrip::from_counts(0, 0), AttributeStrip::Emptied);
    }
}
