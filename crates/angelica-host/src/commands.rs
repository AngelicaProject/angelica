//! Plugin chat commands (the `commands` interface): each plugin's command
//! tree, the lines the game's command handler passed on, and parsing a line
//! into an invocation, a help text, or a usage error. Platform independent:
//! the Windows adapter only hands over the lines whose first word names a
//! plugin's command.
#![forbid(unsafe_code)]
use crate::{
    chat::{Span, Tone},
    error::{Error, Kind, Result},
    game_data::{Cell, GameData},
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    sync::{Mutex, OnceLock, PoisonError, RwLock},
};

/// Commands one plugin defines.
pub const MAX_COMMANDS: usize = 64;
/// Words of a command's path.
pub const MAX_DEPTH: usize = 3;
pub const MAX_PARAMETERS: usize = 16;
pub const MAX_CHOICES: usize = 64;
pub const MAX_ALIASES: usize = 8;
pub const MAX_DESCRIPTION: usize = 256;
/// Lines waiting for the next frame; the player is told about further
/// lines, which do not run.
pub const MAX_PENDING: usize = 32;
/// The longest line the host parses.
pub const MAX_LINE: usize = 1000;
const MAX_WORD: usize = 32;
/// Command words the host keeps for itself.
const RESERVED: &[&str] = &["angelica"];
const HELP: &str = "help";
/// The sheet of the game's text commands.
const GAME_COMMANDS: &str = "TextCommand";

#[derive(Debug, Clone, PartialEq)]
pub enum ValueKind {
    Word,
    Text,
    Integer { min: i64, max: i64 },
    Number,
    Choice(Vec<String>),
    Switch,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    pub name: String,
    pub description: String,
    pub kind: ValueKind,
    pub optional: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub path: Vec<String>,
    pub aliases: Vec<String>,
    pub description: String,
    pub parameters: Vec<Parameter>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Word(String),
    Text(String),
    Integer(i64),
    Number(f64),
    Choice(String),
    Switch(bool),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub path: Vec<String>,
    pub arguments: Vec<(String, Value)>,
}

/// What a line the player sent comes to.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// The plugin's command runs.
    Run(Invocation),
    /// The host answers in the chat log as the plugin: help or a usage
    /// error.
    Reply(Vec<Span>),
}

#[derive(Default)]
pub struct Commands {
    /// Each plugin's commands, parents before children.
    trees: RwLock<BTreeMap<String, Vec<Command>>>,
    pending: Mutex<VecDeque<Vec<u8>>>,
    /// The first words of lines that did not fit in `pending`.
    overflowed: Mutex<BTreeSet<String>>,
    game: OnceLock<BTreeSet<String>>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::invalid(message)
}

