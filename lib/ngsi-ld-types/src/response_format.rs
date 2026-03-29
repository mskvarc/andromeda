//! NGSI-LD response format and join mode types.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Response format for entity queries (ETSI GS CIM 009 clause 4.5.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseFormat {
    Normalized,
    KeyValues,
    Simplified,
    Concise,
}

impl Default for ResponseFormat {
    fn default() -> Self {
        Self::Normalized
    }
}

impl fmt::Display for ResponseFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for ResponseFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "normalized" => Ok(Self::Normalized),
            "keyValues" => Ok(Self::KeyValues),
            "simplified" => Ok(Self::Simplified),
            "concise" => Ok(Self::Concise),
            _ => Err(format!("unknown response format: {}", s)),
        }
    }
}

impl ResponseFormat {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Normalized => "normalized",
            Self::KeyValues => "keyValues",
            Self::Simplified => "simplified",
            Self::Concise => "concise",
        }
    }

    pub fn is_simplified(self) -> bool {
        matches!(self, Self::KeyValues | Self::Simplified)
    }
}

/// Join mode for linked entities (ETSI GS CIM 009 clause 4.5.21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinMode {
    Inline,
    Flat,
}

impl fmt::Display for JoinMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Inline => f.write_str("inline"),
            Self::Flat => f.write_str("flat"),
        }
    }
}

impl std::str::FromStr for JoinMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "inline" => Ok(Self::Inline),
            "flat" => Ok(Self::Flat),
            _ => Err(format!("unknown join mode: {}", s)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_format_roundtrip() {
        for variant in [
            ResponseFormat::Normalized,
            ResponseFormat::KeyValues,
            ResponseFormat::Simplified,
            ResponseFormat::Concise,
        ] {
            let s = variant.to_string();
            let parsed: ResponseFormat = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn response_format_serde_roundtrip() {
        for variant in [
            ResponseFormat::Normalized,
            ResponseFormat::KeyValues,
            ResponseFormat::Simplified,
            ResponseFormat::Concise,
        ] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: ResponseFormat = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn is_simplified() {
        assert!(!ResponseFormat::Normalized.is_simplified());
        assert!(ResponseFormat::KeyValues.is_simplified());
        assert!(ResponseFormat::Simplified.is_simplified());
        assert!(!ResponseFormat::Concise.is_simplified());
    }

    #[test]
    fn join_mode_roundtrip() {
        for variant in [JoinMode::Inline, JoinMode::Flat] {
            let s = variant.to_string();
            let parsed: JoinMode = s.parse().unwrap();
            assert_eq!(variant, parsed);
        }
    }

    #[test]
    fn join_mode_serde_roundtrip() {
        for variant in [JoinMode::Inline, JoinMode::Flat] {
            let json = serde_json::to_string(&variant).unwrap();
            let parsed: JoinMode = serde_json::from_str(&json).unwrap();
            assert_eq!(variant, parsed);
        }
    }
}
