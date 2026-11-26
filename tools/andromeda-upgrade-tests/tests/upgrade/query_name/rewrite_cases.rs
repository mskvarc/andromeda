//! The paths v1.0.0 wrote, each beside the text the rewrite makes of it.

/// Each path text v1.0.0 wrote, beside the text the rewrite makes of it.
///
/// A double-quoted token is copied as it is, and so is everything where an
/// operand stands. A word holding a colon that follows a comma is the last
/// type of a type list glued to the name after it, and a word opening with a
/// colon follows a quoted last type. Each is copied through its first colon,
/// and the rest is quoted when it holds a colon. Every other word holding a
/// colon is a name the rewrite quotes.
pub(super) const REWRITES: [(&str, &str); 20] = [
    (r#""https://example.org/speed">50"#, r#""https://example.org/speed">50"#),
    ("urn:x:weight>1000", r#""urn:x:weight">1000"#),
    (
        r#"ex:speed==5;"https://example.org/a".urn:x:b>1"#,
        r#""ex:speed"==5;"https://example.org/a"."urn:x:b">1"#,
    ),
    (r#""https://example.org/ref"==urn:x:1,urn:x:2"#, r#""https://example.org/ref"==urn:x:1,urn:x:2"#),
    ("urn:x:t>=12:00:00Z", r#""urn:x:t">=12:00:00Z"#),
    (r#"urn:x:n~="^a:b;c$";urn:x:m==1"#, r#""urn:x:n"~="^a:b;c$";"urn:x:m"==1"#),
    (
        r#""https://example.org/rel"{"https://example.org/T1","https://example.org/T2":urn:x:b}==1"#,
        r#""https://example.org/rel"{"https://example.org/T1","https://example.org/T2":"urn:x:b"}==1"#,
    ),
    (r#""https://example.org/rel"{urn:x:b}==1"#, r#""https://example.org/rel"{"urn:x:b"}==1"#),
    ("sensor{ex:humidity}==40", r#"sensor{"ex:humidity"}==40"#),
    (r#"isParkedIn{Building,Store:category}=="x""#, r#"isParkedIn{Building,Store:category}=="x""#),
    (r#"rel{Building,Store:ex:category}=="x""#, r#"rel{Building,Store:"ex:category"}=="x""#),
    (r#"rel{Building,"ex:Store":ex:category}=="x""#, r#"rel{Building,"ex:Store":"ex:category"}=="x""#),
    (
        r#""https://example.org/address"[ex:city]=="Berlin""#,
        r#""https://example.org/address"["ex:city"]=="Berlin""#,
    ),
    (r#""https://example.org/label"[en-GB]=="x""#, r#""https://example.org/label"[en-GB]=="x""#),
    (r#"ex:a=="x:\"y;z"|ex:b==1"#, r#""ex:a"=="x:\"y;z"|"ex:b"==1"#),
    ("(ex:a>1|ex:b<2);ex:c", r#"("ex:a">1|"ex:b"<2);"ex:c""#),
    (
        r#""https://example.org/ref"==http://localhost/a?x=1;urn:x:b==2"#,
        r#""https://example.org/ref"==http://localhost/a?x=1;"urn:x:b"==2"#,
    ),
    ("ex:speed;desc", r#""ex:speed";desc"#),
    ("ex:a.observedAt", r#""ex:a".observedAt"#),
    ("urn:x:a", r#""urn:x:a""#),
];
