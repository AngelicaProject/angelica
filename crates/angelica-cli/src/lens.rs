//! Lens from the command line: what the game map's object model holds
//! (`schema search`, `schema show`), a query checked against it without the
//! game (`query`), and a query run in the game by a development build of
//! the host (`look`).
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use angelica_map::{Domain, Event, Field, Length, Map, Meaning, Objects, Shape, Value};
use angelica_pe::{Image, Layout};
use clap::{Args, Subcommand};
use runtime::lens::{ColumnMeaning, Schema, Stored};
use serde_json::{Value as Json, json};

use crate::Output;

const EXECUTABLE: &str = "ffxiv_dx11.exe";

#[derive(Debug, Subcommand)]
pub enum SchemaCommand {
    /// Roots, types, fields, events, actions, domains, and enums whose
    /// names or descriptions have every word.
    Search(SearchArgs),
    /// A root, type, event, action, domain, or enum with everything in it.
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
    /// The query; with `--watch`, from an event; with `--dump`, a path to
    /// objects.
    query: Option<String>,
    /// The installation folder `angelica.json` in the game folder names.
    #[arg(long, required_unless_present = "compare")]
    installation: Option<PathBuf>,
    /// Watch the event of the query for this many seconds and print every
    /// firing.
    #[arg(long, value_name = "SECONDS", conflicts_with_all = ["dump", "trace"])]
    watch: Option<u32>,
    /// Print this many bytes of each object the path reaches; save the
    /// JSON with `--json` to compare it later.
    #[arg(long, value_name = "BYTES", conflicts_with = "trace")]
    dump: Option<u32>,
    /// Count how many times the game's code reaches this function or
    /// location entry of the map; repeatable.
    #[arg(long, value_name = "ENTRY")]
    trace: Vec<String>,
    /// How many seconds `--trace` counts.
    #[arg(long, default_value_t = 10)]
    seconds: u32,
    /// Compare two dumps saved with `--dump --json`, before and after
    /// something happened in the game, and print the bytes that changed.
    #[arg(long, num_args = 2, value_names = ["BEFORE", "AFTER"], conflicts_with_all = ["watch", "dump", "trace", "installation"])]
    compare: Vec<PathBuf>,
    /// Seconds to wait for the game's answer, after a watch or a trace.
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

/// What an event's argument holds, in words.
fn arg_shape(objects: &Objects, arg: &angelica_map::Arg) -> String {
    let field = Field {
        name: arg.name.clone(),
        doc: arg.doc.clone(),
        at: String::new(),
        shape: arg.shape.clone(),
        domain: arg.domain.clone(),
        forbidden: arg.forbidden,
        set: None,
    };
    let held = match &arg.shape {
        Shape::Text(length) => format!("text of at most {length} bytes, behind a pointer"),
        _ => shape(objects, &field),
    };
    match arg.source {
        angelica_map::ArgSource::Argument(position) => format!("argument {position}, {held}"),
        angelica_map::ArgSource::Register(register) => {
            format!("{}, {held}", format!("{register:?}").to_lowercase())
        }
    }
}

/// What a parameter of an action takes, as `schema show` prints it.
fn param_shape(objects: &Objects, param: &angelica_map::Param) -> String {
    let mut text = match &param.meaning {
        Meaning::Plain => format!("{:?}", param.kind).to_lowercase(),
        Meaning::Sheet(sheet) => format!("a row of {sheet}"),
        Meaning::Enum(name) | Meaning::Flags(name) => {
            let values = objects
                .enums
                .get(name)
                .map(|found| found.values.keys().cloned().collect::<Vec<_>>().join(" "))
                .unwrap_or_default();
            let what = if matches!(param.meaning, Meaning::Flags(_)) {
                "flags"
            } else {
                "one"
            };
            format!("{what} of {name} ({values})")
        }
    };
    if let Some(unit) = &param.unit {
        let _ = write!(text, ", in {unit}");
    }
    if let Some((least, most)) = param.range {
        let _ = write!(text, ", {} to {}", least.float(), most.float());
    }
    match param.default {
        Some(angelica_map::Number::Integer(value)) => {
            let _ = write!(text, ", {value} when left out");
        }
        Some(number) => {
            let _ = write!(text, ", {} when left out", number.float());
        }
        None => {}
    }
    text
}

/// An action, its parameters, and its conditions, for `show`.
fn show_action(
    objects: &Objects,
    schema: Option<&Schema>,
    action: &angelica_map::Action,
) -> (Json, String) {
    let available = schema.map(|schema| {
        schema
            .actions
            .get(&action.name)
            .cloned()
            .unwrap_or_else(|| Err("not in the schema".into()))
            .map(drop)
    });
    let mut text = String::new();
    let _ = writeln!(
        text,
        "action {} calls {} [{}], at most {} a second: {}",
        action.name, action.call, action.domain, action.rate, action.doc
    );
    if let Some(Err(reason)) = &available {
        let _ = writeln!(text, "  absent in this build: {reason}");
    }
    let mut params = Vec::new();
    for (name, param) in action.params() {
        let shape = param_shape(objects, param);
        let _ = writeln!(text, "  {}.{name}: {shape} {}", action.name, param.doc);
        params.push(json!({"name": name, "doc": param.doc, "shape": shape}));
    }
    let mut when = Vec::new();
    for condition in &action.when {
        let _ = writeln!(text, "  when {} (else: {})", condition.path, condition.says);
        when.push(json!({"path": condition.path, "says": condition.says}));
    }
    (
        json!({"kind": "action", "name": action.name, "call": action.call, "doc": action.doc,
               "domain": action.domain, "rate": action.rate, "params": params, "when": when,
               "available": available.as_ref().map(Result::is_ok), "reason": available.and_then(Result::err)}),
        text,
    )
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
        "set": field.set.as_ref().map(settable),
        "available": available.as_ref().map(Result::is_ok),
        "reason": available.and_then(Result::err),
    })
}

