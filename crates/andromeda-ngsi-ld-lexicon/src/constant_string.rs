//! The macro behind every NGSI-LD `type` discriminator fixed to a single token.

/// Generates a type that serializes and deserializes as one fixed string constant,
/// with the visibility written before its name.
///
/// The generated type is a unit struct deriving `Clone`, `Copy`, `Debug`,
/// `Default`, `PartialEq`, `Eq` and `Hash`, plus hand-written `Display`,
/// `Serialize` and `Deserialize`. Deserialization accepts exactly `$value` and
/// rejects anything else with `invalid_value`.
///
/// Use it for the `type` discriminators NGSI-LD fixes to a single token
/// (`Subscription`, `Notification`, `EntityMap`, …), so no other value can be
/// constructed or parsed.
///
/// The deserializer is a visitor rather than `<&str>::deserialize` so it works
/// through `serde_json::from_value` — which hands over owned strings — as well
/// as through `from_str`.
///
/// # Usage
///
/// ```ignore
/// constant_string!(pub SubscriptionType, "Subscription");
/// ```
#[macro_export]
macro_rules! constant_string {
    ($vis:vis $name:ident, $value:literal) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        #[doc = concat!("The fixed `", $value, "` type discriminator.")]
        $vis struct $name;

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                f.write_str($value)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(&self, s: S) -> ::core::result::Result<S::Ok, S::Error> {
                s.serialize_str($value)
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(d: D) -> ::core::result::Result<Self, D::Error> {
                struct ConstantVisitor;

                impl ::serde::de::Visitor<'_> for ConstantVisitor {
                    type Value = $name;

                    fn expecting(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                        f.write_str(concat!("the string \"", $value, "\""))
                    }

                    fn visit_str<E: ::serde::de::Error>(self, v: &str) -> ::core::result::Result<$name, E> {
                        if v == $value {
                            ::core::result::Result::Ok($name)
                        } else {
                            ::core::result::Result::Err(E::invalid_value(::serde::de::Unexpected::Str(v), &$value))
                        }
                    }
                }

                d.deserialize_str(ConstantVisitor)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    constant_string!(SampleType, "Sample");

    #[test]
    fn a_constant_string_serialises_to_exactly_its_literal() {
        pretty_assertions::assert_eq!(serde_json::to_value(SampleType).expect("serialises"), serde_json::json!("Sample"));
        pretty_assertions::assert_eq!(SampleType.to_string(), "Sample");
    }

    #[test]
    fn a_constant_string_deserialises_from_its_literal_through_both_serde_entry_points() {
        let from_str: SampleType = serde_json::from_str("\"Sample\"").expect("parses from a string");
        pretty_assertions::assert_eq!(from_str, SampleType);
        let from_value: SampleType = serde_json::from_value(Value::String("Sample".to_string())).expect("parses from a value");
        pretty_assertions::assert_eq!(from_value, SampleType);
    }

    #[test]
    fn a_constant_string_rejects_anything_but_its_literal() {
        assert!(serde_json::from_str::<SampleType>("\"Other\"").is_err());
        assert!(serde_json::from_value::<SampleType>(Value::String("Other".to_string())).is_err());
        assert!(serde_json::from_value::<SampleType>(Value::Null).is_err());
    }
}
