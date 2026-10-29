//! A test plugin that intercepts the scripted game's functions in every way
//! the host allows. The host's tests click `before:<function>:<handler>` or
//! `after:<function>:<handler>` to intercept, and each handler number does
//! one thing to the call; the log tells what happened. It also calls the
//! game's functions through the views its `build.rs` generates from its
//! `anatomia/`: `add:<a>:<b>`, `thunk:<handler>` and
//! `thunk:<handler>:<fallback>` to make a thunk, `each:<count>` to hand
//! the last one to `game.each`.
use angelica_sdk::{
    Plugin,
    intercepts::{self, Call, Phase, Value},
    memory,
    ui::{Event, Window},
};
use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, OnceLock};

/// The views of the plugin's own entries.
mod own {
    include!(concat!(env!("OUT_DIR"), "/anatomia.rs"));
}

/// The thunk the plugin made last.
static THUNK: Mutex<Option<angelica_sdk::calls::Thunk>> = Mutex::new(None);

struct Intercepting;

fn unsigned(value: Option<&Value>) -> u64 {
    match value {
        Some(Value::Unsigned(number)) => *number,
        _ => 0,
    }
}

fn refused<T>(result: Result<T, angelica_sdk::Error>) -> String {
    match result {
        Ok(_) => "accepted".into(),
        Err(error) => format!("{:?}", error.kind),
    }
}

impl Plugin for Intercepting {
    fn start() -> Result<Self, String> {
        Ok(Self)
    }

    /// Reads the snapshot's tables once, before handlers use them.
    fn prepare() -> Result<(), String> {
        if intercepts::snapshot() == b"broken" {
            return Err("the snapshot is broken".into());
        }
        let tables = read_tables();
        TABLES
            .set(tables)
            .map_err(|_| "this instance prepared a snapshot already".to_owned())
    }

    fn ui(&mut self) -> Vec<Window> {
        vec![Window::new("main", "Intercepting")]
    }

    fn ui_event(&mut self, event: Event) {
        let Event::Clicked(target) = event else {
            return;
        };
        if target.id == "broken" {
            let result = intercepts::publish_snapshot(b"broken");
            angelica_sdk::info(&format!("broken: {}", refused(result)));
            return;
        }
        if let Some(sizes) = target.id.strip_prefix("tables:") {
            let (rows, paths) = sizes.split_once(':').unwrap_or((sizes, "0"));
            let snapshot = tables(rows.parse().unwrap_or(0), paths.parse().unwrap_or(0));
            let result = intercepts::publish_snapshot(&snapshot);
            angelica_sdk::info(&format!("tables: {}", refused(result)));
            return;
        }
        if let Some(line) = calling(&target.id) {
            angelica_sdk::info(&line);
            return;
        }
        let mut parts = target.id.split(':');
        let (Some(phase), Some(function), Some(Ok(handler))) =
            (parts.next(), parts.next(), parts.next().map(str::parse))
        else {
            return;
        };
        let result = match phase {
            "before" => intercepts::before(function, handler),
            "after" => intercepts::after(function, handler),
            _ => return,
        };
        angelica_sdk::info(&format!(
            "{phase} {function} {handler}: {}",
            refused(result)
        ));
    }

    /// 0 logs the call; 1 doubles the first argument and 2 adds one to it;
    /// 3 skips the game's code, returning 99; 4 doubles the result and 5
    /// adds one to it; 6 tries what a `before` handler may not; 7 changes
    /// the first argument and traps; 8 doubles the float arguments and adds
    /// one to the fifth, on the stack; 9 passes the second argument's
    /// handle on as it is, then a zero pointer; 10 replaces a row's text
    /// from the snapshot's row table, as a translation does, and 11 looks
    /// a file's path up in its path set, as file replacements do; 12 reads
    /// and writes the fields of an object of the plugin's own structure.
    /// 0 gives ten times the argument; 1 fails, so the fallback runs; 2
    /// gives a value of another type.
    fn thunk(call: angelica_sdk::calls::ThunkCall) -> memory::Value {
        let first = match call.args.first() {
            Some(memory::Value::Unsigned(number)) => *number,
            _ => 0,
        };
        match call.handler {
            0 => memory::Value::Unsigned(first * 10),
            1 => panic!("the thunk's handler fails"),
            _ => memory::Value::Boolean(true),
        }
    }

