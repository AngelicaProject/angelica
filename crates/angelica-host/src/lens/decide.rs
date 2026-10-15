//! Decisions: plugins' rules at events of the map that have outcomes. A
//! rule is a query from the event that holds when it reaches anything, and
//! the outcome to take then: return from the game's function at once with
//! the outcome's value, without running it. The host applies the rules
//! itself in the hook at the function's start; no plugin code runs there.
//! Platform independent: the Windows adapter installs the hooks and calls
//! [`Decisions::decide`].
#![forbid(unsafe_code)]
use super::plan::{Prepared, prepare_rule};
use super::read::{Call, Memory, run_call};
use super::schema::Schema;
use std::collections::BTreeSet;
use std::sync::{
    Arc, PoisonError, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// A plugin's rule as it asks for it: the query under `handle`, the
/// outcome to take, and the domains it may read or change.
pub struct Request<'a> {
    pub plugin: &'a str,
    pub handle: u32,
    pub text: &'a str,
    pub outcome: &'a str,
    pub permitted: BTreeSet<String>,
}

/// One plugin's rule at a decision point.
pub struct Rule {
    plugin: String,
    handle: u32,
    text: String,
    /// The domains the plugin may read or change, for planning again.
    permitted: BTreeSet<String>,
    /// `None` when the rule no longer fits a reloaded map.
    plan: Option<Prepared>,
    /// The value the function returns when the rule holds.
    returns: u64,
}

impl Rule {
    #[must_use]
    pub fn new(
        plugin: &str,
        handle: u32,
        text: &str,
        permitted: BTreeSet<String>,
        plan: Prepared,
        returns: u64,
    ) -> Self {
        Self {
            plugin: plugin.to_owned(),
            handle,
            text: text.to_owned(),
            permitted,
            plan: Some(plan),
            returns,
        }
    }
}

/// A function of the game that rules decide at.
pub struct Point {
    pub entry: String,
    rules: RwLock<Vec<Rule>>,
    /// Whether any rule is there; a hook decides nothing otherwise.
    active: AtomicBool,
    hooked: AtomicBool,
    /// How many calls rules decided, for the host's diagnostics.
    decided: AtomicU64,
}

/// Every decision point, by id; an id stays its entry's for the process.
#[derive(Default)]
pub struct Decisions {
    points: RwLock<Vec<Arc<Point>>>,
}

impl Decisions {
    /// The id of the point at a map entry, made when it is new, and
    /// whether its hook is installed.
    pub fn point(&self, entry: &str) -> (usize, bool) {
        let mut points = self.points.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(id) = points.iter().position(|point| point.entry == entry) {
            return (id, points[id].hooked.load(Ordering::Acquire));
        }
        points.push(Arc::new(Point {
            entry: entry.to_owned(),
            rules: RwLock::new(Vec::new()),
            active: AtomicBool::new(false),
            hooked: AtomicBool::new(false),
            decided: AtomicU64::new(0),
        }));
        (points.len() - 1, false)
    }

    /// Records that the hook at the point `id` is installed.
    pub fn hooked(&self, id: usize) {
        if let Some(point) = self.get(id) {
            point.hooked.store(true, Ordering::Release);
        }
    }

    /// The id of the point at a map entry, if one was made.
    #[must_use]
    pub fn point_of(&self, entry: &str) -> Option<usize> {
        self.points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .position(|point| point.entry == entry)
    }

    fn get(&self, id: usize) -> Option<Arc<Point>> {
        self.points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .get(id)
            .cloned()
    }

    /// Adds a rule at the point `id`, after the rules already there.
    pub fn add(&self, id: usize, rule: Rule) {
        let Some(point) = self.get(id) else {
            return;
        };
        point
            .rules
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .push(rule);
        point.active.store(true, Ordering::Release);
    }

    fn retain(&self, keep: impl Fn(&Rule) -> bool) {
        for point in self
            .points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            let mut rules = point.rules.write().unwrap_or_else(PoisonError::into_inner);
            rules.retain(&keep);
            point.active.store(!rules.is_empty(), Ordering::Release);
        }
    }

    /// Ends a plugin's rule.
    pub fn remove(&self, plugin: &str, handle: u32) {
        self.retain(|rule| rule.plugin != plugin || rule.handle != handle);
    }

    /// Ends every rule of a plugin.
    pub fn remove_plugin(&self, plugin: &str) {
        self.retain(|rule| rule.plugin != plugin);
    }

    /// The game called the function of the point `id` with the registers
    /// of `call`: the value to return at once when a rule holds, the first
    /// that does in the order they were added; `None` lets the function run.
    /// A rule that cannot be read holds not.
    pub fn decide(&self, id: usize, call: &Call, memory: &dyn Memory) -> Option<u64> {
        let point = self.get(id)?;
        if !point.active.load(Ordering::Acquire) {
            return None;
        }
        let rules = point.rules.read().unwrap_or_else(PoisonError::into_inner);
        let returns = rules.iter().find_map(|rule| {
            let plan = rule.plan.as_ref()?;
            run_call(memory, plan, call)
                .ok()
                .filter(|rows| !rows.is_empty())
                .map(|_| rule.returns)
        })?;
        point.decided.fetch_add(1, Ordering::AcqRel);
        Some(returns)
    }

    /// How many calls rules decided at the point `id`.
    #[must_use]
    pub fn decided(&self, id: usize) -> u64 {
        self.get(id)
            .map_or(0, |point| point.decided.load(Ordering::Acquire))
    }

    /// Plans every rule again for a reloaded map; one that no longer fits
    /// holds never until its plugin decides again.
    pub fn replan(&self, schema: &Schema) {
        for point in self
            .points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            for rule in point
                .rules
                .write()
                .unwrap_or_else(PoisonError::into_inner)
                .iter_mut()
            {
                let permitted = rule.permitted.clone();
                rule.plan =
                    prepare_rule(schema, &rule.text, &|domain| permitted.contains(domain)).ok();
            }
        }
    }
}