fn valid_word(word: &str) -> bool {
    (1..=MAX_WORD).contains(&word.len())
        && word
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn valid_description(text: &str) -> bool {
    text.len() <= MAX_DESCRIPTION && !text.chars().any(char::is_control)
}

impl Commands {
    /// Replaces the plugin's commands.
    ///
    /// # Errors
    /// Refuses a tree that breaks a rule of the `commands` interface, a root
    /// word the game or another plugin uses, and, as `unavailable`, game
    /// files without the game's command list.
    pub fn define(&self, plugin: &str, commands: Vec<Command>, data: &GameData) -> Result<()> {
        check_tree(&commands)?;
        let roots: Vec<&str> = commands
            .iter()
            .filter(|c| c.path.len() == 1)
            .flat_map(|c| std::iter::once(&c.path[0]).chain(&c.aliases))
            .map(String::as_str)
            .collect();
        if !roots.is_empty() {
            let game = self.game_commands(data)?;
            if let Some(word) = roots
                .iter()
                .find(|w| game.contains(**w) || RESERVED.contains(w))
            {
                return Err(invalid(format!("/{word} is a command of the game")));
            }
        }
        let mut trees = self.trees.write().unwrap_or_else(PoisonError::into_inner);
        for (other, tree) in trees.iter().filter(|(other, _)| *other != plugin) {
            if let Some(word) = roots.iter().find(|w| root_of(tree, w).is_some()) {
                return Err(invalid(format!("/{word} is a command of {other}")));
            }
        }
        if commands.is_empty() {
            trees.remove(plugin);
        } else {
            trees.insert(plugin.to_owned(), commands);
        }
        Ok(())
    }

    pub fn remove_plugin(&self, plugin: &str) {
        self.trees
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin);
    }

    /// Takes a line the player sent when its first word is a plugin's
    /// command; the game then does not run it. Called on the game's main
    /// thread before the game handles the line.
    pub fn submit(&self, line: &[u8]) -> bool {
        let Some(word) = first_word(line) else {
            return false;
        };
        let ours = self
            .trees
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .any(|tree| root_of(tree, &word).is_some());
        if ours {
            let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
            if pending.len() < MAX_PENDING {
                pending.push_back(line.to_vec());
            } else {
                // A word is a plugin's root, so the set stays small.
                self.overflowed
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .insert(word);
            }
        }
        ours
    }

    /// The lines submitted since the last call.
    pub fn take(&self) -> Vec<Vec<u8>> {
        self.pending
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain(..)
            .collect()
    }

    /// The host's answers to the lines that did not fit in the queue since
    /// the last call, one per command word, for the plugin it belongs to.
    pub fn take_overflowed(&self) -> Vec<(String, Outcome)> {
        let words = std::mem::take(
            &mut *self
                .overflowed
                .lock()
                .unwrap_or_else(PoisonError::into_inner),
        );
        let trees = self.trees.read().unwrap_or_else(PoisonError::into_inner);
        words
            .into_iter()
            .filter_map(|word| {
                let (plugin, _) = trees
                    .iter()
                    .find(|(_, tree)| root_of(tree, &word).is_some())?;
                Some((
                    plugin.clone(),
                    Outcome::Reply(vec![Span::new(
                        format!(
                            "/{word}: a line was not run; at most {MAX_PENDING} lines wait for a frame"
                        ),
                        Tone::Bad,
                    )]),
                ))
            })
            .collect()
    }

    /// The plugin a submitted line is for and what it comes to; `None` when
    /// no plugin defines its command any more.
    #[must_use]
    pub fn resolve(&self, line: &[u8]) -> Option<(String, Outcome)> {
        let word = first_word(line)?;
        let trees = self.trees.read().unwrap_or_else(PoisonError::into_inner);
        let (plugin, tree) = trees
            .iter()
            .find(|(_, tree)| root_of(tree, &word).is_some())?;
        let outcome = match std::str::from_utf8(line) {
            Ok(text) if !text.contains(['\u{2}', '\u{3}']) && text.len() <= MAX_LINE => {
                parse(tree, text)
            }
            _ => Outcome::Reply(vec![Span::new(
                "Plugin commands take plain text: no links or auto-translate phrases.",
                Tone::Bad,
            )]),
        };
        Some((plugin.clone(), outcome))
    }

    fn game_commands(&self, data: &GameData) -> Result<&BTreeSet<String>> {
        if let Some(words) = self.game.get() {
            return Ok(words);
        }
        let words = read_game_commands(data).map_err(|error| {
            Error::new(
                Kind::Unavailable,
                format!("the game's command list cannot be read: {}", error.message),
            )
        })?;
        Ok(self.game.get_or_init(|| words))
    }
}

