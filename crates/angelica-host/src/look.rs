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
//!   seconds, to find the function an action of the player calls.
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
            match prepare_objects(&schema, &request.query).and_then(|prepared| {
                crate::lens::objects(services.game.memory(), &prepared, bytes as usize)
            }) {
                Ok(found) => Response {
                    objects: Some(
                        found
                            .into_iter()
                            .map(|(indexes, address, bytes)| Dumped {
                                indexes,
                                address,
                                bytes: bytes.as_deref().map(hex),
                            })
                            .collect(),
                    ),
                    ..Response::new(&request.id)
                },
                Err(error) => Response::failed(&request.id, &error),
            }
        }
        Mode::Watch | Mode::Trace => Response::failed(
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
                self.respond(&request, &response)
            }
            Mode::Watch => self.start_watch(services, request),
            Mode::Trace => self.start_trace(services, request),
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
            Err(error) => self.respond(&request, &Response::failed(&request.id, &error)),
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
                    return self.respond(&request, &Response::failed(&request.id, &error));
                }
            }
        }
        let until = Self::until(&request);
        self.lasting = Some((Lasting::Trace { request, points }, until));
        None
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
                self.respond(&request, &response)
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
                self.respond(&request, &response)
            }
        }
    }

    #[allow(clippy::unnecessary_wraps)] // the same shape as `poll`
    fn respond(&self, request: &Request, response: &Response) -> Option<String> {
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
        };
        let bytes = serde_json::to_vec(response).unwrap_or_default();
        if let Err(error) = write_atomically(&self.folder.join(RESPONSE), &bytes) {
            return Some(format!("look: the response was not written: {error}"));
        }
        let subject = if request.mode == Mode::Trace {
            request.entries.join(", ")
        } else {
            request.query.clone()
        };
        Some(format!("look: {subject}: {what}"))
    }
}
