//! Localization renderer for i18n proxy endpoints (NGSI-LD Annex G.3).
//!
//! Transforms entity attribute values into locale-specific display strings.
//! Handles datetime formatting and currency formatting per locale conventions.
//! This is a presentation-layer concern: the domain model is unchanged, only
//! the wire representation is adapted for human consumption.

use chrono::{DateTime, Utc};
use serde_json::Value;
use std::collections::HashSet;

/// Localize selected attributes of an entity to display strings.
///
/// For each attribute in `localized_attrs` (or all non-structural attributes
/// if `None`), attempts to convert datetime and currency values to
/// locale-appropriate display strings.  Unrecognized or non-localizable
/// attributes are left unchanged.
pub fn localize_entity_value(mut value: Value, locale: &str, localized_attrs: Option<&HashSet<String>>) -> Value {
    let Some(object) = value.as_object_mut() else {
        return value;
    };

    let keys: Vec<String> = object.keys().cloned().collect();
    for key in keys {
        if matches!(key.as_str(), "id" | "type" | "@context") {
            continue;
        }

        if let Some(filter) = localized_attrs
            && !filter.contains(key.as_str())
        {
            continue;
        }

        if let Some(attr_value) = object.get_mut(&key)
            && let Some(localized) = localize_attribute_value(attr_value, locale)
        {
            *attr_value = localized;
        }
    }

    value
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

fn localize_attribute_value(value: &Value, locale: &str) -> Option<Value> {
    let object = value.as_object()?;
    if object.get("type").and_then(Value::as_str) != Some("Property") {
        return None;
    }

    if let Some(datetime) = object.get("value").and_then(Value::as_str).and_then(parse_datetime) {
        return Some(Value::String(format_datetime_display(datetime, locale)));
    }

    if let Some(amount) = object.get("value").and_then(Value::as_f64)
        && let Some(unit_code) = object.get("unitCode").and_then(Value::as_str)
        && is_currency_code(unit_code)
    {
        return Some(Value::String(format_currency_display(amount, unit_code, locale)));
    }

    None
}

fn parse_datetime(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value).ok().map(|parsed| parsed.with_timezone(&Utc))
}

fn format_datetime_display(datetime: DateTime<Utc>, locale: &str) -> String {
    if locale.contains("u-ca-islamic") {
        return format!("Islamic {}", datetime.format("%Y-%m-%d %H:%M:%S UTC"));
    }

    let language = locale.split('-').next().unwrap_or(locale);
    match language {
        "de" => datetime.format("%d.%m.%Y %H:%M:%S UTC").to_string(),
        _ => datetime.format("%Y-%m-%d %I:%M:%S %p UTC").to_string(),
    }
}

fn format_currency_display(amount: f64, unit_code: &str, locale: &str) -> String {
    let language = locale.split('-').next().unwrap_or(locale);
    let (thousands, decimal, symbol_after) = match language {
        "de" => ('.', ',', true),
        _ => (',', '.', false),
    };

    let number = format_number(amount, thousands, decimal);
    if symbol_after {
        format!("{} {}", number, unit_code)
    } else {
        format!("{} {}", unit_code, number)
    }
}

fn format_number(amount: f64, thousands: char, decimal: char) -> String {
    let rounded = format!("{:.2}", amount.abs());
    let mut split = rounded.split('.');
    let integer = split.next().unwrap_or("0");
    let fraction = split.next().unwrap_or("00");

    let mut reversed = String::new();
    for (index, character) in integer.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            reversed.push(thousands);
        }
        reversed.push(character);
    }

    let sign = if amount.is_sign_negative() { "-" } else { "" };
    let integer_grouped: String = reversed.chars().rev().collect();
    format!("{sign}{integer_grouped}{decimal}{fraction}")
}

fn is_currency_code(value: &str) -> bool {
    value.len() == 3 && value.chars().all(|character| character.is_ascii_alphabetic())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn localize_attribute_value_datetime_de() {
        let attr = json!({"type": "Property", "value": "2020-09-28T17:13:39Z"});
        let result = localize_attribute_value(&attr, "de-DE");
        assert_eq!(result, Some(Value::String("28.09.2020 17:13:39 UTC".to_string())));
    }

    #[test]
    fn localize_attribute_value_datetime_en() {
        let attr = json!({"type": "Property", "value": "2020-09-28T17:13:39Z"});
        let result = localize_attribute_value(&attr, "en-US");
        assert_eq!(result, Some(Value::String("2020-09-28 05:13:39 PM UTC".to_string())));
    }

    #[test]
    fn localize_attribute_value_currency_de() {
        let attr = json!({"type": "Property", "value": 1234.5, "unitCode": "EUR"});
        let result = localize_attribute_value(&attr, "de-DE");
        assert_eq!(result, Some(Value::String("1.234,50 EUR".to_string())));
    }

    #[test]
    fn localize_attribute_value_currency_en() {
        let attr = json!({"type": "Property", "value": 1234.5, "unitCode": "USD"});
        let result = localize_attribute_value(&attr, "en-US");
        assert_eq!(result, Some(Value::String("USD 1,234.50".to_string())));
    }

    #[test]
    fn localize_attribute_value_skips_relationships() {
        let attr = json!({"type": "Relationship", "object": "urn:ngsi-ld:Entity:1"});
        let result = localize_attribute_value(&attr, "en-US");
        assert_eq!(result, None);
    }

    #[test]
    fn localize_entity_value_filters_by_localized_attrs() {
        let entity = json!({
            "id": "urn:test:1",
            "type": "Offer",
            "date": {"type": "Property", "value": "2020-09-28T17:13:39Z"},
            "name": {"type": "Property", "value": "test"}
        });
        let filter: HashSet<String> = ["date".to_string()].into_iter().collect();
        let result = localize_entity_value(entity, "de-DE", Some(&filter));
        assert_eq!(result["date"], "28.09.2020 17:13:39 UTC");
        // name was not in filter, so it should remain unchanged
        assert_eq!(result["name"]["value"], "test");
    }

    #[test]
    fn localize_entity_value_preserves_structural_keys() {
        let entity = json!({
            "id": "urn:test:1",
            "type": "Offer",
            "@context": "https://example.com"
        });
        let result = localize_entity_value(entity.clone(), "en-US", None);
        assert_eq!(result["id"], "urn:test:1");
        assert_eq!(result["type"], "Offer");
        assert_eq!(result["@context"], "https://example.com");
    }

    #[test]
    fn localize_attribute_value_datetime_islamic_calendar() {
        let attr = json!({"type": "Property", "value": "2020-09-28T17:13:39Z"});
        let result = localize_attribute_value(&attr, "ar-SA-u-ca-islamic");
        assert!(result.is_some());
        let text = result.unwrap();
        assert!(text.as_str().unwrap().starts_with("Islamic "));
    }

    #[test]
    fn format_number_negative() {
        let result = format_number(-1234.56, ',', '.');
        assert_eq!(result, "-1,234.56");
    }

    #[test]
    fn format_number_small() {
        let result = format_number(5.0, ',', '.');
        assert_eq!(result, "5.00");
    }
}
