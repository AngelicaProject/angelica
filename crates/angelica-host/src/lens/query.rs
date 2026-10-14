//! The text of a Lens query, parsed into its parts. Names are checked
//! against the schema later, by the planner; here only the syntax.
//!
//! ```text
//! inventory.containers[kind in (bag_1, bag_2) and size > 0].items[item != 0] { item, quantity }
//! ```
#![forbid(unsafe_code)]

/// The longest query text.
pub const MOST_BYTES: usize = 4096;

/// A parsed query: a root, a path of fields from it, and the fields to
/// answer with.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    pub root: String,
    pub path: Vec<Segment>,
    /// `None` when the path ends in the one value to answer with.
    pub select: Option<Vec<Selected>>,
}

/// One field of the path, with what selects among its objects.
#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    pub field: String,
    pub filter: Option<Filter>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Filter {
    /// One element of an array, by position from 0.
    Index(u32),
    /// The objects for which every condition holds.
    Conditions(Vec<Condition>),
}

/// A condition on a value of the object, reached by `field`: a field name
/// or dotted names through references and embedded objects.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub field: Vec<String>,
    pub test: Test,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Test {
    Compare(Op, Literal),
    /// Equal to one of these.
    In(Vec<Literal>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Literal {
    Integer(i128),
    Float(f64),
    Boolean(bool),
    /// A name of an enum's value.
    Name(String),
    Text(String),
}

/// A value to answer with: `path` from the object, answered as `name`
/// (`alias: path`, or the names of the path joined by `_`, such as
/// `data_level` for `data.level`).
#[derive(Debug, Clone, PartialEq)]
pub struct Selected {
    pub name: String,
    pub path: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Word(String),
    Integer(i128),
    Float(f64),
    Text(String),
    Symbol(&'static str),
}

/// Splits `text` into tokens, each with the byte where it starts.
fn tokens(text: &str) -> Result<Vec<(usize, Token)>, String> {
    const SYMBOLS: [&str; 15] = [
        "!=", "<=", ">=", ".", "[", "]", "{", "}", "(", ")", ",", ":", "=", "<", ">",
    ];
    let bytes = text.as_bytes();
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let byte = bytes[at];
        if byte.is_ascii_whitespace() {
            at += 1;
            continue;
        }
        let start = at;
        if byte.is_ascii_alphabetic() || byte == b'_' {
            while at < bytes.len() && (bytes[at].is_ascii_alphanumeric() || bytes[at] == b'_') {
                at += 1;
            }
            out.push((start, Token::Word(text[start..at].to_owned())));
        } else if byte.is_ascii_digit()
            || (byte == b'-' && bytes.get(at + 1).is_some_and(u8::is_ascii_digit))
        {
            at += 1;
            let hex = byte == b'0' && matches!(bytes.get(at), Some(b'x' | b'X'));
            if hex {
                at += 1;
            }
            // A dot belongs to the number only before a digit: `[0].items`.
            while at < bytes.len()
                && ((hex && bytes[at].is_ascii_hexdigit())
                    || bytes[at].is_ascii_digit()
                    || bytes[at] == b'_'
                    || (!hex
                        && bytes[at] == b'.'
                        && bytes.get(at + 1).is_some_and(u8::is_ascii_digit)))
            {
                at += 1;
            }
            let written = text[start..at].replace('_', "");
            let token = if hex {
                i128::from_str_radix(&written[2..], 16)
                    .map(Token::Integer)
                    .map_err(|_| format!("at byte {start}: `{written}` is not a number"))?
            } else if written.contains('.') {
                written
                    .parse::<f64>()
                    .ok()
                    .filter(|value| value.is_finite())
                    .map(Token::Float)
                    .ok_or_else(|| format!("at byte {start}: `{written}` is not a number"))?
            } else {
                written
                    .parse::<i128>()
                    .map(Token::Integer)
                    .map_err(|_| format!("at byte {start}: `{written}` is not a number"))?
            };
            out.push((start, token));
        } else if byte == b'"' {
            at += 1;
            let mut value = String::new();
            loop {
                match text[at..].chars().next() {
                    None => return Err(format!("at byte {start}: the text is not closed")),
                    Some('"') => {
                        at += 1;
                        break;
                    }
                    Some('\\') => {
                        match text[at + 1..].chars().next() {
                            Some(escaped @ ('"' | '\\')) => value.push(escaped),
                            _ => {
                                return Err(format!(
                                    "at byte {at}: only \\\" and \\\\ are escapes in text"
                                ));
                            }
                        }
                        at += 2;
                    }
                    Some(character) => {
                        value.push(character);
                        at += character.len_utf8();
                    }
                }
            }
            out.push((start, Token::Text(value)));
        } else if let Some(symbol) = SYMBOLS
            .iter()
            .find(|symbol| text[at..].starts_with(**symbol))
        {
            at += symbol.len();
            out.push((start, Token::Symbol(symbol)));
        } else {
            let character = text[at..].chars().next().unwrap_or('?');
            return Err(format!(
                "at byte {start}: `{character}` does not belong in a query"
            ));
        }
    }
    Ok(out)
}

struct Parser {
    tokens: Vec<(usize, Token)>,
    next: usize,
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.next).map(|(_, token)| token)
    }

    fn at(&self) -> usize {
        self.tokens.get(self.next).map_or(self.end, |(at, _)| *at)
    }

    fn fail<T>(&self, expected: &str) -> Result<T, String> {
        let found = match self.peek() {
            None => "the end".to_owned(),
            Some(Token::Word(word)) => format!("`{word}`"),
            Some(Token::Integer(value)) => format!("`{value}`"),
            Some(Token::Float(value)) => format!("`{value}`"),
            Some(Token::Text(text)) => format!("\"{text}\""),
            Some(Token::Symbol(symbol)) => format!("`{symbol}`"),
        };
        Err(format!(
            "at byte {}: expected {expected}, found {found}",
            self.at()
        ))
    }

    fn symbol(&mut self, symbol: &str) -> bool {
        if self.peek() == Some(&Token::Symbol(symbol_static(symbol))) {
            self.next += 1;
            true
        } else {
            false
        }
    }

    fn expect(&mut self, symbol: &str) -> Result<(), String> {
        if self.symbol(symbol) {
            Ok(())
        } else {
            self.fail(&format!("`{symbol}`"))
        }
    }

    fn keyword(&mut self, word: &str) -> bool {
        if matches!(self.peek(), Some(Token::Word(found)) if found == word) {
            self.next += 1;
            true
        } else {
            false
        }
    }

    /// A field or root name: a lowercase letter, then lowercase letters,
    /// digits, and underscores.
    fn name(&mut self, what: &str) -> Result<String, String> {
        match self.peek() {
            Some(Token::Word(word))
                if word.starts_with(|c: char| c.is_ascii_lowercase())
                    && word
                        .chars()
                        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                    && !matches!(word.as_str(), "in" | "and" | "true" | "false") =>
            {
                let word = word.clone();
                self.next += 1;
                Ok(word)
            }
            _ => self.fail(what),
        }
    }

    fn dotted(&mut self, what: &str) -> Result<Vec<String>, String> {
        let mut names = vec![self.name(what)?];
        while self.symbol(".") {
            names.push(self.name("a field name")?);
        }
        Ok(names)
    }

    fn literal(&mut self) -> Result<Literal, String> {
        let literal = match self.peek() {
            Some(Token::Integer(value)) => Literal::Integer(*value),
            Some(Token::Float(value)) => Literal::Float(*value),
            Some(Token::Text(text)) => Literal::Text(text.clone()),
            Some(Token::Word(word)) if word == "true" => Literal::Boolean(true),
            Some(Token::Word(word)) if word == "false" => Literal::Boolean(false),
            Some(Token::Word(_)) => return self.name("a value").map(Literal::Name),
            _ => return self.fail("a value"),
        };
        self.next += 1;
        Ok(literal)
    }

    fn condition(&mut self) -> Result<Condition, String> {
        let field = self.dotted("a field name")?;
        if self.keyword("in") {
            self.expect("(")?;
            let mut values = vec![self.literal()?];
            while self.symbol(",") {
                values.push(self.literal()?);
            }
            self.expect(")")?;
            return Ok(Condition {
                field,
                test: Test::In(values),
            });
        }
        let op = [
            ("!=", Op::NotEqual),
            ("<=", Op::LessOrEqual),
            (">=", Op::GreaterOrEqual),
            ("=", Op::Equal),
            ("<", Op::Less),
            (">", Op::Greater),
        ]
        .into_iter()
        .find(|(symbol, _)| self.symbol(symbol))
        .map(|(_, op)| op);
        let Some(op) = op else {
            return self.fail("a comparison: =, !=, <, <=, >, >=, or in");
        };
        Ok(Condition {
            field,
            test: Test::Compare(op, self.literal()?),
        })
    }

    fn filter(&mut self) -> Result<Filter, String> {
        if let Some(Token::Integer(index)) = self.peek() {
            let index = u32::try_from(*index)
                .map_err(|_| format!("at byte {}: an index is 0 or more", self.at()))?;
            self.next += 1;
            self.expect("]")?;
            return Ok(Filter::Index(index));
        }
        let mut conditions = vec![self.condition()?];
        while self.keyword("and") {
            conditions.push(self.condition()?);
        }
        self.expect("]")?;
        Ok(Filter::Conditions(conditions))
    }

    fn selected(&mut self) -> Result<Selected, String> {
        let first = self.dotted("a field name")?;
        if first.len() == 1 && self.symbol(":") {
            return Ok(Selected {
                name: first.into_iter().next().unwrap_or_default(),
                path: self.dotted("a field name")?,
            });
        }
        Ok(Selected {
            name: first.join("_"),
            path: first,
        })
    }
}

