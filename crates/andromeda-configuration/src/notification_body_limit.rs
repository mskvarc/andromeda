//! The largest notification body the broker sends over HTTP.

use crate::human_byte_size::HumanByteSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A notification body limit no HTTP delivery can enforce.
#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum NotificationBodyLimitError {
    /// A zero limit refuses every notification.
    #[error("notifications.delivery.max_http_body must be above zero")]
    Zero,
    /// A limit past the platform address width would admit every body.
    #[error("notifications.delivery.max_http_body of {0} bytes does not fit the platform address width")]
    ExceedsAddressWidth(u64),
}

/// The largest serialised notification body an HTTP delivery sends.
///
/// A notification over it is not sent: its delivery fails without a retry,
/// since every attempt would write the same body. Its wire form is a byte size
/// (`"128 MiB"`).
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, JsonSchema)]
#[serde(try_from = "HumanByteSize", into = "HumanByteSize")]
#[schemars(description = "A notification body size, written as a byte size such as `\"128 MiB\"`. Zero is refused.")]
pub struct NotificationBodyLimit {
    /// The limit in octets, as the delivery compares a body's length with it.
    #[schemars(description = "The limit in bytes.")]
    octets: usize,
    /// The limit as it was written, rendered back unchanged.
    #[schemars(description = "The limit as it was written.")]
    written: HumanByteSize,
}

impl NotificationBodyLimit {
    /// The default limit, 128 MiB: above the default request body, so an
    /// entity a client could write is never too large to notify.
    pub const DEFAULT: NotificationBodyLimit = NotificationBodyLimit {
        octets: 128 * 1024 * 1024,
        written: HumanByteSize::mib(128),
    };

    /// The limit in octets.
    #[must_use]
    pub const fn octets(self) -> usize {
        self.octets
    }

    /// The limit as the byte size it was written as.
    #[must_use]
    pub const fn byte_size(self) -> HumanByteSize {
        self.written
    }
}

impl TryFrom<HumanByteSize> for NotificationBodyLimit {
    type Error = NotificationBodyLimitError;

    fn try_from(size: HumanByteSize) -> Result<NotificationBodyLimit, NotificationBodyLimitError> {
        match size.as_u64() {
            0 => Err(NotificationBodyLimitError::Zero),
            octets => usize::try_from(octets)
                .map(|octets| NotificationBodyLimit { octets, written: size })
                .map_err(|_| NotificationBodyLimitError::ExceedsAddressWidth(octets)),
        }
    }
}

impl From<NotificationBodyLimit> for HumanByteSize {
    fn from(limit: NotificationBodyLimit) -> HumanByteSize {
        limit.written
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        human_byte_size::HumanByteSize,
        notification_body_limit::{NotificationBodyLimit, NotificationBodyLimitError},
    };
    use pretty_assertions::assert_eq;
    use serde::{Deserialize, Serialize};
    use std::error::Error;

    #[derive(Debug, Deserialize, Serialize)]
    struct Holder {
        limit: NotificationBodyLimit,
    }

    #[test]
    fn a_zero_body_limit_is_refused() {
        assert_eq!(NotificationBodyLimit::try_from(HumanByteSize::b(0)), Err(NotificationBodyLimitError::Zero));
    }

    #[test]
    fn a_body_limit_of_one_byte_is_accepted() -> Result<(), NotificationBodyLimitError> {
        assert_eq!(NotificationBodyLimit::try_from(HumanByteSize::b(1))?.octets(), 1);
        Ok(())
    }

    /// The address width is checked only where `usize` is narrower than
    /// `u64`, the only platforms a byte size can pass it on.
    #[test]
    fn a_body_limit_past_the_address_width_is_refused() {
        let outcome = NotificationBodyLimit::try_from(HumanByteSize::b(u64::MAX));
        if usize::try_from(u64::MAX).is_ok() {
            assert!(outcome.is_ok(), "{outcome:?}");
        } else {
            assert_eq!(outcome, Err(NotificationBodyLimitError::ExceedsAddressWidth(u64::MAX)));
        }
    }

    #[test]
    fn the_default_body_limit_is_128_mib() {
        assert_eq!(NotificationBodyLimit::DEFAULT.octets(), 134_217_728);
    }

    #[test]
    fn a_body_limit_parses_a_human_size_and_renders_one() -> Result<(), Box<dyn Error>> {
        let holder: Holder = toml::from_str("limit = \"20 MiB\"")?;
        assert_eq!(holder.limit.octets(), 20_971_520);
        let rendered = toml::to_string(&holder)?;
        assert!(rendered.contains("limit = \"20.0 MiB\""), "{rendered}");
        Ok(())
    }
}
