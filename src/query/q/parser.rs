use crate::query::{
    error::QueryError,
    q::ast::{AttrPath, CmpOp, CompareValue, Expr, Predicate, Scalar},
};
use serde_json::Number;

/// Parses an NGSI-LD `q` filter expression string into an AST.
///
/// The grammar follows NGSI-LD 4.9, supporting boolean combinators (`;` for AND,
/// `|` for OR), comparison operators, range/list values, regex patterns, and
/// nested attribute paths.
///
/// Returns a `QueryError::BadRequestData` if the input is syntactically invalid.
pub fn parse(input: &str) -> Result<Expr, QueryError> {
    let tokens = tokenize(input)?;
    let mut parser = Parser::new(&tokens);
    let expr = parser.parse_or()?;
    if parser.pos < parser.tokens.len() {
        return Err(QueryError::BadRequestData(format!(
            "Unexpected token at position {}: {:?}",
            parser.pos, parser.tokens[parser.pos]
        )));
    }
    Ok(expr)
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Ident(String),
    QuotedStr(String),
    NumberLit(String),
    True,
    False,
    Eq,
    Neq,
    Gt,
    Ge,
    Lt,
    Le,
    Match,
    NotMatch,
    Dots,
    Semi,
    Pipe,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Dot,
    Comma,
}

fn tokenize(input: &str) -> Result<Vec<Token>, QueryError> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        match chars[i] {
            ' ' | '\t' | '\n' | '\r' => i += 1,
            ';' => {
                tokens.push(Token::Semi);
                i += 1;
            }
            '|' => {
                tokens.push(Token::Pipe);
                i += 1;
            }
            '(' => {
                tokens.push(Token::LParen);
                i += 1;
            }
            ')' => {
                tokens.push(Token::RParen);
                i += 1;
            }
            '[' => {
                tokens.push(Token::LBracket);
                i += 1;
            }
            ']' => {
                tokens.push(Token::RBracket);
                i += 1;
            }
            ',' => {
                tokens.push(Token::Comma);
                i += 1;
            }
            '=' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Eq);
                i += 2;
            }
            '!' if i + 2 < chars.len() && chars[i + 1] == '~' && chars[i + 2] == '=' => {
                tokens.push(Token::NotMatch);
                i += 3;
            }
            '!' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Neq);
                i += 2;
            }
            '~' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Match);
                i += 2;
            }
            '>' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Ge);
                i += 2;
            }
            '>' => {
                tokens.push(Token::Gt);
                i += 1;
            }
            '<' if i + 1 < chars.len() && chars[i + 1] == '=' => {
                tokens.push(Token::Le);
                i += 2;
            }
            '<' => {
                tokens.push(Token::Lt);
                i += 1;
            }
            '.' if i + 1 < chars.len() && chars[i + 1] == '.' => {
                tokens.push(Token::Dots);
                i += 2;
            }
            '.' => {
                tokens.push(Token::Dot);
                i += 1;
            }
            '"' => {
                i += 1;
                let mut s = String::new();
                while i < chars.len() && chars[i] != '"' {
                    if chars[i] == '\\' && i + 1 < chars.len() {
                        s.push(chars[i + 1]);
                        i += 2;
                    } else {
                        s.push(chars[i]);
                        i += 1;
                    }
                }
                if i >= chars.len() {
                    return Err(QueryError::BadRequestData("Unterminated string in q parameter".to_string()));
                }
                i += 1;
                let s = percent_encoding::percent_decode_str(&s).decode_utf8_lossy().into_owned();
                tokens.push(Token::QuotedStr(s));
            }
            c if c == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() => {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                if i < chars.len() && chars[i] == '.' && !(i + 1 < chars.len() && chars[i + 1] == '.') && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    i += 1;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                    i += 1;
                    if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                        i += 1;
                    }
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        i += 1;
                    }
                }
                let num_str: String = chars[start..i].iter().collect();
                tokens.push(Token::NumberLit(num_str));
            }
            c if c.is_ascii_digit() => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
                // Check if this looks like a datetime/date (digits followed by '-')
                // e.g. 2017-12-24T12:00:00Z
                if i < chars.len() && chars[i] == '-' && i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    // Looks like a date-like token, consume as identifier
                    while i < chars.len()
                        && (chars[i].is_alphanumeric()
                            || chars[i] == '-'
                            || chars[i] == ':'
                            || chars[i] == '.'
                            || chars[i] == '+'
                            || chars[i] == 'T'
                            || chars[i] == 'Z')
                    {
                        if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1] == '.' {
                            break;
                        }
                        i += 1;
                    }
                    let word: String = chars[start..i].iter().collect();
                    tokens.push(Token::Ident(word));
                } else {
                    if i < chars.len()
                        && chars[i] == '.'
                        && !(i + 1 < chars.len() && chars[i + 1] == '.')
                        && i + 1 < chars.len()
                        && chars[i + 1].is_ascii_digit()
                    {
                        i += 1;
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                        i += 1;
                        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                            i += 1;
                        }
                        while i < chars.len() && chars[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let num_str: String = chars[start..i].iter().collect();
                    tokens.push(Token::NumberLit(num_str));
                }
            }
            c if c.is_alphanumeric() || c == '_' || c == ':' || c == '/' => {
                let start = i;
                while i < chars.len()
                    && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == ':' || chars[i] == '/' || chars[i] == '-' || chars[i] == '#')
                {
                    if chars[i] == '.' && i + 1 < chars.len() && chars[i + 1] == '.' {
                        break;
                    }
                    if chars[i] == '.' {
                        break;
                    }
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match word.as_str() {
                    "true" => tokens.push(Token::True),
                    "false" => tokens.push(Token::False),
                    _ => tokens.push(Token::Ident(word)),
                }
            }
            other => {
                return Err(QueryError::BadRequestData(format!(
                    "Unexpected character '{}' in q parameter at position {}",
                    other, i
                )));
            }
        }
    }

    Ok(tokens)
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
}

