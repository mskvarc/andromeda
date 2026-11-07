//! The address width a configured byte size has to fit.
//!
//! A byte size is configured as a `u64`, and a limit the broker enforces on
//! an in-memory buffer is a `usize`. A size past the platform's address width
//! sets a limit no buffer can reach. The validator takes the width as a
//! parameter, so a rule over it is checked for every platform on any one.

/// A pointer width, in bits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct AddressWidth {
    /// The number of bits an address holds.
    bits: u32,
}

impl AddressWidth {
    /// The width of the platform the broker is built for.
    pub(crate) const PLATFORM: AddressWidth = AddressWidth { bits: usize::BITS };

    /// A width of `bits` bits.
    #[cfg(test)]
    pub(crate) const fn new(bits: u32) -> AddressWidth {
        AddressWidth { bits }
    }

    /// Whether a buffer of `bytes` bytes can be addressed: whether `bytes`
    /// is at most two to the power of the width, less one.
    pub(crate) const fn admits(self, bytes: u64) -> bool {
        self.bits >= u64::BITS || bytes >> self.bits == 0
    }
}

#[cfg(test)]
mod tests {
    use crate::validate::address_width::AddressWidth;
    use pretty_assertions::assert_eq;

    #[test]
    fn a_width_admits_sizes_up_to_its_largest_address() {
        let narrow = AddressWidth::new(32);
        assert_eq!([u64::from(u32::MAX), u64::from(u32::MAX) + 1].map(|bytes| narrow.admits(bytes)), [true, false]);
        assert!(AddressWidth::new(64).admits(u64::MAX));
    }

    #[test]
    fn the_platform_width_admits_what_a_usize_holds() {
        assert_eq!(AddressWidth::PLATFORM.admits(u64::MAX), usize::try_from(u64::MAX).is_ok());
    }
}
