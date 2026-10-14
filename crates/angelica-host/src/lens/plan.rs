//! A query checked against the schema and turned into a plan of reads:
//! every name known, every domain on the way permitted, every place present
//! in the running build. Running the plan only reads memory.
#![forbid(unsafe_code)]
use super::query::{self, Filter, Literal, Op, Query, Test};
use super::schema::{RootAt, Schema};
use crate::error::{Error, Kind, Result};
use angelica_map::{Field, Length, Meaning, Objects, Shape, ValueKind};
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
}

/// One step to another object: into an embedded object, or through the
/// pointer at `offset`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hop {
    pub offset: u32,
    pub pointer: bool,
}

/// How a value is read from an object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Read {
    pub hops: Vec<Hop>,
    pub offset: u32,
    pub stored: Stored,
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

#[derive(Debug, Clone, PartialEq, Eq)]
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

/// A query ready to run in the build its schema describes.
#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub columns: Vec<Column>,
    /// Every domain the query reads.
    pub domains: BTreeSet<String>,
    pub(crate) root: RootAt,
    pub(crate) steps: Vec<Step>,
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
    fn value(&mut self, type_name: &str, path: &[String]) -> Result<(Read, &'a Field, String)> {
        let mut current = type_name.to_owned();
        let mut hops = Vec::new();
        for (position, name) in path.iter().enumerate() {
            let (field, offset) = self.field(&current, name)?;
            // The type that declares the field, which may be a base of `current`.
            let owner = self
                .objects
                .field(&current, name)
                .map_or_else(|| current.clone(), |(owner, _)| owner.name.clone());
            let last = position + 1 == path.len();
            match (&field.shape, last) {
                (Shape::Value { kind, .. }, true) => {
                    return Ok((
                        Read {
                            hops,
                            offset,
                            stored: Stored::Value(*kind),
                        },
                        field,
                        format!("{owner}.{name}"),
                    ));
                }
                (Shape::Text(length), true) => {
                    return Ok((
                        Read {
                            hops,
                            offset,
                            stored: Stored::Text(*length),
                        },
                        field,
                        format!("{owner}.{name}"),
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

    fn condition(&mut self, type_name: &str, condition: &query::Condition) -> Result<Condition> {
        let (read, field, name) = self.value(type_name, &condition.field)?;
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
            return Ok(Condition {
                read,
                check: Check::Text(equal, texts),
            });
        }
        let Shape::Value { kind, meaning, .. } = &field.shape else {
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
        type_name: &str,
        conditions: &[query::Condition],
    ) -> Result<Vec<Condition>> {
        conditions
            .iter()
            .map(|condition| self.condition(type_name, condition))
            .collect()
    }

    fn column(&mut self, type_name: &str, name: &str, path: &[String]) -> Result<Column> {
        let (read, field, qualified) = self.value(type_name, path)?;
        let (meaning, unit) = match &field.shape {
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
                };
                (meaning, unit.clone())
            }
            _ => (ColumnMeaning::Plain, None),
        };
        Ok(Column {
            name: name.to_owned(),
            doc: field.doc.clone(),
            field: qualified,
            stored: read.stored,
            meaning,
            unit,
            domain: field.domain.clone(),
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
            Some(Filter::Conditions(conditions)) => (None, self.conditions(&array.of, conditions)?),
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
            hops: Vec::new(),
            offset,
            stored: Stored::Value(*kind),
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

    fn plan(&mut self, query: &Query) -> Result<Prepared> {
        let objects = self.objects;
        let (root, at) = self.root(&query.root)?;
        let mut current = root.r#type.clone();
        let mut steps = Vec::new();
        let mut columns = Vec::new();
        for (position, segment) in query.path.iter().enumerate() {
            let last = position + 1 == query.path.len();
            let (field, offset) = self.field(&current, &segment.field)?;
            let field = field.clone();
            let holder = current.clone();
            match &field.shape {
                Shape::Ref(to) | Shape::Embed(to) => {
                    steps.push(Step::Hop(Hop {
                        offset,
                        pointer: matches!(field.shape, Shape::Ref(_)),
                    }));
                    current.clone_from(to);
                    match &segment.filter {
                        None => {}
                        Some(Filter::Index(_)) => {
                            return Err(Error::invalid(format!(
                                "{holder}.{} is one object, not an array: it has no index",
                                segment.field
                            )));
                        }
                        Some(Filter::Conditions(conditions)) => {
                            let conditions = self.conditions(&current, conditions)?;
                            steps.push(Step::Filter(conditions));
                        }
                    }
                }
                Shape::Array {
                    array,
                    behind_pointer,
                } => {
                    let step = self.array(
                        &holder,
                        segment,
                        offset,
                        array,
                        *behind_pointer,
                        &field.domain,
                    )?;
                    steps.push(Step::Array(step));
                    current.clone_from(&array.of);
                }
                Shape::Value { .. } | Shape::Text(_) if last && query.select.is_none() => {
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
                        &holder,
                        &segment.field,
                        std::slice::from_ref(&segment.field),
                    )?);
                }
                Shape::Value { .. } | Shape::Text(_) => {
                    return Err(Error::invalid(format!(
                        "{holder}.{} is a value: a path goes on only through references, objects, and arrays",
                        segment.field
                    )));
                }
            }
        }
        match &query.select {
            Some(selected) => {
                for selected in selected {
                    columns.push(self.column(&current, &selected.name, &selected.path)?);
                }
            }
            None if columns.is_empty() => {
                let known = objects
                    .fields(&current)
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
        Ok(Prepared {
            columns,
            domains: std::mem::take(&mut self.domains),
            root: at,
            steps,
        })
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
    let query = query::parse(text).map_err(Error::invalid)?;
    Planner {
        objects: &schema.objects,
        schema,
        permitted,
        domains: BTreeSet::new(),
        covered: BTreeSet::new(),
        depth: 0,
    }
    .plan(&query)
}