impl<'a> Parser<'a> {
    const fn new(tokens: &'a [Token]) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn advance(&mut self) -> Option<&Token> {
        let tok = self.tokens.get(self.pos);
        if tok.is_some() {
            self.pos += 1;
        }
        tok
    }

    fn expect(&mut self, expected: &Token) -> Result<(), QueryError> {
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            Err(QueryError::BadRequestData(format!(
                "Expected {:?} at position {}, found {:?}",
                expected,
                self.pos,
                self.peek()
            )))
        }
    }

    fn parse_or(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_and()?;
        let mut terms = Vec::new();

        while self.peek() == Some(&Token::Pipe) {
            self.advance();
            terms.push(left);
            left = self.parse_and()?;
        }

        if terms.is_empty() {
            Ok(left)
        } else {
            terms.push(left);
            Ok(Expr::Or(terms))
        }
    }

    fn parse_and(&mut self) -> Result<Expr, QueryError> {
        let mut left = self.parse_primary()?;
        let mut terms = Vec::new();

        while self.peek() == Some(&Token::Semi) {
            self.advance();
            terms.push(left);
            left = self.parse_primary()?;
        }

        if terms.is_empty() {
            Ok(left)
        } else {
            terms.push(left);
            Ok(Expr::And(terms))
        }
    }

    fn parse_primary(&mut self) -> Result<Expr, QueryError> {
        if self.peek() == Some(&Token::LParen) {
            self.advance();
            let expr = self.parse_or()?;
            self.expect(&Token::RParen)?;
            Ok(expr)
        } else {
            Ok(Expr::Pred(self.parse_predicate()?))
        }
    }

    fn parse_predicate(&mut self) -> Result<Predicate, QueryError> {
        let path = self.parse_attr_path()?;

        match self.peek() {
            Some(Token::Eq) => {
                self.advance();
                let rhs = self.parse_compare_value()?;
                Ok(Predicate::Compare { path, op: CmpOp::Equal, rhs })
            }
            Some(Token::Neq) => {
                self.advance();
                let rhs = self.parse_compare_value()?;
                Ok(Predicate::Compare { path, op: CmpOp::Unequal, rhs })
            }
            Some(Token::Gt) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare { path, op: CmpOp::Greater, rhs })
            }
            Some(Token::Ge) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare {
                    path,
                    op: CmpOp::GreaterEq,
                    rhs,
                })
            }
            Some(Token::Lt) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare { path, op: CmpOp::Less, rhs })
            }
            Some(Token::Le) => {
                self.advance();
                let rhs = CompareValue::Scalar(self.parse_scalar()?);
                Ok(Predicate::Compare { path, op: CmpOp::LessEq, rhs })
            }
            Some(Token::Match) => {
                self.advance();
                let pattern = self.parse_pattern_string()?;
                Ok(Predicate::MatchPattern { path, pattern })
            }
            Some(Token::NotMatch) => {
                self.advance();
                let pattern = self.parse_pattern_string()?;
                Ok(Predicate::NotMatchPattern { path, pattern })
            }
            _ => Ok(Predicate::Exists(path)),
        }
    }

    fn parse_attr_path(&mut self) -> Result<AttrPath, QueryError> {
        let attr = self.parse_ident()?;
        let mut sub_attrs = Vec::new();
        let mut value_path = Vec::new();

        while self.peek() == Some(&Token::Dot) {
            self.advance();
            sub_attrs.push(self.parse_ident()?);
        }

        if self.peek() == Some(&Token::LBracket) {
            self.advance();
            value_path.push(self.parse_ident()?);
            while self.peek() == Some(&Token::Dot) {
                self.advance();
                value_path.push(self.parse_ident()?);
            }
            self.expect(&Token::RBracket)?;
        }

        Ok(AttrPath { attr, sub_attrs, value_path })
    }

    fn parse_ident(&mut self) -> Result<String, QueryError> {
        match self.advance() {
            Some(Token::Ident(s)) => Ok(s.clone()),
            Some(Token::QuotedStr(s)) => Ok(s.clone()),
            other => Err(QueryError::BadRequestData(format!("Expected identifier, found {:?}", other))),
        }
    }

    fn parse_compare_value(&mut self) -> Result<CompareValue, QueryError> {
        let first = self.parse_scalar()?;

        if self.peek() == Some(&Token::Dots) {
            self.advance();
            let second = self.parse_scalar()?;
            return Ok(CompareValue::Range { min: first, max: second });
        }

        if self.peek() == Some(&Token::Comma) {
            let mut values = vec![first];
            while self.peek() == Some(&Token::Comma) {
                self.advance();
                values.push(self.parse_scalar()?);
            }
            return Ok(CompareValue::ValueList(values));
        }

        Ok(CompareValue::Scalar(first))
    }

    fn parse_scalar(&mut self) -> Result<Scalar, QueryError> {
        match self.advance() {
            Some(Token::NumberLit(s)) => {
                let num: Number = s
                    .parse::<serde_json::Number>()
                    .map_err(|_| QueryError::BadRequestData(format!("Invalid number: {}", s)))?;
                Ok(Scalar::Number(num))
            }
            Some(Token::QuotedStr(s)) => {
                if is_datetime(s) {
                    Ok(Scalar::DateTime(s.clone()))
                } else if is_date(s) {
                    Ok(Scalar::Date(s.clone()))
                } else if is_time(s) {
                    Ok(Scalar::Time(s.clone()))
                } else {
                    Ok(Scalar::Str(s.clone()))
                }
            }
            Some(Token::Ident(s)) => {
                if is_datetime(s) {
                    Ok(Scalar::DateTime(s.clone()))
                } else if is_date(s) {
                    Ok(Scalar::Date(s.clone()))
                } else if is_time(s) {
                    Ok(Scalar::Time(s.clone()))
                } else {
                    Ok(Scalar::Str(s.clone()))
                }
            }
            Some(Token::True) => Ok(Scalar::Bool(true)),
            Some(Token::False) => Ok(Scalar::Bool(false)),
            other => Err(QueryError::BadRequestData(format!("Expected value, found {:?}", other))),
        }
    }

    fn parse_pattern_string(&mut self) -> Result<String, QueryError> {
        match self.advance() {
            Some(Token::QuotedStr(s)) => Ok(s.clone()),
            Some(Token::Ident(s)) => Ok(s.clone()),
            other => Err(QueryError::BadRequestData(format!("Expected pattern string, found {:?}", other))),
        }
    }
}

