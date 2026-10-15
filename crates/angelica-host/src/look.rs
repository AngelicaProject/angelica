//! `angelica look`: Lens from the command line, run in the game by a
//! development build of the host, for work on the game map. The command
//! writes `request.json` in the installation's `look` folder; the host reads
//! it between frames and writes `response.json`. A request asks for one of:
//!
//! - `rows`: a query's rows now;
//! - `watch`: the firings of a watch during some seconds;
//! - `dump`: the bytes of the objects a path reaches, to compare the game's
//!   memory before and after something happens in the game;
//! - `trace`: how many times the game's code reached map entries during some
//!   seconds, to find the function an action of the player calls;
//! - `writes`: which instructions wrote a field during some seconds, in dev
//!   mode, to find the code that changes memory the map does not describe.
//!
//! Every domain is readable, as the player's own tool on their own game;
//! fields no plugin reads stay unread by `rows` and `watch`. A release build
//! never serves it.
#![forbid(unsafe_code)]
use crate::lens::{Prepared, Value, prepare, prepare_objects, prepare_watch};
use crate::sandbox::Services;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const REQUEST: &str = "request.json";
pub const RESPONSE: &str = "response.json";
/// The largest request the host reads.
const MOST_REQUEST: u64 = 64 * 1024;
/// The longest a watch or a trace lasts.
pub const MOST_SECONDS: u32 = 60;
/// The most bytes a dump reads of each object.
pub const MOST_BYTES: u32 = 4096;
/// The most entries a trace counts.
pub const MOST_ENTRIES: usize = 64;
/// The most pointers a dump follows from each object.
pub const MOST_FOLLOWS: usize = 8;
/// The plugin id the watches of `look` are kept under; no plugin id starts
/// with `_`.
const LOOK: &str = "_look";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    #[default]
    Rows,
    Watch,
    Dump,
    Trace,
    Writes,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub format: u32,
    /// Echoed in the response, so the command knows it is the answer.
    pub id: String,
    #[serde(default)]
    pub mode: Mode,
    /// The query of `rows` and `watch`, the path of `dump`.
    #[serde(default)]
    pub query: String,
    /// How long `watch` and `trace` last, 1 to [`MOST_SECONDS`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u32>,
    /// How many bytes of each object `dump` reads, 1 to [`MOST_BYTES`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes: Option<u32>,
    /// The `function` and `location` entries `trace` counts.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<String>,
    /// For `dump`: from each object, the pointers to follow before reading,
    /// each at this offset of the object reached so far; for memory the map
    /// does not describe yet.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub follow: Vec<u32>,
    /// For `writes`: the field's offset in the object reached.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u32>,
    /// For `writes`: the field's size, 1, 2, 4, or 8 bytes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u8>,
}

/// Where writes `writes` watched came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Write {
    /// The address where the writing instruction ended.
    pub after: u64,
    /// Its offset in the game's executable, for `angelica exe disasm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset: Option<u64>,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Column {
    pub name: String,
    /// `<type>.<field>`.
    pub field: String,
    pub doc: String,
    pub domain: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Row {
    pub indexes: Vec<u32>,
    pub values: Vec<serde_json::Value>,
}

/// An object `dump` read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dumped {
    pub indexes: Vec<u32>,
    pub address: u64,
    /// Lowercase hexadecimal; `None` when the memory cannot be read.
    pub bytes: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    /// As `types.error-kind` names it: `not-found`, `unavailable`, …
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Response {
    pub format: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<Column>>,
    /// The rows of `rows`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Row>>,
    /// The rows of each firing of `watch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub firings: Option<Vec<Vec<Row>>>,
    /// Firings of `watch` left out past the event's rate.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dropped: Option<u32>,
    /// The objects of `dump`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub objects: Option<Vec<Dumped>>,
    /// The calls `trace` counted, by entry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub calls: Option<BTreeMap<String, u64>>,
    /// The address `writes` watched.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub watched: Option<u64>,
    /// Where writes came from, most first.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub writes: Option<Vec<Write>>,
    /// Writes from places past the ones `writes` tells apart.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub missed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Failure>,
}