/// Every command word in the game's command sheet, in every language: the
/// letters, digits, and hyphens after a `/` that starts a text cell.
fn read_game_commands(data: &GameData) -> Result<BTreeSet<String>> {
    let info = data.sheet(GAME_COMMANDS)?;
    let mut words = BTreeSet::new();
    for &language in &info.languages {
        let mut after = None;
        loop {
            let rows = data.rows(GAME_COMMANDS, language, after, 1024)?;
            let Some(last) = rows.last() else { break };
            after = Some((last.row, last.subrow));
            for row in &rows {
                for cell in &row.cells {
                    if let Cell::Text(text) = cell
                        && let Some(rest) = text.strip_prefix(b"/")
                    {
                        let word: String = rest
                            .iter()
                            .take_while(|b| b.is_ascii_alphanumeric() || **b == b'-')
                            .map(|&b| char::from(b.to_ascii_lowercase()))
                            .collect();
                        if !word.is_empty() {
                            words.insert(word);
                        }
                    }
                }
            }
        }
    }
    if words.is_empty() {
        return Err(Error::new(Kind::NotFound, "the sheet lists no commands"));
    }
    Ok(words)
}

fn check_tree(commands: &[Command]) -> Result<()> {
    if commands.len() > MAX_COMMANDS {
        return Err(invalid(format!("at most {MAX_COMMANDS} commands")));
    }
    for (index, command) in commands.iter().enumerate() {
        let shown = format!("/{}", command.path.join(" "));
        if command.path.is_empty() || command.path.len() > MAX_DEPTH {
            return Err(invalid(format!(
                "{shown}: a path has 1 to {MAX_DEPTH} words"
            )));
        }
        for word in command.path.iter().chain(&command.aliases) {
            if !valid_word(word) {
                return Err(invalid(format!(
                    "{shown}: {word:?} is not 1-{MAX_WORD} lower-case letters, digits, and hyphens"
                )));
            }
        }
        if command.aliases.len() > MAX_ALIASES {
            return Err(invalid(format!("{shown}: at most {MAX_ALIASES} aliases")));
        }
        if !valid_description(&command.description) {
            return Err(invalid(format!(
                "{shown}: a description is one line of at most {MAX_DESCRIPTION} bytes"
            )));
        }
        let parent = &command.path[..command.path.len() - 1];
        let earlier = &commands[..index];
        if !parent.is_empty() && !earlier.iter().any(|c| c.path == parent) {
            return Err(invalid(format!(
                "{shown}: its parent is not listed before it"
            )));
        }
        if command.path.len() > 1
            && std::iter::once(command.path.last().expect("not empty"))
                .chain(&command.aliases)
                .any(|word| word == HELP)
        {
            return Err(invalid(format!("{shown}: help is the host's subcommand")));
        }
        // Words that run a sibling: the name and the aliases.
        let words = |c: &Command| {
            std::iter::once(c.path.last().cloned().unwrap_or_default())
                .chain(c.aliases.iter().cloned())
                .collect::<Vec<_>>()
        };
        let mine = words(command);
        let unique: BTreeSet<_> = mine.iter().collect();
        if unique.len() != mine.len()
            || earlier
                .iter()
                .filter(|c| {
                    c.path.len() == command.path.len() && c.path[..c.path.len() - 1] == *parent
                })
                .any(|c| words(c).iter().any(|w| mine.contains(w)))
        {
            return Err(invalid(format!("{shown}: a word runs two commands")));
        }
        check_parameters(&shown, &command.parameters)?;
    }
    Ok(())
}

