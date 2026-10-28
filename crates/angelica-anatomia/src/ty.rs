//! Type expressions: how a field, a global, or a function's argument is
//! typed (`u32`, `ptr<object.character>`, `char8[64]`, `fn(ptr) -> u8`).

use std::fmt;

/// A value of the Windows x64 calling convention that fits a register.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Primitive {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    F32,
    F64,
    Bool,
}

impl Primitive {
    /// Every primitive by its name in the format.
    pub const ALL: [(&'static str, Self); 11] = [
        ("i8", Self::I8),
        ("i16", Self::I16),
        ("i32", Self::I32),
        ("i64", Self::I64),
        ("u8", Self::U8),
        ("u16", Self::U16),
        ("u32", Self::U32),
        ("u64", Self::U64),
        ("f32", Self::F32),
        ("f64", Self::F64),
        ("bool", Self::Bool),
    ];

    #[must_use]
    pub fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(_, primitive)| *primitive == self)
            .map_or("?", |(name, _)| name)
    }

    /// The size in bytes.
    #[must_use]
    pub fn size(self) -> u32 {
        match self {
            Self::I8 | Self::U8 | Self::Bool => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::I64 | Self::U64 | Self::F64 => 8,
        }
    }

    /// Whether the value is an integer, `bool` excluded.
    #[must_use]
    pub fn is_integer(self) -> bool {
        !matches!(self, Self::F32 | Self::F64 | Self::Bool)
    }
}

/// One argument of a function type, optionally named.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Param {
    pub name: Option<String>,
    pub ty: Ty,
}

/// A function of the Windows x64 calling convention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FnType {
    pub params: Vec<Param>,
    /// `None` for a function that returns nothing.
    pub ret: Option<Box<Ty>>,
}

/// A type expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ty {
    Primitive(Primitive),
    Char8,
    Char16,
    /// A pointer to a `T`, or to unknown memory.
    Ptr(Option<Box<Ty>>),
    /// `n` elements in place.
    Array(Box<Ty>, u32),
    Fn(FnType),
    /// A structure, union, enum, alias, or template parameter, by name,
    /// with the arguments of a template.
    Named(String, Vec<Ty>),
    /// An object of a type the description does not give, in place: its
    /// address is all that is known.
    Opaque,
}

impl Ty {
    /// Every name of a type this expression refers to, template parameters
    /// included.
    pub fn names(&self, names: &mut Vec<String>) {
        match self {
            Self::Ptr(Some(inner)) | Self::Array(inner, _) => inner.names(names),
            Self::Fn(function) => {
                for param in &function.params {
                    param.ty.names(names);
                }
                if let Some(ret) = &function.ret {
                    ret.names(names);
                }
            }
            Self::Named(name, args) => {
                names.push(name.clone());
                for arg in args {
                    arg.names(names);
                }
            }
            Self::Primitive(_) | Self::Char8 | Self::Char16 | Self::Ptr(None) | Self::Opaque => {}
        }
    }

    /// Parses a type expression.
    ///
    /// # Errors
    ///
    /// Says where the text stops being a type expression.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut parser = Parser { text, position: 0 };
        let ty = parser.ty()?;
        parser.space();
        if parser.position != text.len() {
            return Err(format!(
                "`{text}` is not a type: unexpected `{}`",
                &text[parser.position..]
            ));
        }
        Ok(ty)
    }
}

impl fmt::Display for Ty {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Primitive(primitive) => formatter.write_str(primitive.name()),
            Self::Char8 => formatter.write_str("char8"),
            Self::Char16 => formatter.write_str("char16"),
            Self::Ptr(None) => formatter.write_str("ptr"),
            Self::Ptr(Some(inner)) => write!(formatter, "ptr<{inner}>"),
            Self::Array(inner, length) => write!(formatter, "{inner}[{length}]"),
            Self::Fn(function) => {
                formatter.write_str("fn(")?;
                for (index, param) in function.params.iter().enumerate() {
                    if index > 0 {
                        formatter.write_str(", ")?;
                    }
                    if let Some(name) = &param.name {
                        write!(formatter, "{name}: ")?;
                    }
                    write!(formatter, "{}", param.ty)?;
                }
                formatter.write_str(")")?;
                if let Some(ret) = &function.ret {
                    write!(formatter, " -> {ret}")?;
                }
                Ok(())
            }
            Self::Named(name, args) => {
                formatter.write_str(name)?;
                if !args.is_empty() {
                    formatter.write_str("(")?;
                    for (index, arg) in args.iter().enumerate() {
                        if index > 0 {
                            formatter.write_str(", ")?;
                        }
                        write!(formatter, "{arg}")?;
                    }
                    formatter.write_str(")")?;
                }
                Ok(())
            }
            Self::Opaque => formatter.write_str("opaque"),
        }
    }
}

struct Parser<'a> {
    text: &'a str,
    position: usize,
}

