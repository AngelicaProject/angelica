//! Lens from the command line: what the game map's object model holds
//! (`schema search`, `schema show`), a query checked against it without the
//! game (`query`), and a query run in the game by a development build of
//! the host (`look`).
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use angelica_map::{Field, Length, Map, Meaning, Objects, Shape, Value};
use angelica_pe::{Image, Layout};
use clap::{Args, Subcommand};
use runtime::lens::{ColumnMeaning, Schema, Stored};
use serde_json::{Value as Json, json};

use crate::Output;

const EXECUTABLE: &str = "ffxiv_dx11.exe";

#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
    /// Roots, types, fields, domains, and enums whose names or descriptions
    /// have every word.
    Search(SearchArgs),
    /// A root, type, domain, or enum with everything in it.
    Show(ShowArgs),
}

#[derive(Debug, Args)]
pub struct Source {
    /// The game map directory, with `map.toml`.
    #[arg(long)]
    map: PathBuf,
    /// The game's `game` folder, to say what this build lacks.
    #[arg(long)]
    game: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct SearchArgs {
    /// Words to look for, in any case.
    #[arg(required = true)]
    words: Vec<String>,
    #[command(flatten)]
    source: Source,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// A root, type, domain, or enum name.
    name: String,
    #[command(flatten)]
    source: Source,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, Args)]
pub struct QueryArgs {
    /// The query, as a plugin passes it to `lens.prepare`.
    query: String,
    #[command(flatten)]
    source: Source,
    /// A plugin folder with `angelica.toml`: check the domains its
    /// `[lens] read` lists.
    #[arg(long)]
    plugin: Option<PathBuf>,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, Args)]
pub struct LookArgs {
    /// The query.
    query: String,
    /// The installation folder `angelica.json` in the game folder names.
    #[arg(long)]
    installation: PathBuf,
    /// Seconds to wait for the game's answer.
    #[arg(long, default_value_t = 10)]
    timeout: u64,
    #[command(flatten)]
    output: Output,
}

/// The map and, with a game folder, its schema for that build.
struct Loaded {
    map: Map,
    /// `None` without a game folder.
    schema: Option<Schema>,
    game_version: Option<String>,
}

fn load(source: &Source) -> Result<Loaded, String> {
    let map = Map::load(&source.map).map_err(|error| error.to_string())?;
    let Some(game) = &source.game else {
        return Ok(Loaded {
            map,
            schema: None,
            game_version: None,
        });
    };
    let path = game.join(EXECUTABLE);
    let executable = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let image =
        Image::parse(&executable, Layout::File).map_err(|e| format!("{}: {e}", path.display()))?;
    let resolution = map.resolve(&image, image.image_base());
    let schema = Schema::new(&map, 0, |name| match resolution.get(name) {
        Some(Ok(value)) => Ok(*value),
        Some(Err(absence)) => Err(absence.to_string()),
        None => Err("not in the map".into()),
    });
    let game_version = std::fs::read_to_string(game.join("ffxivgame.ver"))
        .ok()
        .map(|text| text.trim().to_owned());
    Ok(Loaded {
        map,
        schema: Some(schema),
        game_version,
    })
}

/// A schema in which every entry is present: a constant offset as written,
/// an offset bound to code as zero. For checking names and domains only.
fn unchecked(map: &Map) -> Schema {
    Schema::new(map, 0, |name| {
        match map.get(name).map(|entry| &entry.kind) {
            Some(angelica_map::EntryKind::Field { offset, .. }) => {
                Ok(Value::Offset(offset.unwrap_or(0)))
            }
            _ => Ok(Value::Address(0)),
        }
    })
}

