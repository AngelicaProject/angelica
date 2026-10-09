//! Chat commands as a tree: build it with [`Command`], define it with
//! [`define`], and read what the player ran from an [`Invocation`].
//!
//! ```ignore
//! use angelica_sdk::commands::{Command, Parameter};
//!
//! angelica_sdk::commands::define(&[Command::new("hello", "Greets you")
//!     .alias("hi")
//!     .subcommand(
//!         Command::new("greet", "Greets someone")
//!             .parameter(Parameter::word("name", "Who to greet"))
//!             .parameter(Parameter::integer("times", "How often", 1, 5).optional()),
//!     )])?;
//!
//! // In `Plugin::command`:
//! if invocation.is(&["hello", "greet"]) {
//!     let name = invocation.word("name").unwrap_or_default();
//!     let times = invocation.integer("times").unwrap_or(1);
//! }
//! ```
use crate::api::commands as api;
pub use api::{Invocation, Value};

/// A command and its subcommands.
#[derive(Debug, Clone)]
pub struct Command {
    name: String,
    aliases: Vec<String>,
    description: String,
    parameters: Vec<api::Parameter>,
    subcommands: Vec<Command>,
}

impl Command {
    /// A command run by `name`: the root's word, such as `hello` for
    /// `/hello`, or a subcommand's.
    #[must_use]
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            aliases: Vec::new(),
            description: description.into(),
            parameters: Vec::new(),
            subcommands: Vec::new(),
        }
    }

    /// Another word that runs the command.
    #[must_use]
    pub fn alias(mut self, alias: impl Into<String>) -> Self {
        self.aliases.push(alias.into());
        self
    }

    #[must_use]
    pub fn parameter(mut self, parameter: Parameter) -> Self {
        self.parameters.push(parameter.0);
        self
    }

    #[must_use]
    pub fn subcommand(mut self, command: Command) -> Self {
        self.subcommands.push(command);
        self
    }

    fn flatten(&self, parent: &[String], out: &mut Vec<api::Command>) {
        let mut path = parent.to_vec();
        path.push(self.name.clone());
        out.push(api::Command {
            path: path.clone(),
            aliases: self.aliases.clone(),
            description: self.description.clone(),
            parameters: self.parameters.clone(),
        });
        for command in &self.subcommands {
            command.flatten(&path, out);
        }
    }
}

/// A parameter of a command.
#[derive(Debug, Clone)]
pub struct Parameter(api::Parameter);

impl Parameter {
    fn new(name: impl Into<String>, description: impl Into<String>, kind: api::ValueKind) -> Self {
        Self(api::Parameter {
            name: name.into(),
            description: description.into(),
            kind,
            optional: false,
        })
    }

    /// One word, or text in double quotes.
    #[must_use]
    pub fn word(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, api::ValueKind::Word)
    }

    /// The rest of the line; only as the last parameter.
    #[must_use]
    pub fn text(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, api::ValueKind::Text)
    }

    /// A whole number from `min` to `max`.
    #[must_use]
    pub fn integer(
        name: impl Into<String>,
        description: impl Into<String>,
        min: i64,
        max: i64,
    ) -> Self {
        Self::new(
            name,
            description,
            api::ValueKind::Integer(api::IntegerBounds { min, max }),
        )
    }

    #[must_use]
    pub fn number(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, api::ValueKind::Number)
    }

    /// One of `words`.
    #[must_use]
    pub fn choice(name: impl Into<String>, description: impl Into<String>, words: &[&str]) -> Self {
        Self::new(
            name,
            description,
            api::ValueKind::Choice(words.iter().map(|w| (*w).to_owned()).collect()),
        )
    }

    /// `on` or `off`.
    #[must_use]
    pub fn switch(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, api::ValueKind::Switch)
    }

    /// The player may leave it out; optional parameters come last.
    #[must_use]
    pub fn optional(mut self) -> Self {
        self.0.optional = true;
        self
    }
}

/// Replaces the plugin's commands with these trees; an empty slice removes
/// them.
///
/// # Errors
/// See `commands.define`.
pub fn define(commands: &[Command]) -> Result<(), crate::Error> {
    let mut flat = Vec::new();
    for command in commands {
        command.flatten(&[], &mut flat);
    }
    api::define(&flat)
}

impl Invocation {
    /// Whether the player ran the command at `path`, such as
    /// `&["hello", "greet"]`.
    #[must_use]
    pub fn is(&self, path: &[&str]) -> bool {
        self.path.len() == path.len() && self.path.iter().zip(path).all(|(a, b)| a == b)
    }

    /// The argument of the parameter `name`, when the player gave one.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Value> {
        self.arguments
            .iter()
            .find(|argument| argument.name == name)
            .map(|argument| &argument.value)
    }

    /// The text of a word, text, or choice argument.
    #[must_use]
    pub fn word(&self, name: &str) -> Option<String> {
        match self.get(name)? {
            Value::Word(text) | Value::Text(text) | Value::Choice(text) => Some(text.clone()),
            _ => None,
        }
    }

    #[must_use]
    pub fn integer(&self, name: &str) -> Option<i64> {
        match self.get(name)? {
            Value::Integer(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn number(&self, name: &str) -> Option<f64> {
        match self.get(name)? {
            Value::Number(value) => Some(*value),
            _ => None,
        }
    }

    #[must_use]
    pub fn switch(&self, name: &str) -> Option<bool> {
        match self.get(name)? {
            Value::Switch(on) => Some(*on),
            _ => None,
        }
    }
}