impl Response {
    fn new(id: &str) -> Self {
        Self {
            format: 1,
            id: id.to_owned(),
            ..Self::default()
        }
    }

    fn failed(id: &str, error: &crate::error::Error) -> Self {
        Self {
            error: Some(Failure {
                kind: kind_name(error.kind).into(),
                message: error.message.clone(),
            }),
            ..Self::new(id)
        }
    }
}

/// A value as JSON: a number, true or false, the plain text, a sheet row
/// as `{"sheet", "row"}`, a name, or flags as `{"names", "bits"}`.
#[must_use]
pub fn json(value: Option<&Value>) -> serde_json::Value {
    use serde_json::json;
    match value {
        None => serde_json::Value::Null,
        Some(Value::Boolean(value)) => json!(value),
        Some(Value::Signed(value)) => json!(value),
        Some(Value::Unsigned(value)) => json!(value),
        Some(Value::Float(value)) => json!(value),
        Some(Value::Text(bytes)) => json!(angelica_text::sestring::plain(bytes)),
        Some(Value::Row { sheet, row }) => json!({"sheet": sheet, "row": row}),
        Some(Value::Named(name)) => json!(name),
        Some(Value::Flags { names, bits }) => json!({"names": names, "bits": bits}),
    }
}

/// The kind's name in `types.error-kind`.
fn kind_name(kind: crate::error::Kind) -> &'static str {
    use crate::error::Kind;
    match kind {
        Kind::InvalidArgument => "invalid-argument",
        Kind::NotFound => "not-found",
        Kind::PermissionDenied => "permission-denied",
        Kind::WrongContext => "wrong-context",
        Kind::Unavailable => "unavailable",
        Kind::LimitExceeded => "limit-exceeded",
        Kind::Io => "io",
        Kind::Network => "network",
    }
}

fn columns(prepared: &Prepared) -> Vec<Column> {
    prepared
        .columns
        .iter()
        .map(|column| Column {
            name: column.name.clone(),
            field: column.field.clone(),
            doc: column.doc.clone(),
            domain: column.domain.clone(),
        })
        .collect()
}

fn rows(rows: Vec<crate::lens::Row>) -> Vec<Row> {
    rows.into_iter()
        .map(|row| Row {
            indexes: row.indexes,
            values: row
                .values
                .iter()
                .map(|value| json(value.as_ref()))
                .collect(),
        })
        .collect()
}

/// The field `writes` watches: the one object the path and the pointers
/// followed reach, at the request's offset.
fn watched_address(services: &Services, request: &Request) -> crate::error::Result<u64> {
    let (schema, _) = services.lens.schema();
    let memory = services.game.memory();
    if request.follow.len() > MOST_FOLLOWS {
        return Err(crate::error::Error::invalid(format!(
            "a watch follows at most {MOST_FOLLOWS} pointers"
        )));
    }
    let prepared = prepare_objects(&schema, &request.query)?;
    let found = crate::lens::objects(memory, &prepared, 0)?;
    let [(_, object, _)] = found.as_slice() else {
        return Err(crate::error::Error::invalid(format!(
            "a watch of writes needs exactly one object; the path reaches {}",
            found.len()
        )));
    };
    follow(memory, *object, &request.follow)
        .and_then(|at| at.checked_add(u64::from(request.offset.unwrap_or(0))))
        .ok_or_else(|| {
            crate::error::Error::new(
                crate::error::Kind::Unavailable,
                "a pointer on the way is null or unreadable",
            )
        })
}

/// The address reached from `address` by reading a pointer at each offset
/// in turn; none when a pointer cannot be read or is null.
fn follow(memory: &dyn crate::lens::Memory, address: u64, offsets: &[u32]) -> Option<u64> {
    offsets.iter().try_fold(address, |at, offset| {
        let mut pointer = [0u8; 8];
        memory
            .read(at.checked_add(u64::from(*offset))?, &mut pointer)
            .then(|| u64::from_le_bytes(pointer))
            .filter(|pointer| *pointer != 0)
    })
}