/// What a field holds, in words.
fn shape(objects: &Objects, field: &Field) -> String {
    match &field.shape {
        Shape::Value {
            kind,
            meaning,
            unit,
        } => {
            let mut text = format!("{kind:?}").to_lowercase();
            match meaning {
                Meaning::Plain => {}
                Meaning::Sheet(sheet) => {
                    let _ = write!(text, ", a row of {sheet}");
                }
                Meaning::Enum(name) => {
                    let values = objects.enums.get(name).map_or_else(String::new, |found| {
                        found.values.keys().cloned().collect::<Vec<_>>().join(" ")
                    });
                    let _ = write!(text, ", {name}: {values}");
                }
                Meaning::Flags(name) => {
                    let _ = write!(text, ", flags {name}");
                }
            }
            if let Some(unit) = unit {
                let _ = write!(text, ", in {unit}");
            }
            text
        }
        Shape::Text(length) => format!("text of at most {length} bytes"),
        Shape::Ref(to) => format!("reference to {to}"),
        Shape::Embed(of) => format!("{of}, embedded"),
        Shape::Array {
            array,
            behind_pointer,
        } => {
            let length = match &array.length {
                Length::Fixed(length) => format!("{length}"),
                Length::Field { field, most } => format!("{field} (at most {most})"),
            };
            let mut text = format!("array of {length} {}", array.of);
            if array.pointers {
                text.push_str(", by pointer");
            }
            if *behind_pointer {
                text.push_str(", behind a pointer");
            }
            if let Some(present) = &array.present {
                let _ = write!(text, ", skipping those without {present}");
            }
            text
        }
    }
}

/// Whether a field is in the build: `None` without one.
fn availability(schema: Option<&Schema>, entry: &str) -> Option<Result<(), String>> {
    schema.map(|schema| schema.offset(entry).map(drop))
}

fn field_json(objects: &Objects, owner: &str, field: &Field, schema: Option<&Schema>) -> Json {
    let available = availability(schema, &field.at);
    json!({
        "kind": "field",
        "name": format!("{owner}.{}", field.name),
        "doc": field.doc,
        "domain": field.domain,
        "shape": shape(objects, field),
        "entry": field.at,
        "forbidden": field.forbidden,
        "available": available.as_ref().map(Result::is_ok),
        "reason": available.and_then(Result::err),
    })
}

fn field_line(objects: &Objects, owner: &str, field: &Field, schema: Option<&Schema>) -> String {
    let mut line = format!(
        "{owner}.{}: {} [{}] {}",
        field.name,
        shape(objects, field),
        field.domain,
        field.doc
    );
    if field.forbidden {
        line.push_str(" (never read by a plugin)");
    }
    if let Some(Err(reason)) = availability(schema, &field.at) {
        let _ = write!(line, " (absent in this build: {reason})");
    }
    line
}

