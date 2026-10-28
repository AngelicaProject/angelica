//! What the described data means for permissions: domains, the domain of
//! each type and field, and what no plugin ever reads.

use std::collections::BTreeMap;

use toml::{Table, Value};

use crate::load::{Loader, valid_name, valid_word};
use crate::model::TypeDef;
use crate::table::Keys;

/// How far a domain reaches, as the player sees it.
pub const CLASSES: [&str; 5] = ["you", "display", "others", "acts", "acts_on_others"];

/// Words for the player, in English and Russian.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wording {
    pub en: String,
    pub ru: String,
}

/// A unit of the player's consent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Domain {
    pub name: String,
    pub doc: String,
    pub class: String,
    pub read: Wording,
    pub change: Option<Wording>,
    pub act: Option<Wording>,
    pub act_local: bool,
}

/// Which domains the elements of an array belong to, by the value of one
/// of their fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Partition {
    pub field: String,
    pub domains: BTreeMap<String, Vec<String>>,
}

/// What a field means beyond its type's domain.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FieldMeaning {
    pub domain: Option<String>,
    pub forbidden: bool,
    /// Whose objects a reference or an array reaches: `you` or `others`.
    pub whose: Option<String>,
    /// Domains renamed below the field.
    pub renames: BTreeMap<String, String>,
    pub partition: Option<Partition>,
}

/// The meaning of the description's data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Meaning {
    pub domains: BTreeMap<String, Domain>,
    /// The domain of each structure's fields.
    pub types: BTreeMap<String, String>,
    pub fields: BTreeMap<String, FieldMeaning>,
    /// Functions that lead to a request to the game's servers.
    pub server: BTreeMap<String, bool>,
}

impl Meaning {
    /// Reads every `meaning/<area>.toml`. What cannot be read marks the
    /// type or field it is about: without its meaning, nothing may read it.
    pub(crate) fn read(files: Vec<(String, Table)>, loader: &mut Loader) -> Self {
        let mut meaning = Self::default();
        let mut pending = Vec::new();
        for (area, table) in files {
            for (kind, items) in table {
                let Value::Table(items) = items else {
                    loader.mark(
                        &format!("meaning/{area}.toml"),
                        format!("`{kind}` is not a table"),
                    );
                    continue;
                };
                for (name, value) in items {
                    pending.push((area.clone(), kind.clone(), name, value));
                }
            }
        }
        // Domains first: types and fields name them.
        pending.sort_by_key(|(_, kind, _, _)| kind != "domains");
        for (area, kind, name, value) in pending {
            let result = match kind.as_str() {
                "domains" => read_domain(&name, value).map(|domain| {
                    meaning.domains.insert(name.clone(), domain);
                }),
                "types" | "fields" | "functions"
                    if name.split('.').next() != Some(area.as_str()) =>
                {
                    Err(format!(
                        "does not belong to the area of meaning/{area}.toml"
                    ))
                }
                "types" => meaning.read_type(&name, value, loader),
                "fields" => meaning.read_field(&name, value, loader),
                "functions" => meaning.read_function(&name, value, loader),
                _ => Err(format!("`{kind}` is not a kind of meaning")),
            };
            if let Err(reason) = result {
                let subject = if kind == "domains" {
                    format!("domain {name}")
                } else {
                    name
                };
                loader.mark(&subject, format!("meaning: {reason}"));
            }
        }
        meaning
    }

    fn known_domain(&self, name: &str) -> Result<(), String> {
        if self.domains.contains_key(name) {
            Ok(())
        } else {
            Err(format!("domain `{name}` is not defined"))
        }
    }

    fn read_type(&mut self, name: &str, value: Value, loader: &Loader) -> Result<(), String> {
        if !matches!(loader.types.get(name), Some(TypeDef::Struct(_))) {
            return Err("is not a structure".to_owned());
        }
        let mut keys = Keys::new(value)?;
        let domain = keys.required_string("domain")?;
        keys.finish()?;
        self.known_domain(&domain)?;
        self.types.insert(name.to_owned(), domain);
        Ok(())
    }