fn read_bytes(memory: &dyn crate::lens::Memory, address: u64, bytes: usize) -> Option<Vec<u8>> {
    let mut buffer = vec![0u8; bytes];
    memory.read(address, &mut buffer).then_some(buffer)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::new(), |mut text, byte| {
        let _ = write!(text, "{byte:02x}");
        text
    })
}

/// Runs a `rows` or `dump` request against the game now.
#[must_use]
pub fn answer(services: &Services, request: &Request) -> Response {
    let (schema, _) = services.lens.schema();
    match request.mode {
        Mode::Rows => match prepare(&schema, &request.query, &|_| true)
            .and_then(|prepared| Ok((services.game.query(&prepared)?, prepared)))
        {
            Ok((found, prepared)) => Response {
                columns: Some(columns(&prepared)),
                rows: Some(rows(found)),
                ..Response::new(&request.id)
            },
            Err(error) => Response::failed(&request.id, &error),
        },
        Mode::Dump => {
            let bytes = request.bytes.unwrap_or(256).clamp(1, MOST_BYTES);
            let memory = services.game.memory();
            match prepare_objects(&schema, &request.query).and_then(|prepared| {
                if request.follow.len() > MOST_FOLLOWS {
                    return Err(crate::error::Error::invalid(format!(
                        "a dump follows at most {MOST_FOLLOWS} pointers"
                    )));
                }
                crate::lens::objects(memory, &prepared, bytes as usize)
            }) {
                Ok(found) => Response {
                    objects: Some(
                        found
                            .into_iter()
                            .map(|(indexes, address, read)| {
                                if request.follow.is_empty() {
                                    return Dumped {
                                        indexes,
                                        address,
                                        bytes: read.as_deref().map(hex),
                                    };
                                }
                                let reached = follow(memory, address, &request.follow);
                                Dumped {
                                    indexes,
                                    address: reached.unwrap_or(0),
                                    bytes: reached
                                        .and_then(|at| read_bytes(memory, at, bytes as usize))
                                        .as_deref()
                                        .map(hex),
                                }
                            })
                            .collect(),
                    ),
                    ..Response::new(&request.id)
                },
                Err(error) => Response::failed(&request.id, &error),
            }
        }
        Mode::Watch | Mode::Trace | Mode::Writes => Response::failed(
            &request.id,
            &crate::error::Error::invalid("a watch or a trace lasts: it is answered later"),
        ),
    }
}

/// Writes `bytes` to a sibling temporary file and renames it over `path`.
///
/// # Errors
/// The operating system's message.
pub fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, bytes).map_err(|e| format!("{}: {e}", temporary.display()))?;
    std::fs::rename(&temporary, path).map_err(|e| format!("{}: {e}", path.display()))
}

/// A watch or a trace that lasts.
enum Lasting {
    Watch {
        request: Request,
        handle: u32,
        columns: Vec<Column>,
        firings: Vec<Vec<Row>>,
        dropped: u32,
    },
    Trace {
        request: Request,
        points: Vec<(String, usize)>,
    },
    Writes {
        request: Request,
        address: u64,
    },
}

/// The host's side of the channel: one folder, looked at a few times a
/// second.
pub struct Look {
    folder: PathBuf,
    checked: Option<Instant>,
    lasting: Option<(Lasting, Instant)>,
    /// Handles of `look`'s watches, never reused.
    next: u32,
}

impl Look {
    const INTERVAL: Duration = Duration::from_millis(100);

