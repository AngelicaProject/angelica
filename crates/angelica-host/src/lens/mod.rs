//! Lens: plugins read the game's objects by paths through the game map's
//! object model (`docs/architecture/lens.md`). A query is checked and
//! planned once against the schema of the running build ([`prepare`]);
//! running it ([`run`]) only reads memory along the plan, which fails
//! instead of faulting. A watch is a query from an event of the map, read
//! when the game reaches the event's point ([`watch`]). Platform
//! independent: the Windows adapter only supplies the memory reads and the
//! hooks.
#![forbid(unsafe_code)]
pub mod act;
pub mod decide;
pub mod plan;
pub mod query;
pub mod read;
pub mod schema;
pub mod set;
#[cfg(test)]
mod tests;
pub mod watch;
#[cfg(test)]
mod window_tests;

pub use plan::{
    Column, ColumnMeaning, Prepared, Stored, prepare, prepare_objects, prepare_rule, prepare_watch,
};
pub use read::{Call, Memory, Row, Value, objects, run, run_call};
pub use schema::Schema;
pub use watch::{Delivery, Watches};

use crate::error::{Error, Kind, Result};
use std::sync::{
    Arc, PoisonError, RwLock,
    atomic::{AtomicU64, Ordering},
};

/// The most queries one plugin instance holds prepared.
pub const MOST_QUERIES: usize = 256;

/// The schema of the running build, which a reloaded map replaces.
#[derive(Default)]
pub struct Lens {
    schema: RwLock<Arc<Schema>>,
    generation: AtomicU64,
    /// The plugins' watches of events.
    pub watches: Watches,
    /// Whether plugins may act, and how often each acted.
    pub acting: act::Acting,
    /// The values plugins changed, and their originals.
    pub changes: set::Changes,
    /// The plugins' rules at decision points.
    pub decisions: decide::Decisions,
}

impl Lens {
    #[must_use]
    pub fn new(schema: Schema) -> Self {
        Self {
            schema: RwLock::new(Arc::new(schema)),
            generation: AtomicU64::new(0),
            watches: Watches::default(),
            acting: act::Acting::default(),
            changes: set::Changes::default(),
            decisions: decide::Decisions::default(),
        }
    }

    /// The current schema and its generation, which changes with each
    /// replacement.
    #[must_use]
    pub fn schema(&self) -> (Arc<Schema>, u64) {
        let schema = self
            .schema
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        (schema, self.generation.load(Ordering::Acquire))
    }

