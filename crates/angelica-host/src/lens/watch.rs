//! Watches: plugins' queries from events of the map, read when the game
//! reaches the event's point and delivered in the next frame. A point is a
//! `function` or `location` entry of the map: the host installs one hook per
//! point, kept until the process exits, whatever watches and counts share
//! it; with none left it records nothing. Platform independent: the Windows
//! adapter installs the hooks and calls [`Watches::fire`].
#![forbid(unsafe_code)]
use super::plan::{Prepared, prepare_watch};
use super::read::{Call, Memory, Row, run_call};
use super::schema::Schema;
use std::collections::BTreeSet;
use std::sync::{
    Arc, Mutex, PoisonError, RwLock,
    atomic::{AtomicBool, AtomicU64, Ordering},
};

/// One plugin's watch of an event.
struct Subscriber {
    plugin: String,
    handle: u32,
    text: String,
    /// The domains the plugin may read, for planning again.
    read: BTreeSet<String>,
    /// The event watched, and the most firings kept between two frames.
    event: String,
    rate: u16,
    /// `None` when the query no longer fits a reloaded map.
    plan: Option<Prepared>,
    /// The rows of each firing since the last delivery.
    firings: Vec<Vec<Row>>,
    dropped: u32,
}

/// A point of the game's code that plugins watch: a `function` or
/// `location` entry of the map.
pub struct Point {
    pub entry: String,
    subscribers: Mutex<Vec<Subscriber>>,
    /// Whether any watch is subscribed; a hook does nothing otherwise.
    active: AtomicBool,
    /// Whether the hook at the point is installed.
    hooked: AtomicBool,
    /// Whether the point counts its calls, for `angelica look`.
    counting: AtomicBool,
    calls: AtomicU64,
}

/// What one watch receives in a frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Delivery {
    pub handle: u32,
    pub firings: Vec<Vec<Row>>,
    /// Firings left out since the last delivery: past the event's rate, or
    /// past a firing's limits.
    pub dropped: u32,
}

/// Every watched point, by id; an id stays its event's for the process.
#[derive(Default)]
pub struct Watches {
    points: RwLock<Vec<Arc<Point>>>,
}

impl Watches {
    /// The id of the point at a map entry, made when it is new, and
    /// whether its hook is installed.
    pub fn point(&self, entry: &str) -> (usize, bool) {
        let mut points = self.points.write().unwrap_or_else(PoisonError::into_inner);
        if let Some(id) = points.iter().position(|point| point.entry == entry) {
            return (id, points[id].hooked.load(Ordering::Acquire));
        }
        points.push(Arc::new(Point {
            entry: entry.to_owned(),
            subscribers: Mutex::new(Vec::new()),
            active: AtomicBool::new(false),
            hooked: AtomicBool::new(false),
            counting: AtomicBool::new(false),
            calls: AtomicU64::new(0),
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

    /// Adds a plugin's watch of `event`, at the point `id`, keeping at most
    /// `rate` firings between two frames.
    #[allow(clippy::too_many_arguments)]
    pub fn subscribe(
        &self,
        id: usize,
        plugin: &str,
        handle: u32,
        text: &str,
        read: BTreeSet<String>,
        event: &str,
        rate: u16,
        plan: Prepared,
    ) {
        let Some(point) = self.get(id) else {
            return;
        };
        let mut subscribers = point
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        subscribers.push(Subscriber {
            plugin: plugin.to_owned(),
            handle,
            text: text.to_owned(),
            read,
            event: event.to_owned(),
            rate,
            plan: Some(plan),
            firings: Vec::new(),
            dropped: 0,
        });
        point.active.store(true, Ordering::Release);
    }

    fn retain(&self, keep: impl Fn(&Subscriber) -> bool) {
        for point in self
            .points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            let mut subscribers = point
                .subscribers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            subscribers.retain(&keep);
            point
                .active
                .store(!subscribers.is_empty(), Ordering::Release);
        }
    }

    /// Starts counting the calls at the point `id` from zero.
    pub fn count(&self, id: usize) {
        if let Some(point) = self.get(id) {
            point.calls.store(0, Ordering::Release);
            point.counting.store(true, Ordering::Release);
        }
    }

    /// Stops counting the calls at the point `id`, and how many there were.
    pub fn counted(&self, id: usize) -> u64 {
        self.get(id).map_or(0, |point| {
            point.counting.store(false, Ordering::Release);
            point.calls.load(Ordering::Acquire)
        })
    }

    /// Ends a plugin's watch.
    pub fn unsubscribe(&self, plugin: &str, handle: u32) {
        self.retain(|subscriber| subscriber.plugin != plugin || subscriber.handle != handle);
    }

    /// Ends every watch of a plugin.
    pub fn remove_plugin(&self, plugin: &str) {
        self.retain(|subscriber| subscriber.plugin != plugin);
    }

    /// The game reached the point `id` with the registers of `call`: every
    /// watch reads its rows now. A firing whose rows are empty (its filter
    /// left everything out) is not delivered.
    pub fn fire(&self, id: usize, call: &Call, memory: &dyn Memory) {
        let Some(point) = self.get(id) else {
            return;
        };
        if point.counting.load(Ordering::Acquire) {
            point.calls.fetch_add(1, Ordering::AcqRel);
        }
        if !point.active.load(Ordering::Acquire) {
            return;
        }
        let mut subscribers = point
            .subscribers
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for subscriber in subscribers.iter_mut() {
            let Some(plan) = &subscriber.plan else {
                continue;
            };
            match run_call(memory, plan, call) {
                Ok(rows) if rows.is_empty() => {}
                Ok(rows) if subscriber.firings.len() < usize::from(subscriber.rate) => {
                    subscriber.firings.push(rows);
                }
                Ok(_) | Err(_) => subscriber.dropped = subscriber.dropped.saturating_add(1),
            }
        }
    }

    /// What a plugin's watches received since the last call.
    pub fn take(&self, plugin: &str) -> Vec<Delivery> {
        let mut deliveries = Vec::new();
        for point in self
            .points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            if !point.active.load(Ordering::Acquire) {
                continue;
            }
            let mut subscribers = point
                .subscribers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for subscriber in subscribers
                .iter_mut()
                .filter(|subscriber| subscriber.plugin == plugin)
            {
                if subscriber.firings.is_empty() && subscriber.dropped == 0 {
                    continue;
                }
                deliveries.push(Delivery {
                    handle: subscriber.handle,
                    firings: std::mem::take(&mut subscriber.firings),
                    dropped: std::mem::take(&mut subscriber.dropped),
                });
            }
        }
        deliveries.sort_by_key(|delivery| delivery.handle);
        deliveries
    }

    /// Plans every watch again for a reloaded map; a watch that no longer
    /// fits receives nothing until its plugin watches again.
    pub fn replan(&self, schema: &Schema) {
        for point in self
            .points
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
        {
            let mut subscribers = point
                .subscribers
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            for subscriber in subscribers.iter_mut() {
                let read = &subscriber.read;
                let event = &subscriber.event;
                subscriber.plan =
                    prepare_watch(schema, &subscriber.text, &|domain| read.contains(domain))
                        .ok()
                        .filter(|plan| {
                            plan.event() == Some(event.as_str())
                                && schema
                                    .objects
                                    .events
                                    .get(event)
                                    .is_some_and(|found| found.at == point.entry)
                        });
            }
        }
    }
}
