//! A query checked against the schema and turned into a plan of reads:
//! every name known, every domain on the way permitted, every place present
//! in the running build. Running the plan only reads memory.
#![forbid(unsafe_code)]
use super::query::{self, Filter, Literal, Op, Query, Test};
use super::schema::{RootAt, Schema};
use crate::error::{Error, Kind, Result};
use angelica_map::{Event, Field, Length, Meaning, Objects, Shape, ValueKind};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

/// The most fields one path goes through, with those of its conditions and
/// answers.
pub const MOST_DEPTH: usize = 16;

/// How a value is stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stored {
    Value(ValueKind),
    /// NUL-terminated, at most this many bytes, inline.
    Text(u16),
    /// One bit of an integer of this kind, as a `bool`.
    Bit {
        kind: ValueKind,
        index: u8,
    },
}

/// One step to another object: into an embedded object, or through the
/// pointer at `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hop {
    pub offset: u32,
    pub pointer: bool,
}

/// Where a value of an event's call is when the event fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgAt {
    /// A general-purpose register, by its position in [`super::Call`].
    Integer(u8),
    /// The low 64 bits of an xmm register.
    Float(u8),
    /// The 8 bytes at this offset from the stack pointer.
    Stack(u32),
}

/// What a read starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Base {
    /// The object the plan reached.
    Object,
    /// The object an argument of the call points to.
    Pointer(ArgAt),
    /// An argument of the call itself, read as the value: the bits of
    /// `mask`.
    Value { at: ArgAt, mask: u64 },
}

/// How a value is read from an object.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    pub base: Base,
    pub hops: Vec<Hop>,
    pub offset: u32,
    pub stored: Stored,
    /// Conditions the object must meet for the value to exist, such as
    /// being of the variant that has the field.
    pub guards: Vec<Condition>,
}

impl Read {
    /// The read starting from `base` instead, with its guards.
    pub(crate) fn rebase(&mut self, base: Base) {
        self.base = base;
        for guard in &mut self.guards {
            guard.read.rebase(base);
        }
    }
}

/// A number to compare with.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Number {
    Integer(i128),
    Float(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Number(Op, Number),
    NumberIn(Vec<Number>),
    /// Equal (or, when false, not equal) to one of these.
    Text(bool, Vec<Vec<u8>>),
}

/// A condition an object must meet.
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub read: Read,
    pub check: Check,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Count {
    Fixed(u16),
    /// A value of the object holding the array, at most `most`.
    Field {
        read: Read,
        most: u16,
    },
}

/// One array on the path: each element that passes becomes rows.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrayStep {
    /// The array's place in the object holding it.
    pub offset: u32,
    pub behind_pointer: bool,
    pub stride: u32,
    pub pointers: bool,
    pub count: Count,
    /// A value of the element whose zero, false, or empty skips it.
    pub present: Option<Read>,
    pub index: Option<u32>,
    pub conditions: Vec<Condition>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// To the object an argument of the call points to.
    FromArg(ArgAt),
    Hop(Hop),
    Array(ArrayStep),
    /// Conditions on the object reached, which is skipped if one fails.
    Filter(Vec<Condition>),
}

/// What a column's number means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ColumnMeaning {
    Plain,
    Sheet(String),
    /// Names by value.
    Enum(BTreeMap<u64, String>),
    /// Names by bit.
    Flags(BTreeMap<u64, String>),
}

/// One value of each row.
#[derive(Debug, Clone, PartialEq)]
pub struct Column {
    pub name: String,
    pub doc: String,
    /// The field's type and field name, `<type>.<field>`.
    pub field: String,
    pub stored: Stored,
    pub meaning: ColumnMeaning,
    pub unit: Option<String>,
    pub domain: String,
    pub(crate) read: Read,
}

/// Where a plan starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// The object of a root.
    Root(RootAt),
    /// The call of an event, when it fires.
    Event(String),
}

/// A query ready to run in the build its schema describes.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub columns: Vec<Column>,
    /// Every domain the query reads.
    pub domains: BTreeSet<String>,
    pub(crate) start: Start,
    pub(crate) steps: Vec<Step>,
}

impl Prepared {
    /// The event a watch reads from; `None` for a query.
    #[must_use]
    pub fn event(&self) -> Option<&str> {
        match &self.start {
            Start::Event(event) => Some(event),
            Start::Root(_) => None,
        }
    }
}

/// What a read value is: its shape, description, domain, and qualified name.
struct Described {
    shape: Shape,
    doc: String,
    domain: String,
    name: String,
}

struct Planner<'a> {
    objects: &'a Objects,
    schema: &'a Schema,
    permitted: &'a dyn Fn(&str) -> bool,
    domains: BTreeSet<String>,
    /// Domains whose fields an earlier partitioned array already answered
    /// for: below it, those fields are read only in elements of permitted
    /// domains.
    covered: BTreeSet<String>,
    depth: usize,
    /// Whether the query is a watch, which starts at an event.
    watch: bool,
    /// Whether the path may end at an object, without values to answer.
    objects_only: bool,
    /// The type the path ended at, once planned.
    reached: Option<String>,
    /// The text a condition on the path fixed for a text field, by the
    /// field's `<type>.<field>`: the window whose elements are named.
    fixed: BTreeMap<String, String>,
}

