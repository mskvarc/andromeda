//! The server-side character sets a PostgreSQL database can be created with.
//!
//! `SHOW server_encoding` answers with one of these names. The set is closed:
//! PostgreSQL refuses to create a database in an encoding it does not list as
//! server-capable, so a name outside it is a server this broker cannot reason
//! about, not a new encoding.

use strum::{Display, EnumString};

/// A PostgreSQL server-side character set, spelled as the server reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Display, EnumString)]
pub enum ServerEncoding {
    /// Extended UNIX Code-CN, simplified Chinese.
    #[strum(serialize = "EUC_CN")]
    EucCn,
    /// Extended UNIX Code-JP, Japanese.
    #[strum(serialize = "EUC_JP")]
    EucJp,
    /// Extended UNIX Code-JP, JIS X 0213.
    #[strum(serialize = "EUC_JIS_2004")]
    EucJis2004,
    /// Extended UNIX Code-KR, Korean.
    #[strum(serialize = "EUC_KR")]
    EucKr,
    /// Extended UNIX Code-TW, traditional Chinese.
    #[strum(serialize = "EUC_TW")]
    EucTw,
    /// ISO 8859-5, Latin/Cyrillic.
    #[strum(serialize = "ISO_8859_5")]
    Iso8859Cyrillic,
    /// ISO 8859-6, Latin/Arabic.
    #[strum(serialize = "ISO_8859_6")]
    Iso8859Arabic,
    /// ISO 8859-7, Latin/Greek.
    #[strum(serialize = "ISO_8859_7")]
    Iso8859Greek,
    /// ISO 8859-8, Latin/Hebrew.
    #[strum(serialize = "ISO_8859_8")]
    Iso8859Hebrew,
    /// KOI8-R, Russian Cyrillic.
    #[strum(serialize = "KOI8R")]
    Koi8R,
    /// KOI8-U, Ukrainian Cyrillic.
    #[strum(serialize = "KOI8U")]
    Koi8U,
    /// ISO 8859-1, Western European.
    #[strum(serialize = "LATIN1")]
    Latin1,
    /// ISO 8859-2, Central European.
    #[strum(serialize = "LATIN2")]
    Latin2,
    /// ISO 8859-3, South European.
    #[strum(serialize = "LATIN3")]
    Latin3,
    /// ISO 8859-4, North European.
    #[strum(serialize = "LATIN4")]
    Latin4,
    /// ISO 8859-9, Turkish.
    #[strum(serialize = "LATIN5")]
    Latin5,
    /// ISO 8859-10, Nordic.
    #[strum(serialize = "LATIN6")]
    Latin6,
    /// ISO 8859-13, Baltic.
    #[strum(serialize = "LATIN7")]
    Latin7,
    /// ISO 8859-14, Celtic.
    #[strum(serialize = "LATIN8")]
    Latin8,
    /// ISO 8859-15, Latin-1 with the euro sign.
    #[strum(serialize = "LATIN9")]
    Latin9,
    /// ISO 8859-16, Romanian.
    #[strum(serialize = "LATIN10")]
    Latin10,
    /// The Mule internal code of multilingual Emacs.
    #[strum(serialize = "MULE_INTERNAL")]
    MuleInternal,
    /// No declared encoding: bytes above 127 are stored uninterpreted.
    #[strum(serialize = "SQL_ASCII")]
    SqlAscii,
    /// Unicode, 8-bit.
    #[strum(serialize = "UTF8")]
    Utf8,
    /// Windows code page 866, Cyrillic.
    #[strum(serialize = "WIN866")]
    Win866,
    /// Windows code page 874, Thai.
    #[strum(serialize = "WIN874")]
    Win874,
    /// Windows code page 1250, Central European.
    #[strum(serialize = "WIN1250")]
    Win1250,
    /// Windows code page 1251, Cyrillic.
    #[strum(serialize = "WIN1251")]
    Win1251,
    /// Windows code page 1252, Western European.
    #[strum(serialize = "WIN1252")]
    Win1252,
    /// Windows code page 1253, Greek.
    #[strum(serialize = "WIN1253")]
    Win1253,
    /// Windows code page 1254, Turkish.
    #[strum(serialize = "WIN1254")]
    Win1254,
    /// Windows code page 1255, Hebrew.
    #[strum(serialize = "WIN1255")]
    Win1255,
    /// Windows code page 1256, Arabic.
    #[strum(serialize = "WIN1256")]
    Win1256,
    /// Windows code page 1257, Baltic.
    #[strum(serialize = "WIN1257")]
    Win1257,
    /// Windows code page 1258, Vietnamese.
    #[strum(serialize = "WIN1258")]
    Win1258,
}

#[cfg(test)]
mod tests {
    use crate::schema_contract::server_encoding::ServerEncoding;
    use std::str::FromStr;

    #[test]
    fn the_encoding_names_the_server_reports_parse_to_their_character_sets() {
        pretty_assertions::assert_eq!(ServerEncoding::from_str("UTF8"), Ok(ServerEncoding::Utf8));
        pretty_assertions::assert_eq!(ServerEncoding::from_str("SQL_ASCII"), Ok(ServerEncoding::SqlAscii));
        pretty_assertions::assert_eq!(ServerEncoding::from_str("ISO_8859_5"), Ok(ServerEncoding::Iso8859Cyrillic));
        pretty_assertions::assert_eq!(ServerEncoding::from_str("EUC_JIS_2004"), Ok(ServerEncoding::EucJis2004));
    }

    #[test]
    fn an_encoding_renders_as_the_name_the_server_reports() {
        pretty_assertions::assert_eq!(ServerEncoding::Latin1.to_string(), "LATIN1");
        pretty_assertions::assert_eq!(ServerEncoding::Win1252.to_string(), "WIN1252");
    }

    #[test]
    fn a_client_only_or_unknown_encoding_name_is_not_a_server_encoding() {
        assert!(ServerEncoding::from_str("SJIS").is_err());
        assert!(ServerEncoding::from_str("utf8").is_err());
    }
}