/// Everything in the object model whose name or description has every
/// word.
fn search(loaded: &Loaded, words: &[String]) -> Vec<(Json, String)> {
    let objects = loaded.map.objects();
    let schema = loaded.schema.as_ref();
    let words: Vec<String> = words.iter().map(|word| word.to_lowercase()).collect();
    let matches = |texts: &[&str]| {
        let all = texts.join(" ").to_lowercase();
        words.iter().all(|word| all.contains(word.as_str()))
    };
    let mut found = Vec::new();
    for domain in objects.domains.values() {
        if matches(&[&domain.name, &domain.doc, &domain.read.en, &domain.read.ru]) {
            found.push((
                json!({"kind": "domain", "name": domain.name, "doc": domain.doc, "class": format!("{:?}", domain.class), "read": {"en": domain.read.en, "ru": domain.read.ru}}),
                format!("domain {}: {} ({})", domain.name, domain.read.en, domain.doc),
            ));
        }
    }
    for root in objects.roots.values() {
        if matches(&[&root.name, &root.doc, &root.r#type]) {
            let domain = objects
                .types
                .get(&root.r#type)
                .map_or("", |found| found.domain.as_str());
            let available = schema.and_then(|schema| schema.roots.get(&root.name).cloned());
            let mut line = format!(
                "root {} -> {} [{domain}] {}",
                root.name, root.r#type, root.doc
            );
            if let Some(Err(reason)) = &available {
                let _ = write!(line, " (absent in this build: {reason})");
            }
            found.push((
                json!({"kind": "root", "name": root.name, "type": root.r#type, "doc": root.doc, "domain": domain,
                       "available": available.as_ref().map(Result::is_ok), "reason": available.and_then(Result::err)}),
                line,
            ));
        }
    }
    for object in objects.types.values() {
        if matches(&[&object.name, &object.doc]) {
            found.push((
                json!({"kind": "type", "name": object.name, "doc": object.doc, "domain": object.domain, "extends": object.extends}),
                format!("type {} [{}] {}", object.name, object.domain, object.doc),
            ));
        }
        for field in object.fields.values() {
            if matches(&[
                &object.name,
                &field.name,
                &field.doc,
                &shape(objects, field),
            ]) {
                found.push((
                    field_json(objects, &object.name, field, schema),
                    format!("field {}", field_line(objects, &object.name, field, schema)),
                ));
            }
        }
    }
    for found_enum in objects.enums.values() {
        let values = found_enum.values.keys().cloned().collect::<Vec<_>>();
        if matches(&[&found_enum.name, &found_enum.doc, &values.join(" ")]) {
            found.push((
                json!({"kind": "enum", "name": found_enum.name, "doc": found_enum.doc, "flags": found_enum.flags, "values": found_enum.values}),
                format!("enum {}: {} ({})", found_enum.name, values.join(" "), found_enum.doc),
            ));
        }
    }
    found
}

fn show(loaded: &Loaded, name: &str) -> Result<(Json, String), String> {
    let objects = loaded.map.objects();
    let schema = loaded.schema.as_ref();
    let type_report = |type_name: &str, text: &mut String| -> Json {
        let object = &objects.types[type_name];
        let _ = writeln!(
            text,
            "type {} [{}] {}",
            object.name, object.domain, object.doc
        );
        if let Some(base) = &object.extends {
            let _ = writeln!(text, "  extends {base}");
        }
        let mut fields = Vec::new();
        for (field_name, field) in objects.fields(type_name) {
            let owner = objects
                .field(type_name, field_name)
                .map_or(type_name, |(owner, _)| owner.name.as_str());
            let _ = writeln!(text, "  {}", field_line(objects, owner, field, schema));
            fields.push(field_json(objects, owner, field, schema));
        }
        json!({"kind": "type", "name": object.name, "doc": object.doc, "domain": object.domain, "extends": object.extends, "fields": fields})
    };
    let mut text = String::new();
    if let Some(root) = objects.roots.get(name) {
        let available = schema.and_then(|schema| schema.roots.get(name).cloned());
        let _ = writeln!(text, "root {} at {}: {}", root.name, root.global, root.doc);
        if let Some(Err(reason)) = &available {
            let _ = writeln!(text, "  absent in this build: {reason}");
        }
        let shown = type_report(&root.r#type, &mut text);
        return Ok((
            json!({"kind": "root", "name": root.name, "global": root.global, "doc": root.doc,
                   "available": available.as_ref().map(Result::is_ok), "reason": available.and_then(Result::err), "type": shown}),
            text,
        ));
    }
    if objects.types.contains_key(name) {
        let shown = type_report(name, &mut text);
        return Ok((shown, text));
    }
    if let Some(domain) = objects.domains.get(name) {
        let _ = writeln!(
            text,
            "domain {} ({:?}): {}",
            domain.name, domain.class, domain.doc
        );
        let _ = writeln!(text, "  read: {} / {}", domain.read.en, domain.read.ru);
        let mut fields = Vec::new();
        for object in objects.types.values() {
            for field in object
                .fields
                .values()
                .filter(|field| field.domain == domain.name)
            {
                let _ = writeln!(
                    text,
                    "  {}",
                    field_line(objects, &object.name, field, schema)
                );
                fields.push(field_json(objects, &object.name, field, schema));
            }
        }
        return Ok((
            json!({"kind": "domain", "name": domain.name, "doc": domain.doc, "class": format!("{:?}", domain.class),
                   "read": {"en": domain.read.en, "ru": domain.read.ru}, "fields": fields}),
            text,
        ));
    }
    if let Some(found) = objects.enums.get(name) {
        let _ = writeln!(text, "enum {}: {}", found.name, found.doc);
        for (value_name, value) in &found.values {
            let _ = writeln!(text, "  {value_name} = {value}");
        }
        return Ok((
            json!({"kind": "enum", "name": found.name, "doc": found.doc, "flags": found.flags, "values": found.values}),
            text,
        ));
    }
    Err(format!(
        "the game map has no root, type, domain, or enum {name}; try `angelica schema search`"
    ))
}

fn stored(stored: Stored) -> String {
    match stored {
        Stored::Value(kind) => format!("{kind:?}").to_lowercase(),
        Stored::Text(length) => format!("text of at most {length} bytes"),
    }
}

fn meaning(meaning: &ColumnMeaning) -> Option<String> {
    match meaning {
        ColumnMeaning::Plain => None,
        ColumnMeaning::Sheet(sheet) => Some(format!("a row of {sheet}")),
        ColumnMeaning::Enum(_) => Some("a named value".into()),
        ColumnMeaning::Flags(_) => Some("flags".into()),
    }
}

fn query(args: &QueryArgs) -> Result<(Json, String), (Json, String)> {
    let failure = |code: &str, message: String| {
        (
            json!({"error": {"code": code, "message": message}}),
            format!("error: {message}"),
        )
    };
    let loaded = load(&args.source).map_err(|message| failure("load", message))?;
    let permitted = match &args.plugin {
        Some(folder) => {
            let path = folder.join("angelica.toml");
            let text = std::fs::read_to_string(&path)
                .map_err(|e| failure("io", format!("{}: {e}", path.display())))?;
            let manifest = runtime::manifest::Manifest::parse(&text)
                .map_err(|e| failure("manifest", e.to_string()))?;
            Some(manifest.lens_read())
        }
        None => None,
    };
    let schema = loaded
        .schema
        .clone()
        .unwrap_or_else(|| unchecked(&loaded.map));
    let prepared = runtime::lens::prepare(&schema, &args.query, &|domain| {
        permitted.as_ref().is_none_or(|read| read.contains(domain))
    })
    .map_err(|error| {
        let code = format!("{:?}", error.kind);
        failure(&code, error.message)
    })?;
    let build = match (&loaded.schema, &loaded.game_version) {
        (Some(_), Some(version)) => format!("everything is present in game {version}"),
        (Some(_), None) => "everything is present in this build".to_owned(),
        (None, _) => "not checked against a build: give --game".to_owned(),
    };
    let mut text = String::new();
    for column in &prepared.columns {
        let _ = write!(
            text,
            "{}: {} {}",
            column.name,
            column.field,
            stored(column.stored)
        );
        if let Some(meaning) = meaning(&column.meaning) {
            let _ = write!(text, ", {meaning}");
        }
        if let Some(unit) = &column.unit {
            let _ = write!(text, ", in {unit}");
        }
        let _ = writeln!(text, " [{}] {}", column.domain, column.doc);
    }
    let domains: Vec<&String> = prepared.domains.iter().collect();
    let _ = writeln!(
        text,
        "reads: {}",
        domains
            .iter()
            .map(|domain| domain.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let _ = writeln!(text, "{build}");
    let columns: Vec<Json> = prepared
        .columns
        .iter()
        .map(|column| {
            json!({"name": column.name, "field": column.field, "stored": stored(column.stored),
                   "meaning": meaning(&column.meaning), "unit": column.unit, "domain": column.domain, "doc": column.doc})
        })
        .collect();
    Ok((
        json!({"columns": columns, "domains": domains, "checked_build": loaded.schema.is_some(), "game_version": loaded.game_version}),
        text,
    ))
}

/// A random request id.
fn request_id() -> String {
    let mut bytes = [0u8; 8];
    let _ = getrandom::fill(&mut bytes);
    runtime::package::hex(&bytes)
}

fn look(args: &LookArgs) -> Result<(Json, String), String> {
    use runtime::look::{REQUEST, RESPONSE, Request, Response, write_atomically};
    let folder = args.installation.join("look");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let response_path = folder.join(RESPONSE);
    let _ = std::fs::remove_file(&response_path);
    let request = Request {
        format: 1,
        id: request_id(),
        query: args.query.clone(),
    };
    let bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    write_atomically(&folder.join(REQUEST), &bytes)?;
    let started = Instant::now();
    let response = loop {
        if let Ok(bytes) = std::fs::read(&response_path)
            && let Ok(response) = serde_json::from_slice::<Response>(&bytes)
            && response.id == request.id
        {
            let _ = std::fs::remove_file(&response_path);
            break response;
        }
        if started.elapsed() > Duration::from_secs(args.timeout) {
            let _ = std::fs::remove_file(folder.join(REQUEST));
            return Err(format!(
                "no answer in {} s: the game must be running with a development build of Angelica from {}",
                args.timeout,
                args.installation.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if let Some(error) = &response.error {
        return Err(format!("{}: {}", error.kind, error.message));
    }
    let columns = response.columns.clone().unwrap_or_default();
    let rows = response.rows.clone().unwrap_or_default();
    let mut text = String::new();
    for row in &rows {
        let indexes = row
            .indexes
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        let values = columns
            .iter()
            .zip(&row.values)
            .map(|(column, value)| format!("{}={value}", column.name))
            .collect::<Vec<_>>()
            .join(" ");
        if indexes.is_empty() {
            let _ = writeln!(text, "{values}");
        } else {
            let _ = writeln!(text, "[{indexes}] {values}");
        }
    }
    let _ = writeln!(text, "{} rows", rows.len());
    Ok((serde_json::to_value(&response).unwrap_or_default(), text))
}

fn finish(result: Result<(Json, String), (Json, String)>, output: &Output) -> ExitCode {
    match result {
        Ok((json, text)) => {
            if output.json {
                println!("{json}");
            } else {
                print!("{text}");
            }
            ExitCode::SUCCESS
        }
        Err((json, text)) => {
            if output.json {
                println!("{json}");
            } else {
                eprintln!("{text}");
            }
            ExitCode::FAILURE
        }
    }
}

fn failed(code: &str, message: &str) -> (Json, String) {
    (
        json!({"error": {"code": code, "message": message}}),
        format!("error: {message}"),
    )
}

pub fn run_schema(command: &SchemaCommand) -> ExitCode {
    match command {
        SchemaCommand::Search(args) => {
            let result = load(&args.source)
                .map_err(|message| failed("load", &message))
                .map(|loaded| {
                    let found = search(&loaded, &args.words);
                    let mut text = String::new();
                    for (_, line) in &found {
                        let _ = writeln!(text, "{line}");
                    }
                    if found.is_empty() {
                        text.push_str("nothing found\n");
                    }
                    let matches: Vec<Json> = found.into_iter().map(|(json, _)| json).collect();
                    (json!({"matches": matches}), text)
                });
            finish(result, &args.output)
        }
        SchemaCommand::Show(args) => {
            let result = load(&args.source)
                .map_err(|message| failed("load", &message))
                .and_then(|loaded| {
                    show(&loaded, &args.name).map_err(|message| failed("not-found", &message))
                });
            finish(result, &args.output)
        }
    }
}

pub fn run_query(args: &QueryArgs) -> ExitCode {
    finish(query(args), &args.output)
}

pub fn run_look(args: &LookArgs) -> ExitCode {
    finish(
        look(args).map_err(|message| failed("look", &message)),
        &args.output,
    )
}
