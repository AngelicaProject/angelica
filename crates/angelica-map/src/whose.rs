//! Whose objects a path reaches, and the domains it renames on the way
//! (`docs/formats/game-map-v2.md#whose-objects`): the player's own
//! character and the one they selected are the same type, read in
//! different domains.
use std::collections::{BTreeMap, BTreeSet};

use crate::objects::{DomainClass, Objects, Shape};

/// Whose objects a path reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Whose {
    /// The player's own.
    You,
    /// Other players' and the world's.
    Others,
}

/// Where a path enters objects, at a root, an event, a reference or array
/// field, or a reference argument: whose they are, and the domains renamed
/// below.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Entering {
    /// `None` keeps the `whose` of the object or event the field or
    /// argument belongs to; a map that `angelica map check` accepts has it
    /// on every root and event.
    pub whose: Option<Whose>,
    /// Below this point, a field of a key's domain belongs to its value.
    pub renames: BTreeMap<String, String>,
}

impl Entering {
    /// `whose` and `as` as written.
    ///
    /// # Errors
    /// An unknown `whose`.
    pub(crate) fn parse(
        whose: Option<String>,
        renames: Option<BTreeMap<String, String>>,
    ) -> Result<Self, String> {
        let whose = whose
            .map(|whose| match whose.as_str() {
                "you" => Ok(Whose::You),
                "others" => Ok(Whose::Others),
                _ => Err(format!("`whose` is you or others, not `{whose}`")),
            })
            .transpose()?;
        Ok(Self {
            whose,
            renames: renames.unwrap_or_default(),
        })
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.whose.is_none() && self.renames.is_empty()
    }
}

/// The domain renames in force at a point of a path.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Renames(BTreeMap<String, String>);

impl Renames {
    /// Goes on below `entering`: its renames replace those in force for
    /// the domains it lists.
    pub fn enter(&mut self, entering: &Entering) {
        for (from, to) in &entering.renames {
            self.0.insert(from.clone(), to.clone());
        }
    }

    /// The domain a field of `domain` belongs to here.
    #[must_use]
    pub fn domain<'a>(&'a self, domain: &'a str) -> &'a str {
        self.0.get(domain).map_or(domain, String::as_str)
    }
}

impl Objects {
    /// The roots and events without `whose`, which `angelica map check`
    /// refuses; a host reads them as the player's.
    #[must_use]
    pub fn without_whose(&self) -> Vec<String> {
        let roots = self
            .roots
            .values()
            .filter(|root| root.entering.whose.is_none())
            .map(|root| format!("root `{}`", root.name));
        let events = self
            .events
            .values()
            .filter(|event| event.entering.whose.is_none())
            .map(|event| format!("event `{}`", event.name));
        roots.chain(events).collect()
    }
}

