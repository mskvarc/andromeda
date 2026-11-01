//! The `[tls]` rules.

use crate::validate::{config_key::ConfigKey, rule::Rule};

/// A zero handshake deadline drops every TCP client before its TLS handshake
/// can finish, so no TLS listener could serve one.
pub(crate) fn rules() -> impl Iterator<Item = Rule> {
    [Rule::Positive(ConfigKey::TlsHandshakeTimeout)].into_iter()
}

#[cfg(test)]
mod tests {
    use crate::validate::{config_key::ConfigKey, rule_tests::assert_positive_duration};

    #[test]
    fn a_zero_handshake_timeout_is_refused() {
        assert_positive_duration(|config, value| config.tls.handshake_timeout = value, ConfigKey::TlsHandshakeTimeout);
    }
}