fn check_parameters(shown: &str, parameters: &[Parameter]) -> Result<()> {
    if parameters.len() > MAX_PARAMETERS {
        return Err(invalid(format!(
            "{shown}: at most {MAX_PARAMETERS} parameters"
        )));
    }
    let mut names = BTreeSet::new();
    let mut optional = false;
    for (index, parameter) in parameters.iter().enumerate() {
        let name = &parameter.name;
        if !valid_word(name) || !names.insert(name) {
            return Err(invalid(format!(
                "{shown}: parameter {name:?} is not a unique word"
            )));
        }
        if !valid_description(&parameter.description) {
            return Err(invalid(format!(
                "{shown} <{name}>: a description is one line of at most {MAX_DESCRIPTION} bytes"
            )));
        }
        if optional && !parameter.optional {
            return Err(invalid(format!(
                "{shown} <{name}>: a required parameter follows an optional one"
            )));
        }
        optional |= parameter.optional;
        match &parameter.kind {
            ValueKind::Text if index + 1 != parameters.len() => {
                return Err(invalid(format!(
                    "{shown} <{name}>: only the last parameter takes the rest of the line"
                )));
            }
            ValueKind::Integer { min, max } if min > max => {
                return Err(invalid(format!("{shown} <{name}>: min is above max")));
            }
            ValueKind::Choice(words) => {
                let unique: BTreeSet<String> = words.iter().map(|w| w.to_lowercase()).collect();
                if words.is_empty()
                    || words.len() > MAX_CHOICES
                    || unique.len() != words.len()
                    || words.iter().any(|w| {
                        w.is_empty()
                            || w.len() > MAX_WORD
                            || w.chars()
                                .any(|c| c.is_whitespace() || c.is_control() || c == '"')
                    })
                {
                    return Err(invalid(format!(
                        "{shown} <{name}>: a choice has 1 to {MAX_CHOICES} distinct words of at most {MAX_WORD} bytes"
                    )));
                }
            }
            _ => {}
        }
    }
    Ok(())
}

/// The lower-case command word a line starts with, without its `/`.
fn first_word(line: &[u8]) -> Option<String> {
    let rest = line.strip_prefix(b"/")?;
    let end = rest
        .iter()
        .position(u8::is_ascii_whitespace)
        .unwrap_or(rest.len());
    let word = std::str::from_utf8(&rest[..end]).ok()?.to_ascii_lowercase();
    valid_word(&word).then_some(word)
}

/// The root command `word` runs.
fn root_of<'a>(tree: &'a [Command], word: &str) -> Option<&'a Command> {
    child(tree, &[], word)
}

/// The child of the command at `parent` that `word` runs.
fn child<'a>(tree: &'a [Command], parent: &[String], word: &str) -> Option<&'a Command> {
    let word = word.to_ascii_lowercase();
    tree.iter().find(|c| {
        c.path.len() == parent.len() + 1
            && c.path[..parent.len()] == *parent
            && (c.path.last() == Some(&word) || c.aliases.contains(&word))
    })
}

fn children<'a>(tree: &'a [Command], parent: &'a [String]) -> impl Iterator<Item = &'a Command> {
    tree.iter()
        .filter(move |c| c.path.len() == parent.len() + 1 && c.path[..parent.len()] == *parent)
}

/// A word of the line and where it starts.
struct Token<'a> {
    text: &'a str,
    start: usize,
}

/// The words of a line: runs without whitespace, or text in double quotes.
fn tokens(line: &str) -> std::result::Result<Vec<Token<'_>>, String> {
    let mut out = Vec::new();
    let mut at = 0;
    let bytes = line.as_bytes();
    while at < bytes.len() {
        if bytes[at].is_ascii_whitespace() {
            at += 1;
            continue;
        }
        if bytes[at] == b'"' {
            let Some(end) = line[at + 1..].find('"') else {
                return Err("a quote is not closed".into());
            };
            out.push(Token {
                text: &line[at + 1..at + 1 + end],
                start: at,
            });
            at += end + 2;
        } else {
            let end = line[at..]
                .find(|c: char| c.is_ascii_whitespace())
                .map_or(line.len(), |end| at + end);
            out.push(Token {
                text: &line[at..end],
                start: at,
            });
            at = end;
        }
    }
    Ok(out)
}