    fn intercepted(call: Call) {
        let first = unsigned(call.args.first());
        let returned = unsigned(Some(&call.returned));
        let outcome = match call.handler {
            0 => {
                angelica_sdk::info(&format!(
                    "{:?} args={:?} returned={:?} skipped={}",
                    call.phase, call.args, call.returned, call.skipped
                ));
                Ok(())
            }
            1 => intercepts::set_argument(0, &Value::Unsigned(first * 2)),
            2 => intercepts::set_argument(0, &Value::Unsigned(first + 1)),
            3 => intercepts::skip(&Value::Unsigned(99)),
            4 => intercepts::set_result(&Value::Unsigned(returned * 2)),
            5 => intercepts::set_result(&Value::Unsigned(returned + 1)),
            6 => {
                let attempts = [
                    refused(intercepts::set_argument(0, &Value::Signed(1))),
                    refused(intercepts::set_argument(0, &Value::Unsigned(1 << 40))),
                    refused(intercepts::set_argument(9, &Value::Unsigned(1))),
                    refused(intercepts::set_result(&Value::Unsigned(1))),
                    refused(intercepts::skip(&Value::Boolean(true))),
                    refused(intercepts::set_argument(1, &Value::Pointer(1))),
                ];
                angelica_sdk::info(&format!("attempts: {}", attempts.join(", ")));
                Ok(())
            }
            7 => {
                let _ = intercepts::set_argument(0, &Value::Unsigned(1000));
                panic!("the handler fails after changing an argument");
            }
            8 => {
                let mut outcome = Ok(());
                for (index, value) in call.args.iter().enumerate() {
                    let index = u32::try_from(index).unwrap_or(u32::MAX);
                    let changed = match value {
                        Value::Float32(number) => Value::Float32(number * 2.0),
                        Value::Float64(number) => Value::Float64(number * 2.0),
                        Value::Unsigned(number) if index == 4 => Value::Unsigned(number + 1),
                        _ => continue,
                    };
                    outcome = outcome.and(intercepts::set_argument(index, &changed));
                }
                outcome
            }
            10 => row(&call),
            11 => path(&call),
            12 => boxed(&call),
            9 => match call.args.get(1) {
                Some(text @ Value::Text(handle)) => {
                    let read = intercepts::text(*handle)
                        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                    angelica_sdk::info(&format!("text: {read:?}"));
                    intercepts::set_argument(1, text)
                        .and_then(|()| intercepts::set_argument(1, &Value::None))
                }
                _ => Ok(()),
            },
            _ => Ok(()),
        };
        if let Err(error) = outcome {
            angelica_sdk::warning(&format!(
                "handler {} in {:?}: {:?} {}",
                call.handler, call.phase, error.kind, error.message
            ));
        }
        if call.phase == Phase::After && call.skipped {
            angelica_sdk::info(&format!("after a skip: {:?}", call.returned));
        }
    }
}

angelica_sdk::plugin!(Intercepting);

/// Calls of the game and thunks a test asks for by `id`, and what came of
/// them; none for another id.
fn calling(id: &str) -> Option<String> {
    let parts: Vec<&str> = id.split(':').collect();
    let number = |at: usize| {
        parts
            .get(at)
            .and_then(|part| part.parse::<u32>().ok())
            .unwrap_or(0)
    };
    Some(match parts.first().copied()? {
        "add" => format!("add: {}", refused_or(own::game::add(number(1), number(2)))),
        "thunk" => {
            let fallback = parts.get(2).copied();
            match angelica_sdk::calls::thunk("fn(u32) -> u32", number(1), fallback) {
                Ok(thunk) => {
                    *THUNK
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(thunk);
                    "thunk: accepted".into()
                }
                Err(error) => format!("thunk: {:?}", error.kind),
            }
        }
        "each" => {
            let thunk = *THUNK
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            match thunk {
                Some(thunk) => format!("each: {}", refused_or(own::game::each(&thunk, number(1)))),
                None => "each: no thunk".into(),
            }
        }
        _ => return None,
    })
}

/// A number a call gave, or the kind of its error.
fn refused_or(result: Result<u32, angelica_sdk::Error>) -> String {
    result.map_or_else(
        |error| format!("{:?}", error.kind),
        |number| number.to_string(),
    )
}