    /// Takes `schema` in place of the current one; prepared queries are
    /// planned again at their next run, watches now.
    pub fn replace(&self, schema: Schema) {
        self.watches.replan(&schema);
        self.decisions.replan(&schema);
        *self.schema.write().unwrap_or_else(PoisonError::into_inner) = Arc::new(schema);
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    /// Starts a plugin's watch, prepared under `handle`: the event's hook
    /// is installed through `hook` (with the point's id, the event's map
    /// entry, and its address) the first time the event is watched.
    ///
    /// # Errors
    /// `unavailable` when the hook cannot be installed.
    pub fn watch(
        &self,
        plugin: &str,
        handle: u32,
        text: &str,
        read: std::collections::BTreeSet<String>,
        prepared: Prepared,
        hook: &dyn Fn(usize, &str, u64) -> std::result::Result<(), String>,
    ) -> Result<()> {
        let (schema, _) = self.schema();
        let Some(event) = prepared
            .event()
            .and_then(|name| schema.objects.events.get(name))
        else {
            return Err(Error::invalid("a watch starts at an event"));
        };
        let address = schema
            .events
            .get(&event.name)
            .cloned()
            .unwrap_or_else(|| Err("not in the schema".into()))
            .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
        let (id, hooked) = self.watches.point(&event.at);
        if !hooked {
            hook(id, &event.at, address).map_err(|reason| {
                Error::new(
                    Kind::Unavailable,
                    format!("event {} cannot be watched: {reason}", event.name),
                )
            })?;
            self.watches.hooked(id);
        }
        let subscriber = watch::Subscriber::new(
            plugin,
            handle,
            text,
            read,
            &event.name,
            event.rate,
            prepared,
        );
        self.watches.subscribe(id, subscriber);
        Ok(())
    }

    /// Starts a plugin's rule, prepared under `handle`, to take `outcome`
    /// at its event: the event's hook is installed through `hook` (with the
    /// point's id, the event's map entry, and its address) the first time.
    ///
    /// # Errors
    /// `not-found` for an outcome the event does not have,
    /// `permission-denied` when the plugin may not change the event's
    /// domain, `unavailable` when the hook cannot be installed.
    pub fn decide(
        &self,
        request: decide::Request<'_>,
        may_change: &dyn Fn(&str) -> bool,
        prepared: Prepared,
        hook: &dyn Fn(usize, &str, u64) -> std::result::Result<(), String>,
    ) -> Result<()> {
        let decide::Request {
            plugin,
            handle,
            text,
            outcome,
            permitted,
        } = request;
        let (schema, _) = self.schema();
        let Some(event) = prepared
            .event()
            .and_then(|name| schema.objects.events.get(name))
        else {
            return Err(Error::invalid("a rule starts at an event"));
        };
        if event.outcomes.is_empty() {
            return Err(Error::invalid(format!(
                "event {} decides nothing: watch it with lens.watch",
                event.name
            )));
        }
        let Some(found) = event.outcomes.get(outcome) else {
            let known = event
                .outcomes
                .keys()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            return Err(Error::new(
                Kind::NotFound,
                format!(
                    "event {} has no outcome {outcome}; its outcomes: {known}",
                    event.name
                ),
            ));
        };
        if !may_change(&event.domain) {
            return Err(Error::new(
                Kind::PermissionDenied,
                format!(
                    "deciding at {} changes domain {}, which the plugin may not change: list it in the manifest's [lens] change",
                    event.name, event.domain
                ),
            ));
        }
        let address = schema
            .events
            .get(&event.name)
            .cloned()
            .unwrap_or_else(|| Err("not in the schema".into()))
            .map_err(|reason| Error::new(Kind::Unavailable, reason))?;
        let (id, hooked) = self.decisions.point(&event.at);
        if !hooked {
            hook(id, &event.at, address).map_err(|reason| {
                Error::new(
                    Kind::Unavailable,
                    format!("event {} cannot be decided at: {reason}", event.name),
                )
            })?;
            self.decisions.hooked(id);
        }
        let returns = match found.returns {
            Some(angelica_map::Number::Integer(value)) => value.cast_unsigned(),
            Some(angelica_map::Number::Float(bits)) => bits,
            None => 0,
        };
        let rule = decide::Rule::new(plugin, handle, text, permitted, prepared, returns);
        self.decisions.add(id, rule);
        Ok(())
    }

    /// Refuses a rule at an event whose domain the plugin may not change,
    /// before the rule is planned, so the refusal names the change.
    ///
    /// # Errors
    /// `permission-denied` for such an event.
    pub fn may_decide(&self, text: &str, may_change: &dyn Fn(&str) -> bool) -> Result<()> {
        let (schema, _) = self.schema();
        let Some(event) = query::parse(text)
            .ok()
            .and_then(|parsed| schema.objects.events.get(&parsed.root).cloned())
        else {
            return Ok(());
        };
        if event.outcomes.is_empty() || may_change(&event.domain) {
            return Ok(());
        }
        Err(Error::new(
            Kind::PermissionDenied,
            format!(
                "deciding at {} changes domain {}, which the plugin may not change: list it in the manifest's [lens] change",
                event.name, event.domain
            ),
        ))
    }

    /// A domain of the map, for what the player is told.
    #[must_use]
    pub fn domain(&self, name: &str) -> Option<angelica_map::Domain> {
        self.schema().0.objects.domains.get(name).cloned()
    }
}

/// A query a plugin prepared, planned again when the schema changes.
struct Held {
    text: String,
    generation: u64,
    planned: Result<Prepared>,
    kind: HeldKind,
}

/// What a held query is: one to run, a watch whose rows arrive as events,
/// or a rule at a decision point; only the first is run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeldKind {
    Query,
    Watch,
    Rule,
}