    #[must_use]
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            checked: None,
            lasting: None,
            next: 0,
        }
    }

    /// Answers a waiting request, or goes on with a lasting one; between
    /// frames, on the game's main thread. Returns a line for the host's log
    /// when it answered or refused one.
    pub fn poll(&mut self, services: &Services) -> Option<String> {
        if self
            .checked
            .is_some_and(|checked| checked.elapsed() < Self::INTERVAL)
        {
            return None;
        }
        self.checked = Some(Instant::now());
        if self.lasting.is_some() {
            return self.go_on(services);
        }
        let path = self.folder.join(REQUEST);
        let size = std::fs::metadata(&path).ok()?.len();
        let bytes = std::fs::read(&path).ok();
        let _ = std::fs::remove_file(&path);
        let request = bytes
            .filter(|_| size <= MOST_REQUEST)
            .and_then(|bytes| serde_json::from_slice::<Request>(&bytes).ok())
            .filter(|request| request.format == 1);
        let Some(request) = request else {
            return Some("look: a request was not a format 1 request; it was removed".into());
        };
        match request.mode {
            Mode::Rows | Mode::Dump => {
                let response = answer(services, &request);
                Some(self.respond(&request, &response))
            }
            Mode::Watch => self.start_watch(services, request),
            Mode::Trace => self.start_trace(services, request),
            Mode::Writes => self.start_writes(services, request),
        }
    }

    fn until(request: &Request) -> Instant {
        Instant::now()
            + Duration::from_secs(u64::from(
                request.seconds.unwrap_or(10).clamp(1, MOST_SECONDS),
            ))
    }

    fn start_watch(&mut self, services: &Services, request: Request) -> Option<String> {
        let (schema, _) = services.lens.schema();
        let handle = self.next;
        self.next = self.next.wrapping_add(1);
        let started = prepare_watch(&schema, &request.query, &|_| true).and_then(|prepared| {
            let columns = columns(&prepared);
            services
                .lens
                .watch(
                    LOOK,
                    handle,
                    &request.query,
                    schema.objects.domains.keys().cloned().collect(),
                    prepared,
                    &|point, entry, address| services.game.enable_watch(point, entry, address),
                )
                .map(|()| columns)
        });
        match started {
            Ok(columns) => {
                let until = Self::until(&request);
                self.lasting = Some((
                    Lasting::Watch {
                        request,
                        handle,
                        columns,
                        firings: Vec::new(),
                        dropped: 0,
                    },
                    until,
                ));
                None
            }
            Err(error) => Some(self.respond(&request, &Response::failed(&request.id, &error))),
        }
    }

    fn start_trace(&mut self, services: &Services, request: Request) -> Option<String> {
        let (schema, _) = services.lens.schema();
        let mut points = Vec::new();
        for entry in request.entries.iter().take(MOST_ENTRIES) {
            let address = schema
                .code
                .get(entry)
                .cloned()
                .unwrap_or_else(|| Err("not a function or location of the map".into()));
            let hooked = address.and_then(|address| {
                let (point, hooked) = services.lens.watches.point(entry);
                if !hooked {
                    services.game.enable_watch(point, entry, address)?;
                    services.lens.watches.hooked(point);
                }
                Ok(point)
            });
            match hooked {
                Ok(point) => {
                    services.lens.watches.count(point);
                    points.push((entry.clone(), point));
                }
                Err(reason) => {
                    for (_, point) in &points {
                        services.lens.watches.counted(*point);
                    }
                    let error = crate::error::Error::new(
                        crate::error::Kind::Unavailable,
                        format!("{entry}: {reason}"),
                    );
                    return Some(self.respond(&request, &Response::failed(&request.id, &error)));
                }
            }
        }
        let until = Self::until(&request);
        self.lasting = Some((Lasting::Trace { request, points }, until));
        None
    }

    fn start_writes(&mut self, services: &Services, request: Request) -> Option<String> {
        match watched_address(services, &request).and_then(|address| {
            services
                .game
                .watch_writes(address, request.size.unwrap_or(4))
                .map(|()| address)
                .map_err(|reason| crate::error::Error::new(crate::error::Kind::Unavailable, reason))
        }) {
            Ok(address) => {
                let until = Self::until(&request);
                self.lasting = Some((Lasting::Writes { request, address }, until));
                None
            }
            Err(error) => Some(self.respond(&request, &Response::failed(&request.id, &error))),
        }
    }

    fn go_on(&mut self, services: &Services) -> Option<String> {
        let (mut lasting, until) = self.lasting.take()?;
        if let Lasting::Watch {
            firings, dropped, ..
        } = &mut lasting
        {
            for delivery in services.lens.watches.take(LOOK) {
                *dropped = dropped.saturating_add(delivery.dropped);
                firings.extend(delivery.firings.into_iter().map(rows));
            }
        }
        if Instant::now() < until {
            self.lasting = Some((lasting, until));
            return None;
        }
        match lasting {
            Lasting::Watch {
                request,
                handle,
                columns,
                firings,
                dropped,
            } => {
                services.lens.watches.unsubscribe(LOOK, handle);
                let response = Response {
                    columns: Some(columns),
                    firings: Some(firings),
                    dropped: Some(dropped),
                    ..Response::new(&request.id)
                };
                Some(self.respond(&request, &response))
            }
            Lasting::Trace { request, points } => {
                let calls = points
                    .into_iter()
                    .map(|(entry, point)| (entry, services.lens.watches.counted(point)))
                    .collect();
                let response = Response {
                    calls: Some(calls),
                    ..Response::new(&request.id)
                };
                Some(self.respond(&request, &response))
            }
            Lasting::Writes { request, address } => {
                let (written, missed) = services.game.take_writes();
                let mut writes: Vec<Write> = written
                    .into_iter()
                    .map(|place| Write {
                        after: place.after,
                        offset: place.offset,
                        count: place.count,
                    })
                    .collect();
                writes.sort_by(|a, b| b.count.cmp(&a.count).then(a.after.cmp(&b.after)));
                let response = Response {
                    watched: Some(address),
                    writes: Some(writes),
                    missed: Some(missed),
                    ..Response::new(&request.id)
                };
                Some(self.respond(&request, &response))
            }
        }
    }

    fn respond(&self, request: &Request, response: &Response) -> String {
        let what = match (&response.error, request.mode) {
            (Some(error), _) => error.message.clone(),
            (None, Mode::Rows) => format!("{} rows", response.rows.as_ref().map_or(0, Vec::len)),
            (None, Mode::Watch) => {
                format!("{} firings", response.firings.as_ref().map_or(0, Vec::len))
            }
            (None, Mode::Dump) => {
                format!("{} objects", response.objects.as_ref().map_or(0, Vec::len))
            }
            (None, Mode::Trace) => format!(
                "{} entries counted",
                response.calls.as_ref().map_or(0, BTreeMap::len)
            ),
            (None, Mode::Writes) => format!(
                "writes from {} places",
                response.writes.as_ref().map_or(0, Vec::len)
            ),
        };
        let bytes = serde_json::to_vec(response).unwrap_or_default();
        if let Err(error) = write_atomically(&self.folder.join(RESPONSE), &bytes) {
            return format!("look: the response was not written: {error}");
        }
        let subject = if request.mode == Mode::Trace {
            request.entries.join(", ")
        } else {
            request.query.clone()
        };
        format!("look: {subject}: {what}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Memory of a few 8-byte pointers; everything else unreadable.
    struct Pointers(BTreeMap<u64, u64>);

    impl crate::lens::Memory for Pointers {
        fn read(&self, address: u64, buffer: &mut [u8]) -> bool {
            match self.0.get(&address) {
                Some(value) if buffer.len() <= 8 => {
                    buffer.copy_from_slice(&value.to_le_bytes()[..buffer.len()]);
                    true
                }
                _ => false,
            }
        }
    }

    #[test]
    fn a_dump_follows_pointers_until_one_is_unreadable_or_null() {
        let memory = Pointers(BTreeMap::from([
            (0x1008, 0x2000),
            (0x2010, 0x3000),
            (0x3000, 0),
        ]));
        assert_eq!(follow(&memory, 0x1000, &[]), Some(0x1000));
        assert_eq!(follow(&memory, 0x1000, &[8]), Some(0x2000));
        assert_eq!(follow(&memory, 0x1000, &[8, 0x10]), Some(0x3000));
        assert_eq!(
            follow(&memory, 0x1000, &[8, 0x10, 0]),
            None,
            "a null pointer"
        );
        assert_eq!(follow(&memory, 0x1000, &[0x18]), None, "unreadable");
        assert_eq!(
            follow(&memory, u64::MAX, &[8]),
            None,
            "past the address space"
        );
    }
}
