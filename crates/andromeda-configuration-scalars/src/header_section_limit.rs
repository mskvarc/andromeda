//! The largest request header section a listener accepts, on every HTTP
//! version.

use crate::human_byte_size::HumanByteSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A request header section limit outside the range every HTTP version can
/// enforce identically.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum HeaderSectionLimitError {
    /// The HTTP/1.1 listener reads a request head into a buffer whose ceiling
    /// is this limit, and that buffer is never smaller than 8 KiB.
    #[error("server.max_request_header_section of {0} bytes is below the 8 KiB minimum")]
    BelowMinimum(u64),
    /// The HTTP/1.1 parser refuses a target URI over 65,534 bytes with 414
    /// once it holds the whole head, and its read buffer can grow to twice its
    /// ceiling. At 32 KiB that buffer never holds a whole head carrying such a
    /// URI, so every request over the limit is refused 431 there too.
    #[error("server.max_request_header_section of {0} bytes is above the 32 KiB maximum")]
    AboveMaximum(u64),
}

/// The largest request header section a listener accepts.
///
/// The section is measured as RFC 9113 section 6.5.2 and RFC 9114 section
/// 4.2.2 measure it: over every field, the pseudo-header fields included, the
/// length of the name plus the length of the value plus 32 octets. A request
/// over the limit is answered 431 Request Header Fields Too Large (RFC 6585
/// section 5) on HTTP/1.1, HTTP/2 and HTTP/3 alike. Its wire form is a byte
/// size (`"32 KiB"`), from 8 KiB to 32 KiB.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "HumanByteSize", into = "HumanByteSize")]
pub struct HeaderSectionLimit(u16);

impl HeaderSectionLimit {
    /// The smallest limit, 8 KiB.
    pub const MINIMUM: HeaderSectionLimit = HeaderSectionLimit(8 * 1024);

    /// The largest limit, 32 KiB.
    pub const MAXIMUM: HeaderSectionLimit = HeaderSectionLimit(32 * 1024);

    /// The limit in octets.
    #[must_use]
    pub const fn octets(self) -> u16 {
        self.0
    }
}

impl TryFrom<HumanByteSize> for HeaderSectionLimit {
    type Error = HeaderSectionLimitError;

    fn try_from(size: HumanByteSize) -> Result<HeaderSectionLimit, HeaderSectionLimitError> {
        let octets = size.as_u64();
        if octets < u64::from(HeaderSectionLimit::MINIMUM.0) {
            Err(HeaderSectionLimitError::BelowMinimum(octets))
        } else {
            u16::try_from(octets)
                .ok()
                .filter(|octets| *octets <= HeaderSectionLimit::MAXIMUM.0)
                .map(HeaderSectionLimit)
                .ok_or(HeaderSectionLimitError::AboveMaximum(octets))
        }
    }
}

impl From<HeaderSectionLimit> for HumanByteSize {
    fn from(limit: HeaderSectionLimit) -> HumanByteSize {
        HumanByteSize::b(u64::from(limit.0))
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        header_section_limit::{HeaderSectionLimit, HeaderSectionLimitError},
        human_byte_size::HumanByteSize,
    };
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use std::error::Error;

    #[derive(Debug, Deserialize, Serialize)]
    struct Holder {
        limit: HeaderSectionLimit,
    }

    #[test]
    fn a_header_section_limit_below_8_kib_is_refused() {
        assert_eq!(
            HeaderSectionLimit::try_from(HumanByteSize::b(8191)),
            Err(HeaderSectionLimitError::BelowMinimum(8191))
        );
    }

    #[test]
    fn a_header_section_limit_of_8_kib_is_accepted() -> Result<(), HeaderSectionLimitError> {
        assert_eq!(HeaderSectionLimit::try_from(HumanByteSize::kib(8))?, HeaderSectionLimit::MINIMUM);
        Ok(())
    }

    #[test]
    fn a_header_section_limit_of_32_kib_is_accepted() -> Result<(), HeaderSectionLimitError> {
        assert_eq!(HeaderSectionLimit::try_from(HumanByteSize::kib(32))?.octets(), 32_768);
        Ok(())
    }

    #[test]
    fn a_header_section_limit_over_32_kib_is_refused() {
        for octets in [32_769, 65_536, u64::MAX] {
            assert_eq!(
                HeaderSectionLimit::try_from(HumanByteSize::b(octets)),
                Err(HeaderSectionLimitError::AboveMaximum(octets))
            );
        }
    }

    #[test]
    fn a_header_section_limit_parses_a_human_size_and_renders_one() -> Result<(), Box<dyn Error>> {
        let holder: Holder = toml::from_str("limit = \"16 KiB\"")?;
        assert_eq!(holder.limit.octets(), 16_384);
        let rendered = toml::to_string(&holder)?;
        assert!(rendered.contains("limit = \"16.0 KiB\""), "{rendered}");
        Ok(())
    }

    #[test]
    fn a_header_section_limit_out_of_range_fails_to_parse() {
        let outcome = toml::from_str::<Holder>("limit = \"64 KiB\"");
        assert!(outcome.is_err(), "{outcome:?}");
    }
}
