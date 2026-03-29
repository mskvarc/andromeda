#[cfg(test)]
mod tests {
    use crate::{
        parsing::Parser,
        types::{Number, TemporalValue, Value},
    };
    use chrono::Datelike;
    use compact_str::CompactString;
    use geojson::Value as GeoJsonValue;
    use indexmap::IndexMap;

    #[test]
    fn test_primitive_parsing() {
        assert_eq!(Parser::parse_boolean("true"), true);
        assert_eq!(Parser::parse_boolean("1"), true);
        assert_eq!(Parser::parse_boolean("yes"), true);
        assert_eq!(Parser::parse_boolean("on"), true);
        assert_eq!(Parser::parse_boolean("false"), false);

        assert_eq!(Parser::parse_integer("42"), Some(42));
        assert_eq!(Parser::parse_integer("\"42\""), Some(42));
        assert_eq!(Parser::parse_integer("'42'"), Some(42));
        assert_eq!(Parser::parse_integer("3.14"), Some(3));
        assert_eq!(Parser::parse_integer("3,14"), Some(3));

        assert_eq!(Parser::parse_float("3.14"), Some(3.14));
        assert_eq!(Parser::parse_float("\"3.14\""), Some(3.14));
        assert_eq!(Parser::parse_float("3,14"), Some(3.14));
    }

    #[test]
    fn test_value_conversions() {
        // Integer
        let v = Value::Number(Number::Integer(42));
        assert_eq!(v.to_integer(), 42);
        assert_eq!(v.to_float(), 42.0);
        assert_eq!(v.to_boolean(), true);
        assert_eq!(v.to_string(), "42");

        // Float
        let v = Value::Number(Number::Float(3.14));
        assert_eq!(v.to_integer(), 3);
        assert_eq!(v.to_float(), 3.14);
        assert_eq!(v.to_string(), "3.14");

        // String
        let s = Value::String(CompactString::from("true"));
        assert_eq!(s.to_boolean(), true);
        assert_eq!(s.to_string(), "true");

        // Null
        assert_eq!(Value::Null.to_integer(), 0);
        assert_eq!(Value::Null.to_float(), 0.0);
        assert_eq!(Value::Null.to_boolean(), false);
        assert_eq!(Value::Null.to_string(), "");
    }

    #[test]
    fn test_temporal_parsing() {
        // RFC3339
        let ts = "2024-03-13T12:00:00Z";
        let t = TemporalValue::try_parse(ts).unwrap();
        if let TemporalValue::DateTime(dt) = t {
            assert_eq!(dt.year(), 2024);
            assert_eq!(dt.month(), 3);
            assert_eq!(dt.day(), 13);
        } else {
            panic!("Expected DateTime");
        }

        // Heuristic - Seconds
        let t = TemporalValue::try_parse("1710331200").unwrap();
        if let TemporalValue::DateTime(dt) = t {
            assert_eq!(dt.year(), 2024);
        }

        // Heuristic - Millis
        let t = TemporalValue::try_parse("1710331200000").unwrap();
        if let TemporalValue::DateTime(dt) = t {
            assert_eq!(dt.year(), 2024);
        }
    }

    #[test]
    fn test_geospatial_conversions() {
        // Array to Point
        let v = Value::Array(vec![Value::Number(Number::Float(12.34)), Value::Number(Number::Float(56.78))]);
        let p = v.to_point();
        if let GeoJsonValue::Point(coords) = p.value {
            assert_eq!(coords, vec![12.34, 56.78]);
        } else {
            panic!("Expected Point");
        }

        // String JSON to Point
        let v = Value::String(CompactString::from(r#"{"type": "Point", "coordinates": [1.0, 2.0]}"#));
        let p = v.to_point();
        if let GeoJsonValue::Point(coords) = p.value {
            assert_eq!(coords, vec![1.0, 2.0]);
        }
    }

    #[test]
    fn test_object_handling() {
        let mut map = IndexMap::new();
        map.insert(CompactString::from("key"), Value::String(CompactString::from("value")));
        let v = Value::Object(Box::new(map));

        let obj = v.to_object();
        assert_eq!(obj.get("key").unwrap().as_str(), Some("value"));
    }

    #[test]
    fn test_display() {
        assert_eq!(format!("{}", Value::Boolean(true)), "true");
        assert_eq!(format!("{}", Value::Number(Number::Integer(100))), "100");
        assert_eq!(format!("{}", Value::String(CompactString::from("hello"))), "hello");
        assert_eq!(format!("{}", Value::Null), "");
    }
}
