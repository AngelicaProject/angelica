//! What the library passes between its instances: the watches and rules in
//! the snapshot, and the rows and decisions in reports. Little-endian
//! bytes, each starting with a mark of the library, so the plugin's own
//! snapshot and reports pass beside them.
use angelica_sdk::api::intercepts::Value as CallValue;
use angelica_sdk::api::lens::{FlagSet, Row, SheetRow, Value};

const SNAPSHOT: &[u8; 4] = b"LNS1";
const ROWS: u8 = 0xA1;
const DECIDED: u8 = 0xA2;

/// How a function returns the outcome's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Returns {
    Nothing,
    Boolean,
    Signed,
    Unsigned,
    Pointer,
}

#[must_use]
pub fn kind(kind: angelica_map::ValueKind) -> Returns {
    use angelica_map::ValueKind;
    match kind {
        ValueKind::Void | ValueKind::Struct | ValueKind::F32 | ValueKind::F64 => Returns::Nothing,
        ValueKind::Bool => Returns::Boolean,
        ValueKind::I8 | ValueKind::I16 | ValueKind::I32 | ValueKind::I64 => Returns::Signed,
        ValueKind::U8 | ValueKind::U16 | ValueKind::U32 | ValueKind::U64 => Returns::Unsigned,
        ValueKind::Ptr => Returns::Pointer,
    }
}

/// The value a function returns for an outcome.
#[must_use]
pub fn returned(returns: u64, kind: Returns) -> CallValue {
    match kind {
        Returns::Nothing | Returns::Pointer => CallValue::None,
        Returns::Boolean => CallValue::Boolean(returns != 0),
        Returns::Signed => CallValue::Signed(returns.cast_signed()),
        Returns::Unsigned => CallValue::Unsigned(returns),
    }
}

/// A watch or a rule, as the snapshot carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub id: u32,
    /// The handler number of its function.
    pub handler: u32,
    pub query: String,
    /// For a rule: the outcome's value and how the function returns it.
    pub rule: Option<(u64, Returns)>,
}

#[derive(Default)]
struct Writer(Vec<u8>);