/// The queries one plugin instance prepared, by handle.
#[derive(Default)]
pub struct Queries {
    held: Vec<Option<Held>>,
}

impl Queries {
    /// Prepares `text` and keeps it under a new handle.
    ///
    /// # Errors
    /// The planner's errors, or `limit-exceeded` with [`MOST_QUERIES`] held.
    pub fn prepare(
        &mut self,
        lens: &Lens,
        text: &str,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<(u32, Prepared)> {
        let (schema, generation) = lens.schema();
        let prepared = prepare(&schema, text, permitted)?;
        self.hold(text, generation, prepared, HeldKind::Query)
    }

    /// Prepares a watch of `text` and keeps it under a new handle; the
    /// caller starts it with [`Lens::watch`].
    ///
    /// # Errors
    /// As for [`Queries::prepare`].
    pub fn watch(
        &mut self,
        lens: &Lens,
        text: &str,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<(u32, Prepared)> {
        let (schema, generation) = lens.schema();
        let prepared = prepare_watch(&schema, text, permitted)?;
        self.hold(text, generation, prepared, HeldKind::Watch)
    }

    /// Prepares a rule of `text` and keeps it under a new handle; the
    /// caller starts it with [`Lens::decide`].
    ///
    /// # Errors
    /// As for [`Queries::prepare`].
    pub fn rule(
        &mut self,
        lens: &Lens,
        text: &str,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<(u32, Prepared)> {
        let (schema, generation) = lens.schema();
        let prepared = prepare_rule(&schema, text, permitted)?;
        self.hold(text, generation, prepared, HeldKind::Rule)
    }

    fn hold(
        &mut self,
        text: &str,
        generation: u64,
        prepared: Prepared,
        kind: HeldKind,
    ) -> Result<(u32, Prepared)> {
        let slot = match self.held.iter().position(Option::is_none) {
            Some(slot) => slot,
            None if self.held.len() < MOST_QUERIES => {
                self.held.push(None);
                self.held.len() - 1
            }
            None => {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!(
                        "a plugin holds at most {MOST_QUERIES} prepared queries; release the ones it no longer runs"
                    ),
                ));
            }
        };
        self.held[slot] = Some(Held {
            text: text.to_owned(),
            generation,
            planned: Ok(prepared.clone()),
            kind,
        });
        Ok((u32::try_from(slot).unwrap_or(u32::MAX), prepared))
    }

    /// The plan of a held query for the current schema.
    ///
    /// # Errors
    /// `not-found` for an unknown handle; the planner's errors when the
    /// query no longer fits the reloaded map.
    pub fn plan(
        &mut self,
        lens: &Lens,
        handle: u32,
        permitted: &dyn Fn(&str) -> bool,
    ) -> Result<Prepared> {
        let held = usize::try_from(handle)
            .ok()
            .and_then(|slot| self.held.get_mut(slot))
            .and_then(Option::as_mut)
            .ok_or_else(|| Error::new(Kind::NotFound, format!("no prepared query {handle}")))?;
        if held.kind != HeldKind::Query {
            return Err(Error::invalid(format!(
                "query {handle} is a watch or a rule: it is never run"
            )));
        }
        let (schema, generation) = lens.schema();
        if held.generation != generation {
            held.planned = prepare(&schema, &held.text, permitted);
            held.generation = generation;
        }
        held.planned.clone()
    }

    /// Forgets a held query; an unknown handle is ignored. Returns what it
    /// was, so the caller ends a watch or a rule.
    pub fn release(&mut self, handle: u32) -> Option<HeldKind> {
        usize::try_from(handle)
            .ok()
            .and_then(|slot| self.held.get_mut(slot))
            .and_then(Option::take)
            .map(|held| held.kind)
    }
}