fn is_datetime(s: &str) -> bool {
    s.len() >= 19 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(10) == Some(&b'T')
}

fn is_date(s: &str) -> bool {
    s.len() == 10 && s.as_bytes().get(4) == Some(&b'-') && s.as_bytes().get(7) == Some(&b'-')
}

fn is_time(s: &str) -> bool {
    s.len() >= 8 && s.as_bytes().get(2) == Some(&b':') && s.as_bytes().get(5) == Some(&b':')
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]
    use super::*;

    #[test]
    fn parse_existence() {
        let expr = parse("temperature").unwrap();
        pretty_assertions::assert_eq!(expr, Expr::Pred(Predicate::Exists(AttrPath::simple("temperature"))));
    }

    #[test]
    fn parse_equality_number() {
        let expr = parse("temperature==20").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr, "temperature");
                pretty_assertions::assert_eq!(op, CmpOp::Equal);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Number(20.into())));
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_inequality_string() {
        let expr = parse("brandName!=\"Mercedes\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr, "brandName");
                pretty_assertions::assert_eq!(op, CmpOp::Unequal);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Str("Mercedes".to_string())));
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_greater_than() {
        let expr = parse("speed>50").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr, "speed");
                pretty_assertions::assert_eq!(op, CmpOp::Greater);
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Number(50.into())));
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_and_expression() {
        let expr = parse("speed>50;brandName!=\"Mercedes\"").unwrap();
        match expr {
            Expr::And(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            _ => unreachable!("expected And"),
        }
    }

    #[test]
    fn parse_or_expression() {
        let expr = parse("speed>50|temperature<10").unwrap();
        match expr {
            Expr::Or(terms) => pretty_assertions::assert_eq!(terms.len(), 2),
            _ => unreachable!("expected Or"),
        }
    }

    #[test]
    fn parse_and_takes_precedence_over_or() {
        let expr = parse("a==1|b==2;c==3").unwrap();
        match expr {
            Expr::Or(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[1] {
                    Expr::And(and_terms) => pretty_assertions::assert_eq!(and_terms.len(), 2),
                    _ => unreachable!("expected And as second OR term"),
                }
            }
            _ => unreachable!("expected Or"),
        }
    }

    #[test]
    fn parse_parenthesized_group() {
        let expr = parse("(speed>50|rpm>3000);brandName==\"Mercedes\"").unwrap();
        match expr {
            Expr::And(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[0] {
                    Expr::Or(_) => {}
                    _ => unreachable!("expected Or inside parens"),
                }
            }
            _ => unreachable!("expected And"),
        }
    }

    #[test]
    fn parse_range() {
        let expr = parse("temperature==10..20").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Range { min, max } => {
                    pretty_assertions::assert_eq!(min, Scalar::Number(10.into()));
                    pretty_assertions::assert_eq!(max, Scalar::Number(20.into()));
                }
                _ => unreachable!("expected Range"),
            },
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_value_list() {
        let expr = parse("color==\"red\",\"blue\",\"green\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::ValueList(vals) => {
                    pretty_assertions::assert_eq!(vals.len(), 3);
                }
                _ => unreachable!("expected ValueList"),
            },
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_match_pattern() {
        let expr = parse("brandName~=\"Merc.*\"").unwrap();
        match expr {
            Expr::Pred(Predicate::MatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.attr, "brandName");
                pretty_assertions::assert_eq!(pattern, "Merc.*");
            }
            _ => unreachable!("expected MatchPattern"),
        }
    }

    #[test]
    fn parse_not_match_pattern() {
        let expr = parse("brandName!~=\"Merc.*\"").unwrap();
        match expr {
            Expr::Pred(Predicate::NotMatchPattern { path, pattern }) => {
                pretty_assertions::assert_eq!(path.attr, "brandName");
                pretty_assertions::assert_eq!(pattern, "Merc.*");
            }
            _ => unreachable!("expected NotMatchPattern"),
        }
    }

    #[test]
    fn parse_dotted_attr_path() {
        let expr = parse("temperature.observedAt>=2017-12-24T12:00:00Z").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, op, rhs }) => {
                pretty_assertions::assert_eq!(path.attr, "temperature");
                pretty_assertions::assert_eq!(path.sub_attrs, vec!["observedAt"]);
                pretty_assertions::assert_eq!(op, CmpOp::GreaterEq);
                match rhs {
                    CompareValue::Scalar(Scalar::DateTime(dt)) => {
                        assert!(dt.starts_with("2017-12-24T"));
                    }
                    _ => unreachable!("expected DateTime scalar"),
                }
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_compound_value_path() {
        let expr = parse("address[city]==\"Berlin\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, .. }) => {
                pretty_assertions::assert_eq!(path.attr, "address");
                pretty_assertions::assert_eq!(path.value_path, vec!["city"]);
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_nested_compound_value_path() {
        let expr = parse("sensor.rawdata[airquality.particulate]==40").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { path, .. }) => {
                pretty_assertions::assert_eq!(path.attr, "sensor");
                pretty_assertions::assert_eq!(path.sub_attrs, vec!["rawdata"]);
                pretty_assertions::assert_eq!(path.value_path, vec!["airquality", "particulate"]);
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_less_equal() {
        let expr = parse("capacity<=10").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { op, .. }) => {
                pretty_assertions::assert_eq!(op, CmpOp::LessEq);
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_complex_mixed() {
        let expr = parse("(temperature>=20;temperature<=25)|capacity<=10").unwrap();
        match expr {
            Expr::Or(terms) => {
                pretty_assertions::assert_eq!(terms.len(), 2);
                match &terms[0] {
                    Expr::And(and_terms) => pretty_assertions::assert_eq!(and_terms.len(), 2),
                    _ => unreachable!("expected And"),
                }
            }
            _ => unreachable!("expected Or"),
        }
    }

    #[test]
    fn parse_uri_equality() {
        let expr = parse("isParked==\"urn:ngsi-ld:OffStreetParking:Downtown1\"").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Str(s)) => {
                    pretty_assertions::assert_eq!(s, "urn:ngsi-ld:OffStreetParking:Downtown1");
                }
                _ => unreachable!("expected Str"),
            },
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_negative_number() {
        let expr = parse("temperature>-5").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n.as_f64(), Some(-5.0));
                }
                _ => unreachable!("expected Number"),
            },
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_float_number() {
        let expr = parse("temperature==23.5").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => match rhs {
                CompareValue::Scalar(Scalar::Number(n)) => {
                    pretty_assertions::assert_eq!(n.as_f64(), Some(23.5));
                }
                _ => unreachable!("expected Number"),
            },
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn parse_boolean_value() {
        let expr = parse("active==true").unwrap();
        match expr {
            Expr::Pred(Predicate::Compare { rhs, .. }) => {
                pretty_assertions::assert_eq!(rhs, CompareValue::Scalar(Scalar::Bool(true)));
            }
            _ => unreachable!("expected Compare"),
        }
    }

    #[test]
    fn error_on_unterminated_string() {
        let result = parse("name==\"hello");
        assert!(result.is_err());
    }

    #[test]
    fn error_on_unexpected_token() {
        let result = parse("==");
        assert!(result.is_err());
    }
}
