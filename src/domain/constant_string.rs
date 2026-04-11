/// Generates a type that serializes/deserializes as a fixed string constant.
/// Implements Clone, Copy, Debug, Default, PartialEq, Eq, Hash.
///
/// Usage:
///   constant_string!(TypeName, "the-value");
#[macro_export]
macro_rules! constant_string {
    ($name:ident, $value:literal) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        pub struct $name;

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str($value)
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str($value)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = <&str as serde::Deserialize>::deserialize(d)?;
                if s == $value {
                    Ok(Self)
                } else {
                    Err(serde::de::Error::invalid_value(serde::de::Unexpected::Str(s), &$value))
                }
            }
        }
    };
}