fn parse(tree: &[Command], line: &str) -> Outcome {
    let words = match tokens(line) {
        Ok(words) => words,
        Err(problem) => return Outcome::Reply(vec![Span::new(problem, Tone::Bad)]),
    };
    let Some(root) = words.first().and_then(|w| root_of(tree, &w.text[1..])) else {
        return Outcome::Reply(vec![Span::new("Unknown command", Tone::Bad)]);
    };
    let mut command = root;
    let mut next = 1;
    if words
        .get(next)
        .is_some_and(|w| w.text.eq_ignore_ascii_case(HELP))
    {
        next += 1;
        while let Some(found) = words
            .get(next)
            .and_then(|w| child(tree, &command.path, w.text))
        {
            command = found;
            next += 1;
        }
        return Outcome::Reply(help(tree, command));
    }
    while let Some(found) = words
        .get(next)
        .and_then(|w| child(tree, &command.path, w.text))
    {
        command = found;
        next += 1;
    }
    match arguments(command, line, &words[next..]) {
        Ok(arguments) => Outcome::Run(Invocation {
            path: command.path.clone(),
            arguments,
        }),
        Err(problem) => {
            let mut spans = vec![
                Span::new(problem, Tone::Bad),
                Span::new("\nUsage: ", Tone::Muted),
                Span::new(usage(command), Tone::Accent),
            ];
            spans.push(Span::new(format!(" · /{} help", root.path[0]), Tone::Muted));
            Outcome::Reply(spans)
        }
    }
}

