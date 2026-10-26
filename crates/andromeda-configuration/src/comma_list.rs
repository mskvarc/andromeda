//! A flat list setting that an environment variable can set.
//!
//! An environment variable sets scalars only, so a list setting set from one
//! is written as its members separated by commas
//! (`ANDROMEDA__HTTP__COMPRESSION__ALGORITHMS=gzip,zstd`). A configuration
//! file writes the same setting as an array. Only fields whose type is a list
//! read a comma-separated string, so a comma inside a scalar, such as one in a
//! database URL, is never split.

use serde::{
    Deserialize,
    Deserializer,
    de::{Error, value::StrDeserializer},
};

/// The two spellings of a list setting.
#[derive(Deserialize)]
#[serde(untagged)]
enum ListSpelling<T> {
    /// An array, as a configuration file writes it.
    Array(Vec<T>),
    /// Members separated by commas, as an environment variable writes it.
    Commas(String),
}

/// Read a list setting written as an array or as comma-separated members.
///
/// Each member of a comma-separated string is trimmed and read as one member;
/// an empty string is the empty list.
///
/// # Errors
///
/// Returns the deserializer's error for a value that is neither, or for a
/// member that does not read as one.
pub fn array_or_comma_list<'de, D, C>(deserializer: D) -> Result<C, D::Error>
where
    D: Deserializer<'de>,
    C: IntoIterator + FromIterator<<C as IntoIterator>::Item>,
    C::Item: Deserialize<'de>,
{
    match ListSpelling::<C::Item>::deserialize(deserializer)? {
        ListSpelling::Array(members) => Ok(members.into_iter().collect()),
        ListSpelling::Commas(text) => text
            .split(',')
            .map(str::trim)
            .filter(|member| !member.is_empty())
            .map(|member| C::Item::deserialize(StrDeserializer::<D::Error>::new(member)).map_err(D::Error::custom))
            .collect(),
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{comma_list::array_or_comma_list, schema::content_coding::ContentCoding};
    use enumset::EnumSet;
    use pretty_assertions::assert_eq;
    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Codings {
        #[serde(deserialize_with = "array_or_comma_list")]
        algorithms: EnumSet<ContentCoding>,
    }

    #[test]
    fn an_array_and_a_comma_list_read_the_same_members() {
        let array: Codings = toml::from_str("algorithms = [\"gzip\", \"zstd\"]").expect("an array");
        let commas: Codings = toml::from_str("algorithms = \"gzip, zstd\"").expect("a comma list");
        assert_eq!(array.algorithms, ContentCoding::Gzip | ContentCoding::Zstd);
        assert_eq!(commas.algorithms, array.algorithms);
    }

    #[test]
    fn an_empty_string_is_the_empty_list_and_an_unknown_member_is_refused() {
        let empty: Codings = toml::from_str("algorithms = \"\"").expect("an empty list");
        assert!(empty.algorithms.is_empty());
        assert!(toml::from_str::<Codings>("algorithms = \"gzip,lzma\"").is_err());
    }
}