impl Writer {
    fn u8(&mut self, value: u8) {
        self.0.push(value);
    }
    fn u32(&mut self, value: u32) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn u64(&mut self, value: u64) {
        self.0.extend_from_slice(&value.to_le_bytes());
    }
    fn bytes(&mut self, value: &[u8]) {
        self.u32(u32::try_from(value.len()).unwrap_or(u32::MAX));
        self.0.extend_from_slice(value);
    }
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn take(&mut self, length: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(length)?;
        self.0 = rest;
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn bytes(&mut self) -> Option<Vec<u8>> {
        let length = usize::try_from(self.u32()?).ok()?;
        Some(self.take(length)?.to_vec())
    }
    fn string(&mut self) -> Option<String> {
        String::from_utf8(self.bytes()?).ok()
    }
}

fn returns_byte(kind: Returns) -> u8 {
    match kind {
        Returns::Nothing => 0,
        Returns::Boolean => 1,
        Returns::Signed => 2,
        Returns::Unsigned => 3,
        Returns::Pointer => 4,
    }
}

fn returns_of(byte: u8) -> Option<Returns> {
    Some(match byte {
        0 => Returns::Nothing,
        1 => Returns::Boolean,
        2 => Returns::Signed,
        3 => Returns::Unsigned,
        4 => Returns::Pointer,
        _ => return None,
    })
}

/// The snapshot of the library's `entries` and the plugin's `own` bytes.
#[must_use]
pub fn snapshot(entries: &[Entry], own: &[u8]) -> Vec<u8> {
    let mut writer = Writer::default();
    writer.0.extend_from_slice(SNAPSHOT);
    writer.u32(u32::try_from(entries.len()).unwrap_or(0));
    for entry in entries {
        writer.u32(entry.id);
        writer.u32(entry.handler);
        writer.bytes(entry.query.as_bytes());
        match entry.rule {
            None => writer.u8(0),
            Some((returns, kind)) => {
                writer.u8(1);
                writer.u64(returns);
                writer.u8(returns_byte(kind));
            }
        }
    }
    writer.bytes(own);
    writer.0
}

/// The entries and the plugin's own bytes of a snapshot; a snapshot of
/// the plugin alone, or none, has no entries.
#[must_use]
pub fn read_snapshot(bytes: &[u8]) -> Option<(Vec<Entry>, Vec<u8>)> {
    let Some(rest) = bytes.strip_prefix(SNAPSHOT) else {
        return Some((Vec::new(), bytes.to_vec()));
    };
    let mut reader = Reader(rest);
    let count = reader.u32()?;
    let mut entries = Vec::new();
    for _ in 0..count {
        let id = reader.u32()?;
        let handler = reader.u32()?;
        let query = reader.string()?;
        let rule = match reader.u8()? {
            0 => None,
            _ => Some((reader.u64()?, returns_of(reader.u8()?)?)),
        };
        entries.push(Entry {
            id,
            handler,
            query,
            rule,
        });
    }
    let own = reader.bytes()?;
    Some((entries, own))
}

/// What the intercept instance tells the main instance.
pub enum Report {
    Rows(u32, Vec<Row>),
    Decided(u32),
}

fn write_value(writer: &mut Writer, value: Option<&Value>) {
    match value {
        None => writer.u8(0),
        Some(Value::Boolean(flag)) => {
            writer.u8(1);
            writer.u8(u8::from(*flag));
        }
        Some(Value::Signed(number)) => {
            writer.u8(2);
            writer.u64(number.cast_unsigned());
        }
        Some(Value::Unsigned(number)) => {
            writer.u8(3);
            writer.u64(*number);
        }
        Some(Value::Float(number)) => {
            writer.u8(4);
            writer.u64(number.to_bits());
        }
        Some(Value::Text(text)) => {
            writer.u8(5);
            writer.bytes(text);
        }
        Some(Value::Sheet(row)) => {
            writer.u8(6);
            writer.bytes(row.sheet.as_bytes());
            writer.u32(row.row);
        }
        Some(Value::Named(name)) => {
            writer.u8(7);
            writer.bytes(name.as_bytes());
        }
        Some(Value::Flags(flags)) => {
            writer.u8(8);
            writer.u32(u32::try_from(flags.names.len()).unwrap_or(0));
            for name in &flags.names {
                writer.bytes(name.as_bytes());
            }
            writer.u64(flags.bits);
        }
    }
}

/// A value of a row: absent, or one.
enum Read {
    Absent,
    Value(Value),
}

fn read_value(reader: &mut Reader<'_>) -> Option<Read> {
    Some(Read::Value(match reader.u8()? {
        0 => return Some(Read::Absent),
        1 => Value::Boolean(reader.u8()? != 0),
        2 => Value::Signed(reader.u64()?.cast_signed()),
        3 => Value::Unsigned(reader.u64()?),
        4 => Value::Float(f64::from_bits(reader.u64()?)),
        5 => Value::Text(reader.bytes()?),
        6 => Value::Sheet(SheetRow {
            sheet: reader.string()?,
            row: reader.u32()?,
        }),
        7 => Value::Named(reader.string()?),
        8 => {
            let count = reader.u32()?;
            let names = (0..count)
                .map(|_| reader.string())
                .collect::<Option<Vec<_>>>()?;
            Value::Flags(FlagSet {
                names,
                bits: reader.u64()?,
            })
        }
        _ => return None,
    }))
}

impl Report {
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        let mut writer = Writer::default();
        match self {
            Self::Rows(id, rows) => {
                writer.u8(ROWS);
                writer.u32(*id);
                writer.u32(u32::try_from(rows.len()).unwrap_or(0));
                for row in rows {
                    writer.u32(u32::try_from(row.indexes.len()).unwrap_or(0));
                    for index in &row.indexes {
                        writer.u32(*index);
                    }
                    writer.u32(u32::try_from(row.values.len()).unwrap_or(0));
                    for value in &row.values {
                        write_value(&mut writer, value.as_ref());
                    }
                }
            }
            Self::Decided(id) => {
                writer.u8(DECIDED);
                writer.u32(*id);
            }
        }
        writer.0
    }

    /// The report the library made; none for one of the plugin's own.
    #[must_use]
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        let mut reader = Reader(bytes);
        match reader.u8()? {
            ROWS => {
                let id = reader.u32()?;
                let count = reader.u32()?;
                let mut rows = Vec::new();
                for _ in 0..count {
                    let indexes = (0..reader.u32()?)
                        .map(|_| reader.u32())
                        .collect::<Option<Vec<_>>>()?;
                    let values = (0..reader.u32()?)
                        .map(|_| {
                            read_value(&mut reader).map(|read| match read {
                                Read::Absent => None,
                                Read::Value(value) => Some(value),
                            })
                        })
                        .collect::<Option<Vec<_>>>()?;
                    rows.push(Row { indexes, values });
                }
                Some(Self::Rows(id, rows))
            }
            DECIDED => Some(Self::Decided(reader.u32()?)),
            _ => None,
        }
    }
}