impl Parser<'_> {
    fn rest(&self) -> &str {
        &self.text[self.position..]
    }

    fn space(&mut self) {
        let trimmed = self.rest().trim_start();
        self.position = self.text.len() - trimmed.len();
    }

    fn eat(&mut self, token: &str) -> bool {
        self.space();
        if self.rest().starts_with(token) {
            self.position += token.len();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, token: &str) -> Result<(), String> {
        if self.eat(token) {
            Ok(())
        } else {
            Err(self.error(&format!("expected `{token}`")))
        }
    }

    fn error(&self, message: &str) -> String {
        format!(
            "`{}` is not a type: {message} at `{}`",
            self.text,
            self.rest()
        )
    }

    fn word(&mut self) -> Option<String> {
        self.space();
        let rest = self.rest();
        let length = rest
            .char_indices()
            .find(|(_, character)| {
                !(character.is_ascii_alphanumeric() || *character == '_' || *character == '.')
            })
            .map_or(rest.len(), |(index, _)| index);
        if length == 0 || !rest.starts_with(|character: char| character.is_ascii_alphabetic()) {
            return None;
        }
        let start = self.position;
        self.position += length;
        Some(self.text[start..start + length].to_owned())
    }

    fn number(&mut self) -> Result<u32, String> {
        self.space();
        let rest = self.rest();
        let length = rest
            .char_indices()
            .find(|(_, character)| !(character.is_ascii_alphanumeric() || *character == '_'))
            .map_or(rest.len(), |(index, _)| index);
        let digits = &rest[..length];
        let value = match digits.strip_prefix("0x") {
            Some(hex) => u32::from_str_radix(hex, 16).ok(),
            None => digits.parse().ok(),
        };
        let value = value.ok_or_else(|| self.error("expected a number"))?;
        self.position += length;
        Ok(value)
    }

    fn ty(&mut self) -> Result<Ty, String> {
        let mut ty = self.primary()?;
        while self.eat("[") {
            let length = self.number()?;
            if length == 0 {
                return Err(self.error("an array has at least one element"));
            }
            self.expect("]")?;
            ty = Ty::Array(Box::new(ty), length);
        }
        Ok(ty)
    }

    fn primary(&mut self) -> Result<Ty, String> {
        let Some(word) = self.word() else {
            return Err(self.error("expected a type"));
        };
        if let Some((_, primitive)) = Primitive::ALL.iter().find(|(name, _)| *name == word) {
            return Ok(Ty::Primitive(*primitive));
        }
        match word.as_str() {
            "char8" => Ok(Ty::Char8),
            "char16" => Ok(Ty::Char16),
            "opaque" => Ok(Ty::Opaque),
            "ptr" => {
                if self.eat("<") {
                    let inner = self.ty()?;
                    self.expect(">")?;
                    Ok(Ty::Ptr(Some(Box::new(inner))))
                } else {
                    Ok(Ty::Ptr(None))
                }
            }
            "fn" => self.function(),
            "void" => Err(self.error("`void` is only a function's return")),
            _ => {
                let mut args = Vec::new();
                if self.eat("(") {
                    loop {
                        args.push(self.ty()?);
                        if !self.eat(",") {
                            break;
                        }
                    }
                    self.expect(")")?;
                }
                Ok(Ty::Named(word, args))
            }
        }
    }

    fn function(&mut self) -> Result<Ty, String> {
        self.expect("(")?;
        let mut params = Vec::new();
        if !self.eat(")") {
            loop {
                let save = self.position;
                let name = match self.word() {
                    Some(word) if !word.contains('.') && self.eat(":") => Some(word),
                    _ => {
                        self.position = save;
                        None
                    }
                };
                params.push(Param {
                    name,
                    ty: self.ty()?,
                });
                if !self.eat(",") {
                    break;
                }
            }
            self.expect(")")?;
        }
        let ret = if self.eat("->") {
            let save = self.position;
            if self.word().as_deref() == Some("void") {
                None
            } else {
                self.position = save;
                Some(Box::new(self.ty()?))
            }
        } else {
            None
        };
        Ok(Ty::Fn(FnType { params, ret }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_prints_every_form() {
        for text in [
            "u32",
            "ptr",
            "ptr<object.character>",
            "char8[64]",
            "ptr<ui.unit_base>[256]",
            "ptr<ptr<ui.res_node>[1024]>",
            "fn(ptr, u8) -> ptr",
            "fn(this: ptr<object.character>) -> u32",
            "fn()",
            "std.vector(object.character)",
            "opaque",
        ] {
            assert_eq!(Ty::parse(text).unwrap().to_string(), text);
        }
        assert_eq!(Ty::parse("fn(ptr) -> void").unwrap().to_string(), "fn(ptr)");
    }

    #[test]
    fn refuses_what_is_not_a_type() {
        for text in ["", "u32[0]", "ptr<u8", "void", "fn(", "u32 u8", "9u"] {
            assert!(Ty::parse(text).is_err(), "{text}");
        }
    }
}
