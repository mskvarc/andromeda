//! A byte size rendered and parsed in the human-readable `bytesize` form.

use bytesize::ByteSize;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;

/// A [`ByteSize`] whose wire form is the human-readable spelling (`"64 MiB"`,
/// `"512 kB"`) and which also accepts a bare integer byte count.
///
/// The representation lives on the type for the reason
/// [`HumanDuration`](crate::human_duration::HumanDuration) gives: the
/// template, the effective configuration and the JSON Schema all see the same
/// spelling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HumanByteSize(ByteSize);

impl HumanByteSize {
    /// Wrap a [`ByteSize`] for human-readable serialisation.
    #[must_use]
    pub const fn new(size: ByteSize) -> HumanByteSize {
        HumanByteSize(size)
    }

    /// Construct from a whole number of bytes.
    #[must_use]
    pub const fn b(bytes: u64) -> HumanByteSize {
        HumanByteSize(ByteSize::b(bytes))
    }

    /// Construct from a whole number of kibibytes.
    #[must_use]
    pub const fn kib(kib: u64) -> HumanByteSize {
        HumanByteSize(ByteSize::kib(kib))
    }

    /// Construct from a whole number of mebibytes.
    #[must_use]
    pub const fn mib(mib: u64) -> HumanByteSize {
        HumanByteSize(ByteSize::mib(mib))
    }

    /// Construct from a whole number of gibibytes.
    #[must_use]
    pub const fn gib(gib: u64) -> HumanByteSize {
        HumanByteSize(ByteSize::gib(gib))
    }

    /// The size as a whole number of bytes, used by size-limit validation.
    #[must_use]
    pub const fn as_u64(self) -> u64 {
        self.0.as_u64()
    }
}

impl Serialize for HumanByteSize {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

// Manual: the size reads from a string or a bare integer, a shape no derive
// states.
impl JsonSchema for HumanByteSize {
    fn schema_name() -> Cow<'static, str> {
        Cow::Borrowed("ByteSize")
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "title": "size",
            "description": "A size such as \"512 KiB\", \"64 MiB\" or \"2 MB\", or a bare number of bytes.",
            "type": ["string", "integer"],
            "minimum": 0
        })
    }
}

impl<'de> Deserialize<'de> for HumanByteSize {
    fn deserialize<D>(deserializer: D) -> Result<HumanByteSize, D::Error>
    where
        D: Deserializer<'de>,
    {
        ByteSize::deserialize(deserializer).map(HumanByteSize)
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::human_byte_size::HumanByteSize;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Holder {
        size: HumanByteSize,
    }

    #[test]
    fn human_byte_size_serialises_as_a_human_string() {
        let holder = Holder { size: HumanByteSize::mib(64) };
        let rendered = toml::to_string(&holder).expect("holder serialises");
        assert!(rendered.contains("size = \"64"), "{rendered}");
    }

    #[test]
    fn human_byte_size_parses_a_human_string() {
        let holder: Holder = toml::from_str("size = \"64 MiB\"").expect("holder parses");
        pretty_assertions::assert_eq!(holder.size.as_u64(), 67_108_864);
    }

    #[test]
    fn human_byte_size_parses_a_bare_integer() {
        let holder: Holder = toml::from_str("size = 1024").expect("holder parses");
        pretty_assertions::assert_eq!(holder.size.as_u64(), 1024);
    }

    #[test]
    fn zero_bytes_is_reported_as_zero() {
        pretty_assertions::assert_eq!(HumanByteSize::b(0).as_u64(), 0);
    }
}