/// Which values plugins may set a field to.
fn settable(set: &angelica_map::Settable) -> String {
    match set.range {
        Some((least, most)) => format!("{} to {}", least.float(), most.float()),
        None => "any value".to_owned(),
    }
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
    if let Some(set) = &field.set {
        let _ = write!(line, " (plugins change it: {})", settable(set));
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
    found_in_types(objects, schema, &matches, &mut found);
    found_in_events(objects, schema, &matches, &mut found);
    for action in objects.actions.values() {
        let params = action
            .params()
            .map(|(name, _)| name)
            .collect::<Vec<_>>()
            .join(" ");
        if matches(&[&action.name, &action.doc, &action.call, &params]) {
            let (json, text) = show_action(objects, schema, action);
            let first = text.lines().next().unwrap_or_default().to_owned();
            found.push((json, first));
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
        return Ok(show_domain(objects, schema, domain));
    }
    if let Some(event) = objects.events.get(name) {
        return Ok(show_event(objects, schema, event));
    }
    if let Some(action) = objects.actions.get(name) {
        return Ok(show_action(objects, schema, action));
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
        "the game map has no root, type, event, action, domain, or enum {name}; try `angelica schema search`"
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
    let permitted = |domain: &str| permitted.as_ref().is_none_or(|read| read.contains(domain));
    let watch = runtime::lens::query::parse(&args.query)
        .is_ok_and(|query| loaded.map.objects().events.contains_key(&query.root));
    let prepared = if watch {
        runtime::lens::prepare_watch(&schema, &args.query, &permitted)
    } else {
        runtime::lens::prepare(&schema, &args.query, &permitted)
    }
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

/// Rows as lines: positions in brackets, then `column=value`.
fn row_lines(text: &mut String, columns: &[runtime::look::Column], rows: &[runtime::look::Row]) {
    for row in rows {
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
}

fn look(args: &LookArgs) -> Result<(Json, String), String> {
    use runtime::look::{Mode, REQUEST, RESPONSE, Request, Response, write_atomically};
    if let [before, after] = args.compare.as_slice() {
        return compare(before, after);
    }
    let installation = args
        .installation
        .as_ref()
        .ok_or("--installation names the installation the game runs")?;
    let (mode, lasts) = match (&args.watch, &args.dump, args.trace.is_empty()) {
        (Some(seconds), _, _) => (Mode::Watch, u64::from(*seconds)),
        (None, Some(_), _) => (Mode::Dump, 0),
        (None, None, false) => (Mode::Trace, u64::from(args.seconds)),
        (None, None, true) => (Mode::Rows, 0),
    };
    let query = args.query.clone().unwrap_or_default();
    if mode != Mode::Trace && query.is_empty() {
        return Err("give the query".into());
    }
    let folder = installation.join("look");
    std::fs::create_dir_all(&folder).map_err(|e| format!("{}: {e}", folder.display()))?;
    let response_path = folder.join(RESPONSE);
    let _ = std::fs::remove_file(&response_path);
    let request = Request {
        format: 1,
        id: request_id(),
        mode,
        query,
        seconds: (mode == Mode::Watch || mode == Mode::Trace)
            .then_some(u32::try_from(lasts).unwrap_or(u32::MAX)),
        bytes: args.dump,
        entries: args.trace.clone(),
    };
    let bytes = serde_json::to_vec(&request).map_err(|e| e.to_string())?;
    write_atomically(&folder.join(REQUEST), &bytes)?;
    let started = Instant::now();
    let wait = Duration::from_secs(args.timeout + lasts);
    let response = loop {
        if let Ok(bytes) = std::fs::read(&response_path)
            && let Ok(response) = serde_json::from_slice::<Response>(&bytes)
            && response.id == request.id
        {
            let _ = std::fs::remove_file(&response_path);
            break response;
        }
        if started.elapsed() > wait {
            let _ = std::fs::remove_file(folder.join(REQUEST));
            return Err(format!(
                "no answer in {} s: the game must be running with a development build of Angelica from {}",
                wait.as_secs(),
                installation.display()
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    if let Some(error) = &response.error {
        return Err(format!("{}: {}", error.kind, error.message));
    }
    let text = response_text(mode, &response);
    Ok((serde_json::to_value(&response).unwrap_or_default(), text))
}

/// Types and their fields that `matches`, for `search`.
fn found_in_types(
    objects: &Objects,
    schema: Option<&Schema>,
    matches: &dyn Fn(&[&str]) -> bool,
    found: &mut Vec<(Json, String)>,
) {
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
}

/// Events and their arguments that `matches`, for `search`.
fn found_in_events(
    objects: &Objects,
    schema: Option<&Schema>,
    matches: &dyn Fn(&[&str]) -> bool,
    found: &mut Vec<(Json, String)>,
) {
    for event in objects.events.values() {
        let available = schema.and_then(|schema| schema.events.get(&event.name).cloned());
        if matches(&[&event.name, &event.doc, &event.at]) {
            let mut line = format!(
                "event {} at {} [{}] {}",
                event.name, event.at, event.domain, event.doc
            );
            if let Some(Err(reason)) = &available {
                let _ = write!(line, " (absent in this build: {reason})");
            }
            found.push((
                json!({"kind": "event", "name": event.name, "at": event.at, "doc": event.doc, "domain": event.domain,
                       "available": available.as_ref().map(Result::is_ok), "reason": available.clone().and_then(Result::err)}),
                line,
            ));
        }
        for arg in event.args.values() {
            if matches(&[&event.name, &arg.name, &arg.doc]) {
                found.push((
                    json!({"kind": "argument", "name": format!("{}.{}", event.name, arg.name), "doc": arg.doc,
                           "domain": arg.domain, "shape": arg_shape(objects, arg)}),
                    format!(
                        "argument {}.{}: {} [{}] {}",
                        event.name,
                        arg.name,
                        arg_shape(objects, arg),
                        arg.domain,
                        arg.doc
                    ),
                ));
            }
        }
    }
}

/// A domain and every field in it, for `show`.
fn show_domain(objects: &Objects, schema: Option<&Schema>, domain: &Domain) -> (Json, String) {
    let mut text = String::new();
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
    (
        json!({"kind": "domain", "name": domain.name, "doc": domain.doc, "class": format!("{:?}", domain.class),
               "read": {"en": domain.read.en, "ru": domain.read.ru}, "fields": fields}),
        text,
    )
}

/// An event and its arguments, for `show`.
fn show_event(objects: &Objects, schema: Option<&Schema>, event: &Event) -> (Json, String) {
    let mut text = String::new();
    let available = schema.and_then(|schema| schema.events.get(&event.name).cloned());
    let _ = writeln!(
        text,
        "event {} at {} [{}], at most {} firings a frame: {}",
        event.name, event.at, event.domain, event.rate, event.doc
    );
    if let Some(Err(reason)) = &available {
        let _ = writeln!(text, "  absent in this build: {reason}");
    }
    let mut args = Vec::new();
    for arg in event.args.values() {
        let _ = writeln!(
            text,
            "  {}.{}: {} [{}] {}",
            event.name,
            arg.name,
            arg_shape(objects, arg),
            arg.domain,
            arg.doc
        );
        args.push(json!({"name": arg.name, "doc": arg.doc, "domain": arg.domain, "shape": arg_shape(objects, arg)}));
    }
    (
        json!({"kind": "event", "name": event.name, "at": event.at, "doc": event.doc, "domain": event.domain,
               "rate": event.rate, "available": available.as_ref().map(Result::is_ok),
               "reason": available.and_then(Result::err), "args": args}),
        text,
    )
}

/// What `look` prints for a response.
fn response_text(mode: runtime::look::Mode, response: &runtime::look::Response) -> String {
    use runtime::look::Mode;
    let columns = response.columns.clone().unwrap_or_default();
    let mut text = String::new();
    match mode {
        Mode::Rows => {
            let rows = response.rows.clone().unwrap_or_default();
            row_lines(&mut text, &columns, &rows);
            let _ = writeln!(text, "{} rows", rows.len());
        }
        Mode::Watch => {
            let firings = response.firings.clone().unwrap_or_default();
            for (number, rows) in firings.iter().enumerate() {
                let _ = writeln!(text, "firing {}:", number + 1);
                row_lines(&mut text, &columns, rows);
            }
            let _ = writeln!(
                text,
                "{} firings, {} left out",
                firings.len(),
                response.dropped.unwrap_or(0)
            );
        }
        Mode::Dump => {
            for object in response.objects.clone().unwrap_or_default() {
                let indexes = object
                    .indexes
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                let _ = writeln!(text, "[{indexes}] at {:#x}", object.address);
                match &object.bytes {
                    None => text.push_str("  unreadable\n"),
                    Some(hex) => {
                        for (line, chunk) in hex.as_bytes().chunks(32).enumerate() {
                            let pairs = chunk
                                .chunks(2)
                                .map(|pair| String::from_utf8_lossy(pair).into_owned())
                                .collect::<Vec<_>>()
                                .join(" ");
                            let _ = writeln!(text, "  +{:04x}  {pairs}", line * 16);
                        }
                    }
                }
            }
        }
        Mode::Trace => {
            let mut calls: Vec<(String, u64)> = response
                .calls
                .clone()
                .unwrap_or_default()
                .into_iter()
                .collect();
            calls.sort_by(|left, right| right.1.cmp(&left.1).then(left.0.cmp(&right.0)));
            for (entry, count) in calls {
                let _ = writeln!(text, "{count:>8}  {entry}");
            }
        }
    }
    text
}

/// The bytes that differ between two dumps, object by object.
fn compare(before: &PathBuf, after: &PathBuf) -> Result<(Json, String), String> {
    use runtime::look::Response;
    let read = |path: &PathBuf| -> Result<Response, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        serde_json::from_slice::<Response>(&bytes)
            .ok()
            .filter(|response| response.objects.is_some())
            .ok_or_else(|| format!("{}: not a dump saved with --dump --json", path.display()))
    };
    let (before, after) = (read(before)?, read(after)?);
    let decode = |hex: &str| -> Vec<u8> {
        hex.as_bytes()
            .chunks(2)
            .filter_map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
            .collect()
    };
    let mut text = String::new();
    let mut changes = Vec::new();
    for old in before.objects.unwrap_or_default() {
        let Some(new) = after
            .objects
            .iter()
            .flatten()
            .find(|new| new.indexes == old.indexes)
        else {
            continue;
        };
        let (Some(old_bytes), Some(new_bytes)) = (&old.bytes, &new.bytes) else {
            continue;
        };
        let (old_bytes, new_bytes) = (decode(old_bytes), decode(new_bytes));
        let indexes = old
            .indexes
            .iter()
            .map(u32::to_string)
            .collect::<Vec<_>>()
            .join(",");
        // Runs of changed bytes, each shown once.
        let mut offset = 0;
        while offset < old_bytes.len().min(new_bytes.len()) {
            if old_bytes[offset] == new_bytes[offset] {
                offset += 1;
                continue;
            }
            let start = offset;
            while offset < old_bytes.len().min(new_bytes.len())
                && old_bytes[offset] != new_bytes[offset]
            {
                offset += 1;
            }
            let show = |bytes: &[u8]| {
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            let _ = writeln!(
                text,
                "[{indexes}] +{start:#06x}: {} -> {}",
                show(&old_bytes[start..offset]),
                show(&new_bytes[start..offset])
            );
            changes.push(json!({"indexes": old.indexes, "offset": start,
                "before": show(&old_bytes[start..offset]), "after": show(&new_bytes[start..offset])}));
        }
    }
    if changes.is_empty() {
        text.push_str("nothing changed\n");
    }
    Ok((json!({"changes": changes}), text))
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