/// `"!="` and the others as the `'static` strings tokens hold.
fn symbol_static(symbol: &str) -> &'static str {
    match symbol {
        "!=" => "!=",
        "<=" => "<=",
        ">=" => ">=",
        "." => ".",
        "[" => "[",
        "]" => "]",
        "{" => "{",
        "}" => "}",
        "(" => "(",
        ")" => ")",
        "," => ",",
        ":" => ":",
        "=" => "=",
        "<" => "<",
        _ => ">",
    }
}

/// Parses a query.
///
/// # Errors
/// A message that names the byte where the query breaks the syntax.
pub fn parse(text: &str) -> Result<Query, String> {
    if text.len() > MOST_BYTES {
        return Err(format!("a query is at most {MOST_BYTES} bytes"));
    }
    let mut parser = Parser {
        tokens: tokens(text)?,
        next: 0,
        end: text.len(),
    };
    let root = parser.name("a root name")?;
    let mut path = Vec::new();
    loop {
        if parser.symbol(".") {
            path.push(Segment {
                field: parser.name("a field name")?,
                filter: None,
            });
        } else if parser.symbol("[") {
            let filter = parser.filter()?;
            match path.last_mut() {
                Some(segment) if segment.filter.is_none() => segment.filter = Some(filter),
                Some(_) => return Err("a field has at most one filter".into()),
                None => return Err("a filter follows a field of the root, not the root".into()),
            }
        } else {
            break;
        }
    }
    let select = if parser.symbol("{") {
        let mut fields = vec![parser.selected()?];
        while parser.symbol(",") {
            if parser.peek() == Some(&Token::Symbol("}")) {
                break;
            }
            fields.push(parser.selected()?);
        }
        parser.expect("}")?;
        let mut names = std::collections::BTreeSet::new();
        for field in &fields {
            if !names.insert(field.name.as_str()) {
                return Err(format!("`{}` is answered twice", field.name));
            }
        }
        Some(fields)
    } else {
        None
    };
    if parser.peek().is_some() {
        return parser.fail("the end of the query");
    }
    Ok(Query { root, path, select })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_has_a_path_filters_and_a_selection() {
        let query = parse(
            "inventory.containers[kind in (bag_1, bag_2) and size > 0].items[item != 0x10] { item, count: quantity, data.level }",
        )
        .unwrap();
        assert_eq!(query.root, "inventory");
        assert_eq!(query.path.len(), 2);
        assert_eq!(
            query.path[0].filter,
            Some(Filter::Conditions(vec![
                Condition {
                    field: vec!["kind".into()],
                    test: Test::In(vec![
                        Literal::Name("bag_1".into()),
                        Literal::Name("bag_2".into())
                    ]),
                },
                Condition {
                    field: vec!["size".into()],
                    test: Test::Compare(Op::Greater, Literal::Integer(0)),
                },
            ]))
        );
        assert_eq!(
            query.path[1].filter,
            Some(Filter::Conditions(vec![Condition {
                field: vec!["item".into()],
                test: Test::Compare(Op::NotEqual, Literal::Integer(16)),
            }]))
        );
        let select = query.select.unwrap();
        assert_eq!(select[1].name, "count");
        assert_eq!(select[1].path, ["quantity"]);
        assert_eq!(select[2].name, "data_level");
        assert_eq!(select[2].path, ["data", "level"]);
    }

    #[test]
    fn values_and_indexes() {
        let query =
            parse("a.b[3].c[x = -1.5 and y = \"Al\\\"isae\" and z = true and w <= -2]").unwrap();
        assert_eq!(query.path[0].filter, Some(Filter::Index(3)));
        let Some(Filter::Conditions(conditions)) = &query.path[1].filter else {
            panic!()
        };
        assert_eq!(
            conditions
                .iter()
                .map(|condition| condition.test.clone())
                .collect::<Vec<_>>(),
            [
                Test::Compare(Op::Equal, Literal::Float(-1.5)),
                Test::Compare(Op::Equal, Literal::Text("Al\"isae".into())),
                Test::Compare(Op::Equal, Literal::Boolean(true)),
                Test::Compare(Op::LessOrEqual, Literal::Integer(-2)),
            ]
        );
        assert!(query.select.is_none());
    }

    #[test]
    fn mistakes_name_where_they_are() {
        for (text, message) in [
            ("", "at byte 0: expected a root name, found the end"),
            (
                "inventory.",
                "at byte 10: expected a field name, found the end",
            ),
            (
                "inventory[0]",
                "a filter follows a field of the root, not the root",
            ),
            ("a.b[0][1]", "a field has at most one filter"),
            ("a.b[x ~ 1]", "at byte 6: `~` does not belong in a query"),
            (
                "a.b[x 1]",
                "at byte 6: expected a comparison: =, !=, <, <=, >, >=, or in, found `1`",
            ),
            ("a.b { x, x }", "`x` is answered twice"),
            (
                "a.b { x } c",
                "at byte 10: expected the end of the query, found `c`",
            ),
            ("a.B", "at byte 2: expected a field name, found `B`"),
            ("a.b[x = \"open", "at byte 8: the text is not closed"),
            ("a.b[-1]", "at byte 4: an index is 0 or more"),
        ] {
            assert_eq!(parse(text).unwrap_err(), message, "{text}");
        }
        assert!(parse(&"a".repeat(MOST_BYTES + 1)).is_err());
    }

    #[test]
    fn damaged_queries_are_refused_without_panicking() {
        let text = "inventory.containers[kind in (bag_1, 2) and x != \"a\"].items[0] { a: b.c, d }";
        let mut mutator = crate::mutations::Mutator::new(29);
        for _ in 0..20_000 {
            let _ = parse(&mutator.mutate_text(text));
        }
    }
}