/// Reads an object of the plugin's own `game.box` through its call handle,
/// adds one to its count, and tries what a handler may not.
fn boxed(call: &Call) -> Result<(), angelica_sdk::Error> {
    static COUNT: memory::Field = memory::Field::new("game.box.count");
    static LABEL: memory::Field = memory::Field::new("game.box.label");
    let Some(Value::Object(object)) = call.args.first() else {
        return Ok(());
    };
    let count: u32 = memory::read(*object, &COUNT)?;
    memory::write(*object, &COUNT, count + 1)?;
    memory::write(*object, &LABEL, b"BOX".as_slice())?;
    let label: Vec<u8> = memory::read(*object, &LABEL)?;
    let held = refused(angelica_sdk::api::memory::hold(*object));
    let central = refused(angelica_sdk::api::memory::field(
        "character.character_data.level",
    ));
    angelica_sdk::info(&format!(
        "box: count {count} label {} hold {held} central {central}",
        String::from_utf8_lossy(&label)
    ));
    Ok(())
}

/// A snapshot of `rows` rows of text, `sheet row length text`, and `paths`
/// file paths, `length path`, each count first.
fn tables(rows: u32, paths: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&rows.to_le_bytes());
    for index in 0..rows {
        let text = format!("строка {index:08}");
        bytes.extend_from_slice(&(index % 100).to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&u16::try_from(text.len()).unwrap_or(0).to_le_bytes());
        bytes.extend_from_slice(text.as_bytes());
    }
    bytes.extend_from_slice(&paths.to_le_bytes());
    for index in 0..paths {
        let path = format!("chara/equipment/e{index:04}/texture/v01_c0101e{index:04}_top_n.tex");
        bytes.extend_from_slice(&u16::try_from(path.len()).unwrap_or(0).to_le_bytes());
        bytes.extend_from_slice(path.as_bytes());
    }
    bytes
}

/// The snapshot's tables, which `prepare` read.
struct Tables {
    rows: HashMap<(u32, u32), Vec<u8>>,
    paths: HashSet<Vec<u8>>,
}

static TABLES: OnceLock<Tables> = OnceLock::new();

fn loaded() -> &'static Tables {
    TABLES.get_or_init(|| Tables {
        rows: HashMap::new(),
        paths: HashSet::new(),
    })
}

fn read_tables() -> Tables {
    let bytes = intercepts::snapshot();
    let mut at = 0;
    let mut take = |count: usize| {
        let slice = bytes.get(at..at + count).unwrap_or(&[]).to_vec();
        at += count;
        slice
    };
    let number = |bytes: Vec<u8>| {
        bytes
            .iter()
            .rev()
            .fold(0u32, |n, b| (n << 8) | u32::from(*b))
    };
    let mut rows = HashMap::new();
    for _ in 0..number(take(4)) {
        let sheet = number(take(4));
        let row = number(take(4));
        let length = number(take(2)) as usize;
        rows.insert((sheet, row), take(length));
    }
    let mut paths = HashSet::new();
    for _ in 0..number(take(4)) {
        let length = number(take(2)) as usize;
        paths.insert(take(length));
    }
    Tables { rows, paths }
}

/// `store_row(sheet, row, text)`: the row's text becomes the table's when
/// the table has the row and the text is as long.
fn row(call: &Call) -> Result<(), angelica_sdk::Error> {
    let (Some(Value::Unsigned(sheet)), Some(Value::Unsigned(row)), Some(Value::Text(text))) =
        (call.args.first(), call.args.get(1), call.args.get(2))
    else {
        return Ok(());
    };
    let key = (
        u32::try_from(*sheet).unwrap_or(u32::MAX),
        u32::try_from(*row).unwrap_or(u32::MAX),
    );
    let Some(replacement) = loaded().rows.get(&key) else {
        return Ok(());
    };
    let current = intercepts::text(*text)?;
    if current.len() == replacement.len() && current != *replacement {
        intercepts::edit_text(*text, replacement)?;
    }
    Ok(())
}

/// `read_file(path)`: whether the path is replaced; a replaced one skips
/// the game's read, returning 1.
fn path(call: &Call) -> Result<(), angelica_sdk::Error> {
    let Some(Value::Text(path)) = call.args.first() else {
        return Ok(());
    };
    if loaded().paths.contains(&intercepts::text(*path)?) {
        intercepts::skip(&Value::Unsigned(1))?;
    }
    Ok(())
}
