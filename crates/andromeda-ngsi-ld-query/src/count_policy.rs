//! Whether a query reports the total number of its matches (clause 6.3.13).

use serde::Deserialize;

/// Whether a query reports the total number of matches (clause 6.3.13).
///
/// The distinction is not cosmetic: emitting the count means running a second
/// statement over the same predicate, so this decides whether that statement is
/// built at all.
///
/// It is read from the Boolean `count` URL parameter of Table 6.3.13-1, and an
/// absent parameter asks for no count.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(from = "bool")]
pub enum CountPolicy {
    /// Build and run the count statement, and emit `NGSILD-Results-Count`.
    Emit,
    /// Skip the count statement entirely.
    #[default]
    Omit,
}

// Table 6.3.13-1 sets `NGSILD-Results-Count` only when `count` is true; a
// derive cannot map a Boolean onto two unit variants.
impl From<bool> for CountPolicy {
    fn from(count: bool) -> CountPolicy {
        if count { CountPolicy::Emit } else { CountPolicy::Omit }
    }
}

#[cfg(test)]
mod tests {
    use crate::count_policy::CountPolicy;
    use serde::Deserialize;
    use serde_urlencoded::de::Error as UrlencodedError;

    /// A parameter set reading the clause 6.3.13 `count` the way every paged
    /// operation does.
    #[derive(Debug, Deserialize)]
    struct Counted {
        /// The `count` URL parameter.
        #[serde(default)]
        count: CountPolicy,
    }

    fn read(query: &str) -> Result<CountPolicy, UrlencodedError> {
        serde_urlencoded::from_str::<Counted>(query).map(|counted| counted.count)
    }

    /// Under Table 6.3.13-1 `count=true` asks for the total, and `count=false`
    /// or an absent `count` does not.
    #[test]
    fn the_count_parameter_decides_the_policy() {
        pretty_assertions::assert_eq!(read("count=true").ok(), Some(CountPolicy::Emit));
        pretty_assertions::assert_eq!(read("count=false").ok(), Some(CountPolicy::Omit));
        pretty_assertions::assert_eq!(read("").ok(), Some(CountPolicy::Omit));
    }

    /// Table 6.3.13-1 types `count` as a Boolean, so any other word is refused.
    #[test]
    fn a_count_that_is_no_boolean_is_refused() {
        assert!(read("count=yes").is_err());
        assert!(read("count=TRUE").is_err());
    }
}