fn arguments(
    command: &Command,
    line: &str,
    words: &[Token<'_>],
) -> std::result::Result<Vec<(String, Value)>, String> {
    let mut out = Vec::new();
    let mut next = 0;
    for parameter in &command.parameters {
        let name = &parameter.name;
        let Some(word) = words.get(next) else {
            if parameter.optional {
                break;
            }
            return Err(format!("Missing <{name}>."));
        };
        next += 1;
        let value = match &parameter.kind {
            ValueKind::Word => Value::Word(word.text.to_owned()),
            ValueKind::Text => {
                next = words.len();
                Value::Text(line[word.start..].trim_end().to_owned())
            }
            ValueKind::Integer { min, max } => match word.text.parse::<i64>() {
                Ok(value) if (*min..=*max).contains(&value) => Value::Integer(value),
                Ok(_) => return Err(format!("<{name}> is from {min} to {max}.")),
                Err(_) => return Err(format!("<{name}> is a whole number, not {:?}.", word.text)),
            },
            ValueKind::Number => match word.text.parse::<f64>() {
                Ok(value) if value.is_finite() => Value::Number(value),
                _ => return Err(format!("<{name}> is a number, not {:?}.", word.text)),
            },
            ValueKind::Choice(choices) => {
                match choices.iter().find(|c| c.eq_ignore_ascii_case(word.text)) {
                    Some(choice) => Value::Choice(choice.clone()),
                    None => {
                        return Err(format!("<{name}> is one of {}.", choices.join(", ")));
                    }
                }
            }
            ValueKind::Switch => match word.text.to_ascii_lowercase().as_str() {
                "on" => Value::Switch(true),
                "off" => Value::Switch(false),
                _ => return Err(format!("<{name}> is on or off.")),
            },
        };
        out.push((name.clone(), value));
    }
    if let Some(extra) = words.get(next) {
        return Err(format!("Unexpected {:?}.", extra.text));
    }
    Ok(out)
}

/// `/root sub <name> [<optional>]`, with the words of a choice.
fn usage(command: &Command) -> String {
    use std::fmt::Write;
    let mut out = format!("/{}", command.path.join(" "));
    for parameter in &command.parameters {
        let inner = match &parameter.kind {
            ValueKind::Choice(words) => words.join("|"),
            ValueKind::Switch => "on|off".into(),
            ValueKind::Text => format!("{}...", parameter.name),
            _ => parameter.name.clone(),
        };
        let _ = if parameter.optional {
            write!(out, " [{inner}]")
        } else {
            write!(out, " <{inner}>")
        };
    }
    out
}

fn kind_hint(kind: &ValueKind) -> Option<String> {
    match kind {
        ValueKind::Integer { min, max } => Some(format!("whole number, {min} to {max}")),
        ValueKind::Number => Some("number".into()),
        ValueKind::Text => Some("rest of the line".into()),
        ValueKind::Word | ValueKind::Choice(_) | ValueKind::Switch => None,
    }
}

/// A command's usage and description, its parameters, and its
/// subcommands.
fn help(tree: &[Command], command: &Command) -> Vec<Span> {
    let mut spans = vec![Span::new(usage(command), Tone::Accent)];
    if !command.description.is_empty() {
        spans.push(Span::new(
            format!(" — {}", command.description),
            Tone::Normal,
        ));
    }
    if !command.aliases.is_empty() {
        spans.push(Span::new(
            format!(" (also {})", command.aliases.join(", ")),
            Tone::Muted,
        ));
    }
    for parameter in &command.parameters {
        spans.push(Span::new(format!("\n  {}", parameter.name), Tone::Accent));
        if !parameter.description.is_empty() {
            spans.push(Span::new(
                format!(" — {}", parameter.description),
                Tone::Normal,
            ));
        }
        if let Some(hint) = kind_hint(&parameter.kind) {
            spans.push(Span::new(format!(" ({hint})"), Tone::Muted));
        }
    }
    for sub in children(tree, &command.path) {
        spans.push(Span::new(format!("\n{}", usage(sub)), Tone::Accent));
        if !sub.description.is_empty() {
            spans.push(Span::new(format!(" — {}", sub.description), Tone::Normal));
        }
    }
    if children(tree, &command.path).next().is_some() {
        spans.push(Span::new(
            format!("\n/{} help <subcommand> for more", command.path.join(" ")),
            Tone::Muted,
        ));
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;
    use angelica_sqpack::testing::{FakeGame, TextSheet};

    fn data() -> (tempfile::TempDir, GameData) {
        let folder = tempfile::tempdir().unwrap();
        FakeGame::new("1")
            .with_text(
                "TextCommand",
                &TextSheet::new(3, &[0, 1, 2])
                    .row(1, &[(0, "/record"), (1, "/replay"), (2, "USAGE: /record")])
                    .row(2, &[(0, "/tell"), (1, "/t")]),
            )
            .write(folder.path())
            .unwrap();
        let data = GameData::new(Some(folder.path()));
        (folder, data)
    }

    fn command(path: &[&str], parameters: Vec<Parameter>) -> Command {
        Command {
            path: path.iter().map(|w| (*w).to_owned()).collect(),
            aliases: Vec::new(),
            description: format!("does {}", path.join(" ")),
            parameters,
        }
    }

    fn parameter(name: &str, kind: ValueKind, optional: bool) -> Parameter {
        Parameter {
            name: name.into(),
            description: format!("the {name}"),
            kind,
            optional,
        }
    }

    fn tree() -> Vec<Command> {
        let mut root = command(&["hello"], vec![]);
        root.aliases = vec!["hi".into()];
        let mut greet = command(
            &["hello", "greet"],
            vec![
                parameter("name", ValueKind::Word, false),
                parameter("times", ValueKind::Integer { min: 1, max: 5 }, true),
            ],
        );
        greet.aliases = vec!["g".into()];
        vec![
            root,
            greet,
            command(
                &["hello", "say"],
                vec![
                    parameter(
                        "mood",
                        ValueKind::Choice(vec!["Happy".into(), "sad".into()]),
                        false,
                    ),
                    parameter("words", ValueKind::Text, false),
                ],
            ),
            command(&["hello", "set"], vec![]),
            command(
                &["hello", "set", "sound"],
                vec![
                    parameter("on", ValueKind::Switch, false),
                    parameter("volume", ValueKind::Number, true),
                ],
            ),
        ]
    }

    fn run(commands: &Commands, line: &str) -> Outcome {
        assert!(commands.submit(line.as_bytes()), "{line}");
        let lines = commands.take();
        assert_eq!(lines.len(), 1);
        let (plugin, outcome) = commands.resolve(&lines[0]).unwrap();
        assert_eq!(plugin, "p");
        outcome
    }

    fn invocation(path: &[&str], arguments: Vec<(&str, Value)>) -> Outcome {
        Outcome::Run(Invocation {
            path: path.iter().map(|w| (*w).to_owned()).collect(),
            arguments: arguments
                .into_iter()
                .map(|(name, value)| (name.to_owned(), value))
                .collect(),
        })
    }

    fn reply(outcome: &Outcome) -> String {
        match outcome {
            Outcome::Reply(spans) => spans.iter().map(|s| s.text.as_str()).collect(),
            Outcome::Run(invocation) => panic!("ran {invocation:?}"),
        }
    }

    #[test]
    fn lines_parse_into_typed_invocations() {
        let (_folder, data) = data();
        let commands = Commands::default();
        commands.define("p", tree(), &data).unwrap();

        assert_eq!(run(&commands, "/hello"), invocation(&["hello"], vec![]));
        assert_eq!(
            run(&commands, "/HI G \"Mog Mog\" 3"),
            invocation(
                &["hello", "greet"],
                vec![
                    ("name", Value::Word("Mog Mog".into())),
                    ("times", Value::Integer(3))
                ]
            )
        );
        assert_eq!(
            run(&commands, "/hello greet kupo"),
            invocation(
                &["hello", "greet"],
                vec![("name", Value::Word("kupo".into()))]
            )
        );
        assert_eq!(
            run(&commands, "/hello say HAPPY  it  works  "),
            invocation(
                &["hello", "say"],
                vec![
                    ("mood", Value::Choice("Happy".into())),
                    ("words", Value::Text("it  works".into()))
                ]
            )
        );
        assert_eq!(
            run(&commands, "/hello set sound off 0.5"),
            invocation(
                &["hello", "set", "sound"],
                vec![("on", Value::Switch(false)), ("volume", Value::Number(0.5))]
            )
        );
    }

    #[test]
    fn mistakes_are_answered_with_the_usage() {
        let (_folder, data) = data();
        let commands = Commands::default();
        commands.define("p", tree(), &data).unwrap();
        for (line, problem) in [
            ("/hello greet", "Missing <name>."),
            ("/hello greet kupo 9", "<times> is from 1 to 5."),
            ("/hello greet kupo x", "<times> is a whole number"),
            ("/hello greet kupo 1 2", "Unexpected \"2\"."),
            ("/hello say angry x", "<mood> is one of Happy, sad."),
            ("/hello set sound maybe", "<on> is on or off."),
            ("/hello set sound on inf", "<volume> is a number"),
            ("/hello greet \"kupo", "a quote is not closed"),
        ] {
            let text = reply(&run(&commands, line));
            assert!(text.starts_with(problem), "{line}: {text}");
        }
        let text = reply(&run(&commands, "/hello greet"));
        assert!(
            text.contains("Usage: /hello greet <name> [times] · /hello help"),
            "{text}"
        );
        let text = reply(&run(&commands, "/hello \u{2}x\u{3}"));
        assert!(text.contains("plain text"));
    }

    #[test]
    fn help_is_built_from_the_descriptions() {
        let (_folder, data) = data();
        let commands = Commands::default();
        commands.define("p", tree(), &data).unwrap();
        let text = reply(&run(&commands, "/hello help"));
        assert!(text.starts_with("/hello — does hello (also hi)"), "{text}");
        assert!(text.contains("\n/hello greet <name> [times] — does hello greet"));
        assert!(text.contains("\n/hello say <Happy|sad> <words...>"));
        let text = reply(&run(&commands, "/hello help greet"));
        assert!(
            text.contains("\n  times — the times (whole number, 1 to 5)"),
            "{text}"
        );
    }

    #[test]
    fn only_plugin_commands_are_taken() {
        let (_folder, data) = data();
        let commands = Commands::default();
        commands.define("p", tree(), &data).unwrap();
        for line in ["/tell a hi", "hello", "/hellox", "", "/"] {
            assert!(!commands.submit(line.as_bytes()), "{line}");
        }
        assert!(commands.take().is_empty());
        commands.remove_plugin("p");
        assert!(!commands.submit(b"/hello"));
    }

    #[test]
    fn lines_past_the_queue_are_answered() {
        let (_folder, data) = data();
        let commands = Commands::default();
        commands.define("p", tree(), &data).unwrap();
        for _ in 0..=MAX_PENDING {
            assert!(commands.submit(b"/hello"));
        }
        assert_eq!(commands.take().len(), MAX_PENDING);
        let answers = commands.take_overflowed();
        assert_eq!(answers.len(), 1);
        assert_eq!(answers[0].0, "p");
        assert!(
            matches!(&answers[0].1, Outcome::Reply(spans) if spans[0].text.contains("was not run"))
        );
        assert!(commands.take_overflowed().is_empty());
    }

    #[test]
    fn trees_are_checked() {
        let (_folder, data) = data();
        let commands = Commands::default();
        let refused = |tree: Vec<Command>, problem: &str| {
            let error = commands.define("p", tree, &data).unwrap_err();
            assert!(error.message.contains(problem), "{}", error.message);
        };
        refused(
            vec![command(&["tell"], vec![])],
            "/tell is a command of the game",
        );
        refused(
            vec![command(&["replay"], vec![])],
            "/replay is a command of the game",
        );
        refused(vec![command(&["angelica"], vec![])], "of the game");
        refused(vec![command(&["Hello"], vec![])], "lower-case");
        refused(vec![command(&["a", "b"], vec![])], "parent");
        refused(
            vec![command(&["a"], vec![]), command(&["a", "help"], vec![])],
            "help is the host",
        );
        refused(
            vec![
                command(&["a"], vec![]),
                command(&["a", "b"], vec![]),
                command(&["a", "b"], vec![]),
            ],
            "two commands",
        );
        refused(
            vec![command(
                &["a"],
                vec![
                    parameter("x", ValueKind::Word, true),
                    parameter("y", ValueKind::Word, false),
                ],
            )],
            "follows an optional",
        );
        refused(
            vec![command(
                &["a"],
                vec![
                    parameter("x", ValueKind::Text, false),
                    parameter("y", ValueKind::Word, false),
                ],
            )],
            "only the last",
        );
        refused(
            vec![command(
                &["a"],
                vec![parameter("x", ValueKind::Integer { min: 2, max: 1 }, false)],
            )],
            "min is above max",
        );
        refused(
            vec![command(
                &["a"],
                vec![parameter(
                    "x",
                    ValueKind::Choice(vec!["a".into(), "A".into()]),
                    false,
                )],
            )],
            "distinct",
        );

        commands.define("p", tree(), &data).unwrap();
        let error = commands
            .define("q", vec![command(&["hi"], vec![])], &data)
            .unwrap_err();
        assert!(error.message.contains("/hi is a command of p"));
        commands.define("p", Vec::new(), &data).unwrap();
        commands
            .define("q", vec![command(&["hi"], vec![])], &data)
            .unwrap();
    }

    /// The installed game: `ANGELICA_GAME_PATH` is its `game` folder.
    #[test]
    #[ignore = "needs ANGELICA_GAME_PATH"]
    fn reads_the_installed_games_commands() {
        let game = std::path::PathBuf::from(
            std::env::var("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH"),
        );
        let data = GameData::new(game.parent());
        let commands = Commands::default();
        commands.define("p", tree(), &data).unwrap();
        for word in ["tell", "echo", "e"] {
            let error = commands
                .define("q", vec![command(&[word], vec![])], &data)
                .unwrap_err();
            assert!(
                error.message.contains("of the game"),
                "{word}: {}",
                error.message
            );
        }
    }

    #[test]
    fn without_the_game_command_list_roots_are_unavailable() {
        let folder = tempfile::tempdir().unwrap();
        FakeGame::new("1").write(folder.path()).unwrap();
        let data = GameData::new(Some(folder.path()));
        let error = Commands::default()
            .define("p", vec![command(&["hello"], vec![])], &data)
            .unwrap_err();
        assert_eq!(error.kind, Kind::Unavailable);
    }
}