fn not_found(message: String) -> Error {
    Error::new(Kind::NotFound, message)
}

impl<'a> Planner<'a> {
    /// A field of `type_name`, checked: known, not forbidden, permitted,
    /// present in the build. Returns the field and its offset.
    fn field(&mut self, type_name: &str, name: &str) -> Result<(&'a Field, u32)> {
        let objects: &'a Objects = self.objects;
        let Some((_, field)) = objects.field(type_name, name) else {
            let known = objects
                .fields(type_name)
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(not_found(format!(
                "{type_name} has no field {name}; its fields: {known}"
            )));
        };
        self.depth += 1;
        if self.depth > MOST_DEPTH {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a query goes through at most {MOST_DEPTH} fields"),
            ));
        }
        if field.forbidden {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!("{type_name}.{name} is never read by a plugin"),
            ));
        }
        // A partitioned array answers for its domains by its elements.
        let partitioned =
            matches!(&field.shape, Shape::Array { array, .. } if array.partition.is_some());
        if !partitioned {
            self.domain(&field.domain)?;
        }
        let offset = self.schema.offset(&field.at).map_err(|reason| {
            Error::new(
                Kind::Unavailable,
                format!("{type_name}.{name} is not available in this game build: {reason}"),
            )
        })?;
        Ok((field, offset))
    }

    /// A field of `type_name`, or of a variant of it: then also the
    /// variant's type and the condition an object meets to be of it.
    fn lookup(
        &mut self,
        type_name: &str,
        name: &str,
    ) -> Result<(&'a Field, u32, String, Option<Condition>)> {
        let objects: &'a Objects = self.objects;
        if let Some((owner, _)) = objects.field(type_name, name) {
            let (field, offset) = self.field(type_name, name)?;
            return Ok((field, offset, owner.name.clone(), None));
        }
        let Some((variant_type, variant, _)) = objects.variant_field(type_name, name) else {
            // The error names the type's own fields.
            let (field, offset) = self.field(type_name, name)?;
            return Ok((field, offset, type_name.to_owned(), None));
        };
        let Some((_, kind_field)) = objects.field(type_name, &variant.field) else {
            unreachable!("the map checks a variant's field");
        };
        let Shape::Value {
            kind,
            meaning: Meaning::Enum(enum_name),
            ..
        } = &kind_field.shape
        else {
            unreachable!("the map checks that a variant's field is an enum value");
        };
        let kind_offset = self.schema.offset(&kind_field.at).map_err(|reason| {
            Error::new(
                Kind::Unavailable,
                format!(
                    "{type_name}.{} is not available in this game build: {reason}",
                    variant.field
                ),
            )
        })?;
        let values = &objects.enums[enum_name].values;
        let guard = Condition {
            read: Read {
                base: Base::Object,
                hops: Vec::new(),
                offset: kind_offset,
                stored: Stored::Value(*kind),
                guards: Vec::new(),
            },
            check: Check::NumberIn(
                variant
                    .values
                    .iter()
                    .map(|value| Number::Integer(i128::from(values[value])))
                    .collect(),
            ),
        };
        let (field, offset) = self.field(&variant_type.name, name)?;
        Ok((field, offset, variant_type.name.clone(), Some(guard)))
    }

    fn domain(&mut self, domain: &str) -> Result<()> {
        if self.covered.contains(domain) {
            return Ok(());
        }
        if !(self.permitted)(domain) {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!(
                    "the query reads {domain}, which the plugin may not read: its manifest lists the domains it reads in [lens] read"
                ),
            ));
        }
        self.domains.insert(domain.to_owned());
        Ok(())
    }

    /// A value reached from `type_name` by `path`: references and embedded
    /// objects, then a value or text.
    fn value(&mut self, type_name: &str, path: &[String]) -> Result<(Read, Described)> {
        let mut current = type_name.to_owned();
        let mut hops = Vec::new();
        let mut guards = Vec::new();
        for (position, name) in path.iter().enumerate() {
            // The type that declares the field, which may be a base of
            // `current` or a variant of it.
            let (field, offset, owner, guard) = self.lookup(&current, name)?;
            if let Some(mut guard) = guard {
                guard.read.hops.clone_from(&hops);
                guards.push(guard);
            }
            let last = position + 1 == path.len();
            match (&field.shape, last) {
                (Shape::Value { kind, .. }, true) => {
                    let stored = field.bit.map_or(Stored::Value(*kind), |bit| Stored::Bit {
                        kind: bit.kind,
                        index: bit.index,
                    });
                    return Ok((
                        Read {
                            base: Base::Object,
                            hops,
                            offset,
                            stored,
                            guards,
                        },
                        described(field, format!("{owner}.{name}")),
                    ));
                }
                (Shape::Text(length), true) => {
                    let offset = self.text_offset(field, offset, &mut hops)?;
                    return Ok((
                        Read {
                            base: Base::Object,
                            hops,
                            offset,
                            stored: Stored::Text(*length),
                            guards,
                        },
                        described(field, format!("{owner}.{name}")),
                    ));
                }
                (Shape::Ref(to), false) => {
                    hops.push(Hop {
                        offset,
                        pointer: true,
                    });
                    current.clone_from(to);
                }
                (Shape::Embed(of), false) => {
                    hops.push(Hop {
                        offset,
                        pointer: false,
                    });
                    current.clone_from(of);
                }
                (Shape::Array { .. }, _) => {
                    return Err(Error::invalid(format!(
                        "{current}.{name} is an array: put it on the path, before the braces, to get a row per element"
                    )));
                }
                (Shape::Value { .. } | Shape::Text(_), false) => {
                    return Err(Error::invalid(format!(
                        "{current}.{name} is a value, and a path goes on only through references and objects"
                    )));
                }
                (Shape::Ref(_) | Shape::Embed(_), true) => {
                    return Err(Error::invalid(format!(
                        "{current}.{name} is an object; name its values, such as {name}.<field>"
                    )));
                }
            }
        }
        Err(Error::invalid("a value needs a field name"))
    }

    /// Where a text field's text is read from its object at `offset`:
    /// behind the pointer of its entry, behind the pointer in the structure
    /// there, or in place.
    fn text_offset(&self, field: &Field, offset: u32, hops: &mut Vec<Hop>) -> Result<u32> {
        if let Some(pointer) = &field.pointer {
            let inner = self.schema.offset(pointer).map_err(|reason| {
                Error::new(
                    Kind::Unavailable,
                    format!(
                        "{} is not available in this game build: {reason}",
                        field.name
                    ),
                )
            })?;
            hops.push(Hop {
                offset: offset.saturating_add(inner),
                pointer: true,
            });
            return Ok(0);
        }
        if self.schema.text_pointers.contains(&field.at) {
            hops.push(Hop {
                offset,
                pointer: true,
            });
            return Ok(0);
        }
        Ok(offset)
    }

    /// The window whose elements a field with element names names: the one
    /// a condition earlier on the path fixed.
    fn window(&self, window_field: &str, name: &str) -> Result<String> {
        self.fixed.get(window_field).cloned().ok_or_else(|| {
            let field = window_field.rsplit('.').next().unwrap_or(window_field);
            Error::invalid(format!(
                "{name} names elements of a window: name the window earlier on the path, such as [{field} = \"_Exp\"]"
            ))
        })
    }

    fn condition(&mut self, scope: Scope<'_>, condition: &query::Condition) -> Result<Condition> {
        let (read, described) = self.scoped(scope, &condition.field)?;
        let name = described.name;
        let literals = match &condition.test {
            Test::Compare(_, literal) => std::slice::from_ref(literal),
            Test::In(literals) => literals.as_slice(),
        };
        let wrong = |what: &str| Err(Error::invalid(format!("{name} {what}")));
        if let Stored::Text(_) = read.stored {
            let mut texts = Vec::new();
            for literal in literals {
                match literal {
                    Literal::Text(text) => texts.push(text.as_bytes().to_vec()),
                    _ => return wrong("is text: compare it with \"text\""),
                }
            }
            let equal = match condition.test {
                Test::Compare(Op::Equal, _) | Test::In(_) => true,
                Test::Compare(Op::NotEqual, _) => false,
                Test::Compare(..) => return wrong("is text: compare it with = or !="),
            };
            if equal && let [text] = texts.as_slice() {
                self.fixed
                    .insert(name.clone(), String::from_utf8_lossy(text).into_owned());
            }
            return Ok(Condition {
                read,
                check: Check::Text(equal, texts),
            });
        }
        let Shape::Value { kind, meaning, .. } = &described.shape else {
            unreachable!("value() returns values and text");
        };
        let mut numbers = Vec::new();
        for literal in literals {
            let number = match (literal, meaning) {
                (Literal::Name(value), Meaning::Enum(enum_name) | Meaning::Flags(enum_name)) => {
                    let objects: &'a Objects = self.objects;
                    let values = &objects.enums[enum_name].values;
                    let Some(number) = values.get(value) else {
                        let known = values.keys().cloned().collect::<Vec<_>>().join(", ");
                        return wrong(&format!("has no value {value}; its values: {known}"));
                    };
                    Number::Integer(i128::from(*number))
                }
                (Literal::Name(element) | Literal::Text(element), Meaning::Element(window)) => {
                    let window = self.window(window, &name)?;
                    Number::Integer(i128::from(self.schema.element(&window, element)?))
                }
                (Literal::Name(value), _) => {
                    return wrong(&format!("has no named values, so {value} means nothing"));
                }
                (Literal::Boolean(value), _) if *kind == ValueKind::Bool => {
                    Number::Integer(i128::from(*value))
                }
                (Literal::Boolean(_), _) => return wrong("is not true or false"),
                (Literal::Integer(value), _) if *kind != ValueKind::Bool => Number::Integer(*value),
                (Literal::Float(value), _) if matches!(kind, ValueKind::F32 | ValueKind::F64) => {
                    Number::Float(*value)
                }
                (Literal::Integer(_) | Literal::Float(_), _) if *kind == ValueKind::Bool => {
                    return wrong("is true or false");
                }
                (Literal::Float(_), _) => return wrong("is a whole number"),
                (Literal::Text(_), _) => return wrong("is a number, not text"),
                (Literal::Integer(_), _) => unreachable!("covered above"),
            };
            numbers.push(number);
        }
        let ordered =
            matches!(meaning, Meaning::Plain | Meaning::Sheet(_)) && *kind != ValueKind::Bool;
        let check = match &condition.test {
            Test::In(_) => Check::NumberIn(numbers),
            Test::Compare(op @ (Op::Equal | Op::NotEqual), _) => Check::Number(*op, numbers[0]),
            Test::Compare(op, _) if ordered => Check::Number(*op, numbers[0]),
            Test::Compare(..) => return wrong("has no order: compare it with =, !=, or in"),
        };
        Ok(Condition { read, check })
    }

    fn conditions(
        &mut self,
        scope: Scope<'_>,
        conditions: &[query::Condition],
    ) -> Result<Vec<Condition>> {
        conditions
            .iter()
            .map(|condition| self.condition(scope, condition))
            .collect()
    }

    /// A value in `scope`: of a type, or of an event's call.
    fn scoped(&mut self, scope: Scope<'_>, path: &[String]) -> Result<(Read, Described)> {
        match scope {
            Scope::Type(type_name) => self.value(type_name, path),
            Scope::Event(event) => self.argument(event, path),
        }
    }

    /// A value of an event's call: an argument, text behind one, or a
    /// value reached through the object one points to.
    fn argument(&mut self, event: &Event, path: &[String]) -> Result<(Read, Described)> {
        let Some((first, rest)) = path.split_first() else {
            return Err(Error::invalid("a value needs a field name"));
        };
        let Some(arg) = event.args.get(first) else {
            let known = event.args.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(not_found(format!(
                "{} has no argument {first}; its arguments: {known}",
                event.name
            )));
        };
        self.depth += 1;
        if arg.forbidden {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!("{}.{first} is never read by a plugin", event.name),
            ));
        }
        self.domain(&arg.domain)?;
        let at = arg_at(arg);
        let name = format!("{}.{first}", event.name);
        let own = || Described {
            shape: arg.shape.clone(),
            doc: arg.doc.clone(),
            domain: arg.domain.clone(),
            name: name.clone(),
        };
        match (&arg.shape, rest.is_empty()) {
            (Shape::Value { kind, .. }, true) => Ok((
                Read {
                    base: Base::Value {
                        at,
                        mask: arg.mask.unwrap_or(u64::MAX),
                    },
                    hops: Vec::new(),
                    offset: 0,
                    stored: Stored::Value(*kind),
                    guards: Vec::new(),
                },
                own(),
            )),
            (Shape::Text(length), true) => Ok((
                Read {
                    base: Base::Pointer(at),
                    hops: Vec::new(),
                    offset: 0,
                    stored: Stored::Text(*length),
                    guards: Vec::new(),
                },
                own(),
            )),
            (Shape::Ref(to), false) => {
                let (mut read, described) = self.value(to, rest)?;
                read.rebase(Base::Pointer(at));
                Ok((read, described))
            }
            (Shape::Ref(_), true) => Err(Error::invalid(format!(
                "{name} is an object; name its values, such as {first}.<field>"
            ))),
            _ => Err(Error::invalid(format!(
                "{name} is a value, and a path goes on only through references and objects"
            ))),
        }
    }

    fn column(&mut self, scope: Scope<'_>, name: &str, path: &[String]) -> Result<Column> {
        let (read, described) = self.scoped(scope, path)?;
        let qualified = described.name.clone();
        let (meaning, unit) = match &described.shape {
            Shape::Value { meaning, unit, .. } => {
                let objects: &'a Objects = self.objects;
                let names = |enum_name: &str| {
                    objects.enums[enum_name]
                        .values
                        .iter()
                        .map(|(name, value)| (*value, name.clone()))
                        .collect()
                };
                let meaning = match meaning {
                    Meaning::Plain => ColumnMeaning::Plain,
                    Meaning::Sheet(sheet) => ColumnMeaning::Sheet(sheet.clone()),
                    Meaning::Enum(name) => ColumnMeaning::Enum(names(name)),
                    Meaning::Flags(name) => ColumnMeaning::Flags(names(name)),
                    Meaning::Element(window) => self
                        .fixed
                        .get(window)
                        .and_then(|window| self.schema.windows.get(window))
                        .and_then(|named| named.as_ref().ok())
                        .map_or(ColumnMeaning::Plain, |named| {
                            ColumnMeaning::Enum(
                                named
                                    .iter()
                                    .map(|(name, id)| (u64::from(*id), name.clone()))
                                    .collect(),
                            )
                        }),
                };
                (meaning, unit.clone())
            }
            _ => (ColumnMeaning::Plain, None),
        };
        Ok(Column {
            name: name.to_owned(),
            doc: described.doc,
            field: qualified,
            stored: read.stored,
            meaning,
            unit,
            domain: described.domain,
            read,
        })
    }

    /// The root of a query, checked: known, permitted, present.
    fn root(&mut self, name: &str) -> Result<(&'a angelica_map::Root, RootAt)> {
        let objects: &'a Objects = self.objects;
        let Some(root) = objects.roots.get(name) else {
            let known = objects.roots.keys().cloned().collect::<Vec<_>>().join(", ");
            let mut message = format!("the game map has no root {name}");
            if !known.is_empty() {
                let _ = write!(message, "; its roots: {known}");
            }
            return Err(not_found(message));
        };
        let at = self
            .schema
            .roots
            .get(&root.name)
            .cloned()
            .unwrap_or_else(|| Err("the root is not in the schema".into()))
            .map_err(|reason| {
                Error::new(
                    Kind::Unavailable,
                    format!(
                        "root {} is not available in this game build: {reason}",
                        root.name
                    ),
                )
            })?;
        Ok((root, at))
    }

    /// An array on the path, held by an object of `holder`.
    fn array(
        &mut self,
        holder: &str,
        segment: &query::Segment,
        offset: u32,
        array: &angelica_map::Array,
        behind_pointer: bool,
        domain: &str,
    ) -> Result<ArrayStep> {
        let partition = array
            .partition
            .as_ref()
            .map(|partition| self.partition(segment, array, partition, domain))
            .transpose()?;
        let count = match &array.length {
            Length::Fixed(length) => Count::Fixed(*length),
            Length::Field { field, most } => {
                let (read, ..) = self.value(holder, std::slice::from_ref(field))?;
                Count::Field { read, most: *most }
            }
        };
        let present = array
            .present
            .as_ref()
            .map(|name| {
                self.value(&array.of, std::slice::from_ref(name))
                    .map(|(read, ..)| read)
            })
            .transpose()?;
        let (index, conditions) = match &segment.filter {
            None => (None, Vec::new()),
            Some(Filter::Index(index)) => {
                let (Length::Fixed(length) | Length::Field { most: length, .. }) = array.length;
                if *index >= u32::from(length) {
                    return Err(Error::invalid(format!(
                        "{holder}.{} has at most {length} elements: index {index} is past them",
                        segment.field
                    )));
                }
                (Some(*index), Vec::new())
            }
            Some(Filter::Conditions(conditions)) => {
                (None, self.conditions(Scope::Type(&array.of), conditions)?)
            }
        };
        let conditions = partition.into_iter().chain(conditions).collect();
        Ok(ArrayStep {
            offset,
            behind_pointer,
            stride: array.stride,
            pointers: array.pointers,
            count,
            present,
            index,
            conditions,
        })
    }

    /// The condition a partitioned array's elements must meet: their value
    /// of the partition's field is named and its domain permitted. A filter
    /// on that field with `=` or `in` names the values the query reads, each
    /// of which must be permitted; without one, the query reads the elements
    /// of every permitted domain.
    fn partition(
        &mut self,
        segment: &query::Segment,
        array: &angelica_map::Array,
        partition: &angelica_map::Partition,
        default: &str,
    ) -> Result<Condition> {
        let objects: &'a Objects = self.objects;
        // The partition's field is read whatever its domain: it only says
        // which elements the plugin may read.
        let Some((_, field)) = objects.field(&array.of, &partition.field) else {
            unreachable!("the map checks that a partition's field exists");
        };
        let name = format!("{}.{}", array.of, partition.field);
        let offset = self.schema.offset(&field.at).map_err(|reason| {
            Error::new(
                Kind::Unavailable,
                format!("{name} is not available in this game build: {reason}"),
            )
        })?;
        let Shape::Value {
            kind,
            meaning: Meaning::Enum(enum_name),
            ..
        } = &field.shape
        else {
            unreachable!("the map checks that a partition's field is an enum value");
        };
        let read = Read {
            base: Base::Object,
            hops: Vec::new(),
            offset,
            stored: Stored::Value(*kind),
            guards: Vec::new(),
        };
        let values = &objects.enums[enum_name].values;
        let domain_of = |value: &str| {
            partition
                .domains
                .get(value)
                .map_or(default, String::as_str)
                .to_owned()
        };
        let named = |literal: &Literal| -> Result<String> {
            match literal {
                Literal::Name(value) if values.contains_key(value) => Ok(value.clone()),
                Literal::Integer(number) => values
                    .iter()
                    .find(|(_, value)| i128::from(**value) == *number)
                    .map(|(value, _)| value.clone())
                    .ok_or_else(|| {
                        Error::invalid(format!(
                            "{name} {number} has no name, and only elements with a named {} are read",
                            partition.field
                        ))
                    }),
                _ => Err(Error::invalid(format!(
                    "{name} has no value {literal:?}"
                ))),
            }
        };
        let mut chosen = None;
        if let Some(Filter::Conditions(conditions)) = &segment.filter {
            for condition in conditions {
                if condition.field != [partition.field.clone()] {
                    continue;
                }
                match &condition.test {
                    Test::Compare(Op::Equal, literal) => chosen = Some(vec![named(literal)?]),
                    Test::In(literals) => {
                        chosen = Some(literals.iter().map(named).collect::<Result<_>>()?);
                    }
                    Test::Compare(..) => {}
                }
            }
        }
        // Without a filter on the field, the elements of every permitted
        // domain.
        let allowed: Vec<String> = chosen.unwrap_or_else(|| {
            values
                .keys()
                .filter(|value| {
                    let domain = domain_of(value);
                    self.covered.contains(&domain) || (self.permitted)(&domain)
                })
                .cloned()
                .collect()
        });
        if allowed.is_empty() {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!(
                    "{name}: the plugin may read none of its domains: its manifest lists the domains it reads in [lens] read"
                ),
            ));
        }
        for value in &allowed {
            self.domain(&domain_of(value))?;
        }
        self.covered.insert(default.to_owned());
        Ok(Condition {
            read,
            check: Check::NumberIn(
                allowed
                    .iter()
                    .map(|value| Number::Integer(i128::from(values[value])))
                    .collect(),
            ),
        })
    }

    /// The start of a watch: the event, checked, its filter, and the first
    /// field of the path when there is one. Returns the type the path
    /// reached, or `None` while it is still at the event.
    fn event_start(
        &mut self,
        event: &'a Event,
        query: &Query,
        steps: &mut Vec<Step>,
        columns: &mut Vec<Column>,
    ) -> Result<Option<String>> {
        if event.derived.is_some() {
            return Err(Error::invalid(format!(
                "{} is derived by the host: give rules there with lens.decide or lens.decide-file; it is never watched",
                event.name
            )));
        }
        self.domain(&event.domain)?;
        if let Err(reason) = self
            .schema
            .events
            .get(&event.name)
            .cloned()
            .unwrap_or_else(|| Err("the event is not in the schema".into()))
        {
            return Err(Error::new(
                Kind::Unavailable,
                format!(
                    "event {} is not available in this game build: {reason}",
                    event.name
                ),
            ));
        }
        match &query.filter {
            None => {}
            Some(Filter::Index(_)) => {
                return Err(Error::invalid(format!(
                    "{} is an event, not an array: it has no index",
                    event.name
                )));
            }
            Some(Filter::Conditions(conditions)) => {
                let conditions = self.conditions(Scope::Event(event), conditions)?;
                steps.push(Step::Filter(conditions));
            }
        }
        let Some(segment) = query.path.first() else {
            return Ok(None);
        };
        if query.path.len() == 1 && query.select.is_none() {
            if segment.filter.is_some() {
                return Err(Error::invalid(format!(
                    "{}.{} is a value: it has no filter",
                    event.name, segment.field
                )));
            }
            columns.push(self.column(
                Scope::Event(event),
                &segment.field,
                std::slice::from_ref(&segment.field),
            )?);
            return Ok(None);
        }
        let arg = event.args.get(&segment.field);
        let Some(arg) = arg.filter(|arg| matches!(arg.shape, Shape::Ref(_))) else {
            return Err(Error::invalid(format!(
                "{}.{} is not a reference to an object: a path from an event goes on only through one",
                event.name, segment.field
            )));
        };
        let Shape::Ref(to) = &arg.shape else {
            unreachable!("filtered above");
        };
        self.depth += 1;
        self.domain(&arg.domain)?;
        steps.push(Step::FromArg(arg_at(arg)));
        let name = format!("{}.{}", event.name, segment.field);
        self.one_object_filter(&name, segment.filter.as_ref(), to, steps)?;
        Ok(Some(to.clone()))
    }

    /// The filter on one object, which can have conditions but not an
    /// index. `name` says which object for the error.
    fn one_object_filter(
        &mut self,
        name: &str,
        filter: Option<&Filter>,
        of: &str,
        steps: &mut Vec<Step>,
    ) -> Result<()> {
        match filter {
            None => {}
            Some(Filter::Index(_)) => {
                return Err(Error::invalid(format!(
                    "{name} is one object, not an array: it has no index"
                )));
            }
            Some(Filter::Conditions(conditions)) => {
                let conditions = self.conditions(Scope::Type(of), conditions)?;
                steps.push(Step::Filter(conditions));
            }
        }
        Ok(())
    }

    /// Where the plan starts, the type it reached (`None` while a watch is
    /// still at its event), and how many path segments that took.
    fn start(
        &mut self,
        query: &Query,
        steps: &mut Vec<Step>,
        columns: &mut Vec<Column>,
    ) -> Result<(Start, Option<String>, usize)> {
        let objects = self.objects;
        if let Some(event) = objects.events.get(&query.root) {
            if !self.watch {
                return Err(Error::invalid(format!(
                    "{} is an event: watch it with lens.watch",
                    query.root
                )));
            }
            let reached = self.event_start(event, query, steps, columns)?;
            let skip = if reached.is_some() {
                1
            } else {
                query.path.len()
            };
            return Ok((Start::Event(event.name.clone()), reached, skip));
        }
        if self.watch {
            let known = objects
                .events
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let mut message = format!("the game map has no event {}", query.root);
            if !known.is_empty() {
                let _ = write!(message, "; its events: {known}");
            }
            return Err(not_found(message));
        }
        let (root, at) = self.root(&query.root)?;
        self.one_object_filter(&root.name, query.filter.as_ref(), &root.r#type, steps)?;
        Ok((Start::Root(at), Some(root.r#type.clone()), 0))
    }

    /// The columns of a watch whose answers are the event's own arguments.
    fn event_columns(&mut self, query: &Query, columns: &mut Vec<Column>) -> Result<()> {
        let event = &self.objects.events[&query.root];
        if let Some(selected) = &query.select {
            for selected in selected {
                columns.push(self.column(Scope::Event(event), &selected.name, &selected.path)?);
            }
        } else if columns.is_empty() && !self.objects_only {
            let known = event.args.keys().cloned().collect::<Vec<_>>().join(", ");
            return Err(Error::invalid(format!(
                "name the values to answer with in braces, such as {{ a, b }}; the arguments of {}: {known}",
                event.name
            )));
        }
        Ok(())
    }

    /// One segment of the path from an object of type `holder`. Returns the
    /// type the path reached. `answer` is set when the segment is the last
    /// one and nothing is selected, so a value there is the one column.
    fn segment(
        &mut self,
        holder: &str,
        segment: &query::Segment,
        answer: bool,
        steps: &mut Vec<Step>,
        columns: &mut Vec<Column>,
    ) -> Result<String> {
        // A field of a variant reaches only the objects of the variant.
        let (field, offset, owner, guard) = self.lookup(holder, &segment.field)?;
        if let Some(guard) = guard {
            steps.push(Step::Filter(vec![guard]));
        }
        let holder = if self.objects.types[&owner].variant.is_some() {
            owner.as_str()
        } else {
            holder
        };
        match &field.shape {
            Shape::Ref(to) | Shape::Embed(to) => {
                steps.push(Step::Hop(Hop {
                    offset,
                    pointer: matches!(field.shape, Shape::Ref(_)),
                }));
                let name = format!("{holder}.{}", segment.field);
                self.one_object_filter(&name, segment.filter.as_ref(), to, steps)?;
                Ok(to.clone())
            }
            Shape::Array {
                array,
                behind_pointer,
            } => {
                let step = self.array(
                    holder,
                    segment,
                    offset,
                    array,
                    *behind_pointer,
                    &field.domain,
                )?;
                steps.push(Step::Array(step));
                Ok(array.of.clone())
            }
            Shape::Value { .. } | Shape::Text(_) if answer => {
                if segment.filter.is_some() {
                    return Err(Error::invalid(format!(
                        "{holder}.{} is a value: it has no filter",
                        segment.field
                    )));
                }
                // The value is the one column; the plan reads it from the
                // object holding it.
                self.depth -= 1;
                columns.push(self.column(
                    Scope::Type(holder),
                    &segment.field,
                    std::slice::from_ref(&segment.field),
                )?);
                Ok(holder.to_owned())
            }
            Shape::Value { .. } | Shape::Text(_) => Err(Error::invalid(format!(
                "{holder}.{} is a value: a path goes on only through references, objects, and arrays",
                segment.field
            ))),
        }
    }

    /// The columns selected in braces from the object the path ended at.
    fn selected(&mut self, query: &Query, current: &str, columns: &mut Vec<Column>) -> Result<()> {
        match &query.select {
            Some(selected) => {
                for selected in selected {
                    columns.push(self.column(
                        Scope::Type(current),
                        &selected.name,
                        &selected.path,
                    )?);
                }
            }
            None if columns.is_empty() && !self.objects_only => {
                let known = self
                    .objects
                    .fields(current)
                    .keys()
                    .copied()
                    .collect::<Vec<_>>()
                    .join(", ");
                return Err(Error::invalid(format!(
                    "the path ends at {current}: name the values to answer with in braces, such as {{ a, b }}; its fields: {known}"
                )));
            }
            None => {}
        }
        Ok(())
    }

    fn plan(&mut self, query: &Query) -> Result<Prepared> {
        let mut steps = Vec::new();
        let mut columns = Vec::new();
        let (start, current, skip) = self.start(query, &mut steps, &mut columns)?;
        if let Some(mut current) = current {
            for (position, segment) in query.path.iter().enumerate().skip(skip) {
                let answer = position + 1 == query.path.len() && query.select.is_none();
                current = self.segment(&current, segment, answer, &mut steps, &mut columns)?;
            }
            self.selected(query, &current, &mut columns)?;
            self.reached = Some(current);
        } else {
            self.event_columns(query, &mut columns)?;
        }
        Ok(Prepared {
            columns,
            domains: std::mem::take(&mut self.domains),
            start,
            steps,
        })
    }
}

/// Where a value is read in a plan: of an object of a type, or of an
/// event's call.
#[derive(Clone, Copy)]
enum Scope<'s> {
    Type(&'s str),
    Event(&'s Event),
}

fn described(field: &Field, name: String) -> Described {
    Described {
        shape: field.shape.clone(),
        doc: field.doc.clone(),
        domain: field.domain.clone(),
        name,
    }
}

/// Where an event's argument is when the event fires: a function's first
/// four arguments in `rcx`, `rdx`, `r8`, and `r9` or, for a float, `xmm0` to
/// `xmm3`; later ones on the stack above the return address and the 32
/// bytes kept for the first four.
#[must_use]
pub fn arg_at(arg: &angelica_map::Arg) -> ArgAt {
    use angelica_map::{ArgSource, Register};
    let float = matches!(arg.kind, ValueKind::F32 | ValueKind::F64);
    match &arg.source {
        // The planner never plans a derived event, which has no registers.
        ArgSource::Derived(_) => ArgAt::Integer(0),
        &ArgSource::Argument(position) if position < 4 => {
            if float {
                ArgAt::Float(position)
            } else {
                ArgAt::Integer([2, 3, 7, 8][usize::from(position)])
            }
        }
        &ArgSource::Argument(position) => ArgAt::Stack(8 + 8 * u32::from(position)),
        &ArgSource::Register(register) => match register {
            Register::Xmm(index) => ArgAt::Float(index),
            Register::Rax => ArgAt::Integer(0),
            Register::Rbx => ArgAt::Integer(1),
            Register::Rcx => ArgAt::Integer(2),
            Register::Rdx => ArgAt::Integer(3),
            Register::Rsi => ArgAt::Integer(4),
            Register::Rdi => ArgAt::Integer(5),
            Register::Rbp => ArgAt::Integer(6),
            Register::R8 => ArgAt::Integer(7),
            Register::R9 => ArgAt::Integer(8),
            Register::R10 => ArgAt::Integer(9),
            Register::R11 => ArgAt::Integer(10),
            Register::R12 => ArgAt::Integer(11),
            Register::R13 => ArgAt::Integer(12),
            Register::R14 => ArgAt::Integer(13),
            Register::R15 => ArgAt::Integer(14),
        },
    }
}

/// Checks `text` against `schema` and plans its reads. `permitted` says
/// whether the plugin may read a domain.
///
/// # Errors
/// `invalid-argument` for a query that breaks the syntax or the model's
/// rules, `not-found` for an unknown root or field, `permission-denied` for a
/// domain the plugin may not read or a forbidden field, `unavailable` for
/// anything the running build lacks, and `limit-exceeded` for a path too
/// deep.
pub fn prepare(schema: &Schema, text: &str, permitted: &dyn Fn(&str) -> bool) -> Result<Prepared> {
    plan(schema, text, permitted, false)
}

/// As [`prepare`], for a watch: a query that starts at an event of the
/// map and is read when the event fires.
///
/// # Errors
/// As for [`prepare`].
pub fn prepare_watch(
    schema: &Schema,
    text: &str,
    permitted: &dyn Fn(&str) -> bool,
) -> Result<Prepared> {
    plan(schema, text, permitted, true)
}

/// As [`prepare`], for a path that ends at objects: the plan reaches them
/// and answers nothing, for looking at their memory while working on the
/// map.
///
/// # Errors
/// As for [`prepare`].
pub fn prepare_objects(schema: &Schema, text: &str) -> Result<Prepared> {
    let query = query::parse(text).map_err(Error::invalid)?;
    Planner {
        objects: &schema.objects,
        schema,
        permitted: &|_| true,
        domains: BTreeSet::new(),
        covered: BTreeSet::new(),
        depth: 0,
        watch: false,
        objects_only: true,
        reached: None,
        fixed: BTreeMap::new(),
    }
    .plan(&query)
}

/// Plans a rule: a query from an event, such as `chat_message_added[kind =
/// 57]`, that holds when it reaches anything, without values to answer.
///
/// # Errors
/// As for [`prepare_watch`].
pub fn prepare_rule(
    schema: &Schema,
    text: &str,
    permitted: &dyn Fn(&str) -> bool,
) -> Result<Prepared> {
    let query = query::parse(text).map_err(Error::invalid)?;
    Planner {
        objects: &schema.objects,
        schema,
        permitted,
        domains: BTreeSet::new(),
        covered: BTreeSet::new(),
        depth: 0,
        watch: true,
        objects_only: true,
        reached: None,
        fixed: BTreeMap::new(),
    }
    .plan(&query)
}

/// A field a plugin changes: where it is in each object the plan reaches.
#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    /// The type that holds the field.
    pub owner: String,
    pub field: Field,
    pub offset: u32,
    /// How its value is read from each object the plan reaches.
    pub read: Read,
}

/// Plans a path to a value to change, such as `camera.camera.max_distance`:
/// the objects the path before the last field reaches, and that field,
/// checked as a query checks it against the domains `permitted`.
///
/// # Errors
/// As for [`prepare`], and `invalid-argument` for a path with braces or
/// without a field after its root.
pub fn prepare_set(
    schema: &Schema,
    text: &str,
    permitted: &dyn Fn(&str) -> bool,
) -> Result<(Prepared, Target)> {
    let mut query = query::parse(text).map_err(Error::invalid)?;
    if query.select.is_some() {
        return Err(Error::invalid(
            "a value to change is a path without braces, such as camera.camera.max_distance",
        ));
    }
    let Some(last) = query.path.pop() else {
        return Err(Error::invalid(format!(
            "name the field of {} to change after it, such as {}.<field>",
            query.root, query.root
        )));
    };
    if last.filter.is_some() {
        return Err(Error::invalid(format!(
            "{} is the value to change: it has no filter",
            last.field
        )));
    }
    let mut planner = Planner {
        objects: &schema.objects,
        schema,
        permitted,
        domains: BTreeSet::new(),
        covered: BTreeSet::new(),
        depth: 0,
        watch: false,
        objects_only: true,
        reached: None,
        fixed: BTreeMap::new(),
    };
    let mut prepared = planner.plan(&query)?;
    let reached = planner
        .reached
        .clone()
        .ok_or_else(|| Error::invalid("a value to change is a field of an object"))?;
    let (field, offset, owner, guard) = planner.lookup(&reached, &last.field)?;
    // A field of a variant is changed only in the objects of the variant.
    prepared
        .steps
        .extend(guard.map(|guard| Step::Filter(vec![guard])));
    let (read, _) = planner.value(&reached, std::slice::from_ref(&last.field))?;
    Ok((
        prepared,
        Target {
            owner,
            field: field.clone(),
            offset,
            read,
        },
    ))
}

fn plan(
    schema: &Schema,
    text: &str,
    permitted: &dyn Fn(&str) -> bool,
    watch: bool,
) -> Result<Prepared> {
    let query = query::parse(text).map_err(Error::invalid)?;
    Planner {
        objects: &schema.objects,
        schema,
        permitted,
        domains: BTreeSet::new(),
        covered: BTreeSet::new(),
        depth: 0,
        watch,
        objects_only: false,
        reached: None,
        fixed: BTreeMap::new(),
    }
    .plan(&query)
}