/// Every `whose` and `as` of the model: each renames only domains below it
/// to domains that exist, and no field on a path to others' objects belongs
/// to a domain about the player.
///
/// # Errors
/// A message naming the place.
pub(crate) fn check(objects: &Objects) -> Result<(), String> {
    let mut walk = Walk {
        objects,
        visited: BTreeSet::new(),
    };
    for root in objects.roots.values() {
        let at = format!("root `{}`", root.name);
        walk.renames_below(&at, &root.entering, &domains_below(objects, &root.r#type))?;
        let mut renames = Renames::default();
        renames.enter(&root.entering);
        let whose = root.entering.whose.unwrap_or(Whose::You);
        walk.object(&at, &root.r#type, whose, &renames)?;
    }
    for event in objects.events.values() {
        let at = format!("event `{}`", event.name);
        let mut below = BTreeSet::new();
        for arg in event.args.values() {
            below.insert(arg.domain.clone());
            if let Shape::Ref(to) = &arg.shape {
                below.extend(domains_below(objects, to));
            }
        }
        walk.renames_below(&at, &event.entering, &below)?;
        let mut renames = Renames::default();
        renames.enter(&event.entering);
        let whose = event.entering.whose.unwrap_or(Whose::You);
        for arg in event.args.values() {
            if arg.forbidden {
                continue;
            }
            let here = format!("{at}: argument `{}`", arg.name);
            walk.domain(&here, &arg.domain, whose, &renames, false)?;
            if let Shape::Ref(to) = &arg.shape {
                walk.renames_below(&here, &arg.entering, &domains_below(objects, to))?;
                let mut inner = renames.clone();
                inner.enter(&arg.entering);
                walk.object(&here, to, arg.entering.whose.unwrap_or(whose), &inner)?;
            }
        }
    }
    Ok(())
}

struct Walk<'a> {
    objects: &'a Objects,
    /// The types already walked, with whose objects and the renames.
    visited: BTreeSet<(String, Whose, Renames)>,
}

impl Walk<'_> {
    /// `entering` renames only domains in `below`, to domains that exist.
    fn renames_below(
        &self,
        at: &str,
        entering: &Entering,
        below: &BTreeSet<String>,
    ) -> Result<(), String> {
        for (from, to) in &entering.renames {
            if !below.contains(from) {
                return Err(format!(
                    "{at}: `as` renames `{from}`, which nothing below it belongs to"
                ));
            }
            if !self.objects.domains.contains_key(to) {
                return Err(format!(
                    "{at}: `as` renames `{from}` to `{to}`, which is not in domains.toml"
                ));
            }
        }
        Ok(())
    }

    /// A field or argument of `domain`, read here.
    fn domain(
        &self,
        at: &str,
        domain: &str,
        whose: Whose,
        renames: &Renames,
        settable: bool,
    ) -> Result<(), String> {
        let belongs = renames.domain(domain);
        let Some(found) = self.objects.domains.get(belongs) else {
            return Err(format!("{at}: domain `{belongs}` is not in domains.toml"));
        };
        if whose == Whose::Others && found.class == DomainClass::You {
            return Err(format!(
                "{at} belongs to `{belongs}`, a domain only about the player, on a path to others' objects (`whose = \"others\"`): rename it with `as`"
            ));
        }
        if settable && belongs != domain && found.change.is_none() {
            return Err(format!(
                "{at} can be set, and `{belongs}`, which `as` renames `{domain}` to, has no change wording"
            ));
        }
        Ok(())
    }

    /// The fields of an object of `type_name` and of its variants, and the
    /// objects below them.
    fn object(
        &mut self,
        at: &str,
        type_name: &str,
        whose: Whose,
        renames: &Renames,
    ) -> Result<(), String> {
        if !self
            .visited
            .insert((type_name.to_owned(), whose, renames.clone()))
        {
            return Ok(());
        }
        let objects = self.objects;
        let variants = objects
            .types
            .values()
            .filter(|object| {
                object.variant.is_some() && object.extends.as_deref() == Some(type_name)
            })
            .map(|object| object.name.as_str());
        for name in std::iter::once(type_name).chain(variants) {
            for (field_name, field) in objects.fields(name) {
                if field.forbidden {
                    continue;
                }
                let here = format!("{at}: `{name}.{field_name}`");
                let mut inner = renames.clone();
                inner.enter(&field.entering);
                let inner_whose = field.entering.whose.unwrap_or(whose);
                match &field.shape {
                    Shape::Array { array, .. } => {
                        self.renames_below(
                            &here,
                            &field.entering,
                            &domains_below(objects, &array.of),
                        )?;
                        if let Some(partition) = &array.partition {
                            for domain in partition
                                .domains
                                .values()
                                .chain(std::iter::once(&field.domain))
                            {
                                self.domain(&here, domain, inner_whose, &inner, false)?;
                            }
                        } else {
                            self.domain(&here, &field.domain, whose, renames, false)?;
                        }
                        self.object(&here, &array.of, inner_whose, &inner)?;
                    }
                    Shape::Ref(to) => {
                        self.domain(&here, &field.domain, whose, renames, false)?;
                        self.renames_below(&here, &field.entering, &domains_below(objects, to))?;
                        self.object(&here, to, inner_whose, &inner)?;
                    }
                    Shape::Embed(to) => {
                        self.domain(&here, &field.domain, whose, renames, false)?;
                        self.object(&here, to, whose, renames)?;
                    }
                    Shape::Value { .. } | Shape::Text(_) => {
                        self.domain(&here, &field.domain, whose, renames, field.set.is_some())?;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The domains of every field, partition, and object below an object of
/// `type_name`, as the map writes them.
fn domains_below(objects: &Objects, type_name: &str) -> BTreeSet<String> {
    let mut domains = BTreeSet::new();
    let mut seen = BTreeSet::new();
    let mut pending = vec![type_name.to_owned()];
    while let Some(name) = pending.pop() {
        if !seen.insert(name.clone()) {
            continue;
        }
        pending.extend(
            objects
                .types
                .values()
                .filter(|object| {
                    object.variant.is_some() && object.extends.as_deref() == Some(name.as_str())
                })
                .map(|object| object.name.clone()),
        );
        for field in objects.fields(&name).values() {
            domains.insert(field.domain.clone());
            match &field.shape {
                Shape::Array { array, .. } => {
                    if let Some(partition) = &array.partition {
                        domains.extend(partition.domains.values().cloned());
                    }
                    pending.push(array.of.clone());
                }
                Shape::Ref(to) | Shape::Embed(to) => pending.push(to.clone()),
                Shape::Value { .. } | Shape::Text(_) => {}
            }
        }
    }
    domains
}

#[cfg(test)]
mod tests {
    use crate::Map;

    const DOMAINS: &str = r#"
[domains.character]
doc = "The player's character."
class = "you"
read = { en = "Sees your character", ru = "Видит вашего персонажа" }

[domains.location]
doc = "Where the player is."
class = "you"
read = { en = "Sees where you are", ru = "Видит, где вы" }

[domains.target]
doc = "The object the player has selected."
class = "others"
read = { en = "Sees your target", ru = "Видит вашу цель" }
"#;

    const CHARACTER_ENTRIES: &str = r#"
[global."character.player.instance"]
locator = { signature = "48 8D 0D ?? ?? ?? ?? E8", follow = "rel32", at = 3 }
type = "ptr"

[field."character.character.health"]
of = "character.character"
type = "u32"
offset = 0x1AC

[field."character.character.x"]
of = "character.character"
type = "f32"
offset = 0xB0
"#;

    const TARGET_ENTRIES: &str = r#"
[global."target.system.instance"]
locator = { signature = "48 8D 0D ?? ?? ?? ?? E9", follow = "rel32", at = 3 }
type = "struct"

[field."target.system.current"]
of = "target.system"
type = "ptr"
offset = 0x80
"#;

    const CHARACTER: &str = r#"
[types."character.character"]
doc = "A character."
domain = "character"

[types."character.character".fields.health]
doc = "Current HP."

[types."character.character".fields.x]
doc = "East-west."
domain = "location"
"#;

    const TARGET: &str = r#"
[types."target.system"]
doc = "The player's targets."
domain = "target"

[types."target.system".fields.current]
doc = "The object the player has selected."
ref = "character.character"
whose = "others"
as = { character = "target", location = "target" }
"#;

    const ROOTS: &str = r#"
[roots.player]
doc = "The player's character."
global = "character.player.instance"
type = "character.character"
whose = "you"

[roots.target]
doc = "The player's targets."
global = "target.system.instance"
type = "target.system"
whose = "you"
"#;

    fn model(character: &str, target: &str, roots: &str) -> Result<Map, String> {
        Map::from_sources(
            "format = 2",
            &[
                ("entries/character.toml".into(), CHARACTER_ENTRIES.into()),
                ("entries/target.toml".into(), TARGET_ENTRIES.into()),
                ("domains.toml".into(), DOMAINS.into()),
                ("types/character.toml".into(), character.into()),
                ("types/target.toml".into(), target.into()),
                ("roots.toml".into(), roots.into()),
            ],
        )
        .map_err(|error| error.to_string())
    }

    fn map(target: &str) -> Result<Map, String> {
        model(CHARACTER, target, ROOTS)
    }

    #[test]
    fn a_target_reads_the_players_type_in_its_own_domain() {
        let map = map(TARGET).unwrap();
        assert!(map.objects().without_whose().is_empty());
        let (_, current) = map.objects().field("target.system", "current").unwrap();
        assert_eq!(current.entering.renames["character"], "target");
    }

    #[test]
    fn a_root_without_whose_is_listed_for_map_check() {
        let roots = ROOTS.replacen("whose = \"you\"\n", "", 1);
        let map = model(CHARACTER, TARGET, &roots).unwrap();
        assert_eq!(map.objects().without_whose(), ["root `player`"]);
    }

    #[test]
    fn others_objects_never_read_a_domain_only_about_the_player() {
        for (target, says) in [
            (
                TARGET.replace(
                    "as = { character = \"target\", location = \"target\" }\n",
                    "",
                ),
                "`character.character.health` belongs to `character`",
            ),
            (
                TARGET.replace(", location = \"target\"", ""),
                "`character.character.x` belongs to `location`",
            ),
        ] {
            let error = map(&target).unwrap_err();
            assert!(error.contains(says), "{error}");
            assert!(error.contains("rename it with `as`"), "{error}");
        }
    }

    #[test]
    fn as_renames_only_domains_below_it_to_domains_that_exist() {
        for (target, says) in [
            (
                TARGET.replace("location = \"target\"", "inventory = \"target\""),
                "renames `inventory`, which nothing below it belongs to",
            ),
            (
                TARGET.replace("location = \"target\"", "location = \"party\""),
                "renames `location` to `party`, which is not in domains.toml",
            ),
            (
                TARGET.replace("whose = \"others\"", "whose = \"theirs\""),
                "`whose` is you or others",
            ),
        ] {
            let error = map(&target).unwrap_err();
            assert!(error.contains(says), "{error}");
        }
    }

    #[test]
    fn whose_belongs_to_references_and_arrays() {
        let character = CHARACTER.replace(
            "doc = \"Current HP.\"",
            "doc = \"Current HP.\"\nwhose = \"others\"",
        );
        let error = model(&character, TARGET, ROOTS).unwrap_err();
        assert!(
            error.contains("`whose` and `as` belong to references and arrays"),
            "{error}"
        );
    }
}