    fn read_field(&mut self, name: &str, value: Value, loader: &Loader) -> Result<(), String> {
        if !loader.has_field(name) {
            return Err("is not a field of a structure".to_owned());
        }
        let mut keys = Keys::new(value)?;
        let mut field = FieldMeaning {
            domain: keys.string("domain")?,
            forbidden: keys.boolean("forbidden")?.unwrap_or(false),
            whose: keys.string("whose")?,
            ..FieldMeaning::default()
        };
        if let Some(whose) = &field.whose
            && whose != "you"
            && whose != "others"
        {
            return Err(format!("`whose` is `you` or `others`, not `{whose}`"));
        }
        for (from, to) in keys.table("as")?.unwrap_or_default() {
            let Value::String(to) = to else {
                return Err("`as` renames a domain to a domain".to_owned());
            };
            self.known_domain(&from)?;
            self.known_domain(&to)?;
            field.renames.insert(from, to);
        }
        if let Some(partition) = keys.table("partition")? {
            let mut partition = Keys::new(Value::Table(partition))?;
            let by = partition.required_string("field")?;
            let mut domains = BTreeMap::new();
            for (domain, values) in partition.table("domains")?.unwrap_or_default() {
                self.known_domain(&domain)?;
                let Value::Array(values) = values else {
                    return Err("a partition lists values per domain".to_owned());
                };
                let values = values
                    .into_iter()
                    .map(|value| match value {
                        Value::String(value) => Ok(value),
                        _ => Err("a partition's values are names".to_owned()),
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                domains.insert(domain, values);
            }
            partition.finish()?;
            field.partition = Some(Partition { field: by, domains });
        }
        keys.finish()?;
        if let Some(domain) = &field.domain {
            self.known_domain(domain)?;
        }
        self.fields.insert(loader.canonical(name), field);
        Ok(())
    }

    fn read_function(&mut self, name: &str, value: Value, loader: &Loader) -> Result<(), String> {
        if !matches!(
            loader.entries.get(name).map(|entry| entry.kind.table()),
            Some("function")
        ) {
            return Err("is not a function".to_owned());
        }
        let mut keys = Keys::new(value)?;
        let server = keys.boolean("server")?.unwrap_or(false);
        keys.finish()?;
        self.server.insert(name.to_owned(), server);
        Ok(())
    }
}

fn read_domain(name: &str, value: Value) -> Result<Domain, String> {
    if !valid_word(name) || valid_name(name) {
        return Err("a domain is named by one word".to_owned());
    }
    let mut keys = Keys::new(value)?;
    let wording = |table: Option<Table>| -> Result<Option<Wording>, String> {
        table
            .map(|table| {
                let mut words = Keys::new(Value::Table(table))?;
                let wording = Wording {
                    en: words.required_string("en")?,
                    ru: words.required_string("ru")?,
                };
                words.finish()?;
                if wording.en.trim().is_empty() || wording.ru.trim().is_empty() {
                    return Err("wording is not blank".to_owned());
                }
                Ok(wording)
            })
            .transpose()
    };
    let doc = keys.required_string("doc")?;
    let class = keys.required_string("class")?;
    if !CLASSES.contains(&class.as_str()) {
        return Err(format!(
            "`class` is one of {}, not `{class}`",
            CLASSES.join(", ")
        ));
    }
    let read = wording(keys.table("read")?)?.ok_or("`read` is required")?;
    let change = wording(keys.table("change")?)?;
    let act = wording(keys.table("act")?)?;
    let act_local = keys.boolean("act_local")?.unwrap_or(false);
    keys.finish()?;
    if act_local && act.is_none() {
        return Err("`act_local` goes with `act`".to_owned());
    }
    Ok(Domain {
        name: name.to_owned(),
        doc,
        class,
        read,
        change,
        act,
        act_local,
    })
}
