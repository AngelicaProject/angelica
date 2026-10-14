//! `angelica look`: a Lens query from the command line, run in the game by
//! a development build of the host, for work on the game map. The command
//! writes `request.json` in the installation's `look` folder; the host
//! reads it between frames, runs the query on the game's main thread, and
//! writes `response.json`. Every domain is readable, as the player's own
//! tool on their own game; fields no plugin reads stay unread. A release
//! build never serves it.
#![forbid(unsafe_code)]
use crate::lens::{Value, prepare};
use crate::sandbox::Services;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

pub const REQUEST: &str = "request.json";
pub const RESPONSE: &str = "response.json";
/// The largest request the host reads.
const MOST_REQUEST: u64 = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub format: u32,
    /// Echoed in the response, so the command knows it is the answer.
    pub id: String,
    pub query: String,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Failure {
    /// As `types.error-kind` names it: `not-found`, `unavailable`, …
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub format: u32,
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<Vec<Column>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Row>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Failure>,
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

/// Runs a request against the game.
#[must_use]
pub fn answer(services: &Services, request: &Request) -> Response {
    let (schema, _) = services.lens.schema();
    let result = prepare(&schema, &request.query, &|_| true).and_then(|prepared| {
        let rows = services.game.query(&prepared)?;
        Ok((prepared, rows))
    });
    match result {
        Ok((prepared, rows)) => Response {
            format: 1,
            id: request.id.clone(),
            columns: Some(
                prepared
                    .columns
                    .into_iter()
                    .map(|column| Column {
                        name: column.name,
                        field: column.field,
                        doc: column.doc,
                        domain: column.domain,
                    })
                    .collect(),
            ),
            rows: Some(
                rows.into_iter()
                    .map(|row| Row {
                        indexes: row.indexes,
                        values: row
                            .values
                            .iter()
                            .map(|value| json(value.as_ref()))
                            .collect(),
                    })
                    .collect(),
            ),
            error: None,
        },
        Err(error) => Response {
            format: 1,
            id: request.id.clone(),
            columns: None,
            rows: None,
            error: Some(Failure {
                kind: kind_name(error.kind).into(),
                message: error.message,
            }),
        },
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

/// The host's side of the channel: one folder, looked at a few times a
/// second.
pub struct Look {
    folder: PathBuf,
    checked: Option<Instant>,
}

impl Look {
    const INTERVAL: Duration = Duration::from_millis(100);

    #[must_use]
    pub fn new(folder: PathBuf) -> Self {
        Self {
            folder,
            checked: None,
        }
    }

    /// Answers a waiting request; between frames, on the game's main
    /// thread. Returns a line for the host's log when it answered or
    /// refused one.
    pub fn poll(&mut self, services: &Services) -> Option<String> {
        if self
            .checked
            .is_some_and(|checked| checked.elapsed() < Self::INTERVAL)
        {
            return None;
        }
        self.checked = Some(Instant::now());
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
        let response = answer(services, &request);
        let note = match &response.error {
            Some(error) => format!("look: {}: {}", request.query, error.message),
            None => format!(
                "look: {}: {} rows",
                request.query,
                response.rows.as_ref().map_or(0, Vec::len)
            ),
        };
        let bytes = serde_json::to_vec(&response).unwrap_or_default();
        if let Err(error) = write_atomically(&self.folder.join(RESPONSE), &bytes) {
            return Some(format!("look: the response was not written: {error}"));
        }
        Some(note)
    }
}
