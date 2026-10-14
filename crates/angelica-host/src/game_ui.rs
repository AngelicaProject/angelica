//! Rules for the game's own windows: elements plugins keep hidden and text
//! they put in text elements. A plugin names a window and an element by the
//! game map's window schemas; the rules are applied every frame on the main
//! thread through [`Screen`], which the game adapter implements, so the
//! plugin's code never runs when the game draws.
#![forbid(unsafe_code)]
use crate::{
    error::{Error, Kind, Result},
    game_data::GameData,
};
use angelica_map::{ElementKind, WindowSchema, window_layout_fingerprint};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{Mutex, PoisonError, RwLock},
};

/// The most elements one plugin hides.
pub const MAX_HIDDEN: usize = 256;
/// The most elements one plugin puts text in.
pub const MAX_TEXTS: usize = 64;
/// The most bytes of one plugin's text for an element.
pub const MAX_TEXT: usize = 256;
/// The most bytes an element's text has with the plugins' texts; longer
/// text is left as the game wrote it.
pub const MAX_ELEMENT_TEXT: usize = 4096;

/// A named element of a named window.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Target {
    pub window: String,
    pub element: String,
}

/// Where a plugin's text goes in an element.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// After the game's text.
    After,
    /// Before the game's text.
    Before,
    /// In place of the game's text.
    Instead,
}

/// A plugin's text for an element.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextRule {
    pub target: Target,
    pub text: String,
    pub placement: Placement,
}

/// An element of a live window. The handle differs for another element
/// and after the game builds the window again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Element {
    pub handle: usize,
}

/// The game's windows as the rules need them. Used on the main thread only.
pub trait Screen {
    /// The element `id` of the window named `window`, when the window
    /// exists and is built; with `text`, only when it is a text element.
    fn element(&mut self, window: &str, id: u32, text: bool) -> Option<Element>;
    fn visible(&mut self, element: Element) -> bool;
    fn set_visible(&mut self, element: Element, visible: bool);
    /// The element's text, in the game's encoding.
    fn text(&mut self, element: Element) -> Vec<u8>;
    fn set_text(&mut self, element: Element, text: &[u8]);
}

#[derive(Default)]
struct PluginRules {
    hidden: Vec<Target>,
    texts: Vec<TextRule>,
}

/// What every plugin together asks of one element.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Planned {
    hide: bool,
    /// The plugins' texts, in the order of plugin ids.
    texts: Vec<(Placement, String)>,
}

type Plan = BTreeMap<(String, u32), Planned>;

/// What the host did to a live element and has to undo.
#[derive(Default)]
struct Held {
    handle: usize,
    hid: bool,
    /// The text the game wrote, and the text the host wrote over it.
    text: Option<(Vec<u8>, Vec<u8>)>,
}

/// The window schemas of the game map and the plugins' rules.
#[derive(Default)]
pub struct GameUi {
    schemas: RwLock<BTreeMap<String, WindowSchema>>,
    resolved: Mutex<BTreeMap<String, std::result::Result<Vec<NamedId>, String>>>,
    rules: Mutex<BTreeMap<String, PluginRules>>,
    plan: Mutex<Option<Plan>>,
    held: Mutex<BTreeMap<(String, u32), Held>>,
}

/// A named element in the running game's layout of its window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedId {
    pub name: String,
    pub id: u32,
    pub kind: ElementKind,
}

fn is_name(name: &str, most: usize) -> bool {
    !name.is_empty()
        && name.len() <= most
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn check_target(target: &Target) -> Result<()> {
    if is_name(&target.window, 31) && is_name(&target.element, 64) {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "{}.{} is not a window and an element name",
            target.window, target.element
        )))
    }
}

/// The game's text with the plugins' texts.
fn compose(original: &[u8], texts: &[(Placement, String)]) -> Vec<u8> {
    let instead = texts
        .iter()
        .find(|(placement, _)| *placement == Placement::Instead)
        .map(|(_, text)| text.as_bytes());
    let mut composed = Vec::new();
    for (_, text) in texts.iter().filter(|(p, _)| *p == Placement::Before) {
        composed.extend_from_slice(text.as_bytes());
    }
    composed.extend_from_slice(instead.unwrap_or(original));
    for (_, text) in texts.iter().filter(|(p, _)| *p == Placement::After) {
        composed.extend_from_slice(text.as_bytes());
    }
    if composed.len() > MAX_ELEMENT_TEXT {
        return original.to_vec();
    }
    composed
}

impl GameUi {
    #[must_use]
    pub fn new(schemas: impl IntoIterator<Item = WindowSchema>) -> Self {
        let ui = Self::default();
        ui.set(schemas);
        ui
    }

    /// Uses these schemas from now on.
    pub fn set(&self, schemas: impl IntoIterator<Item = WindowSchema>) {
        *self.schemas.write().unwrap_or_else(PoisonError::into_inner) = schemas
            .into_iter()
            .map(|schema| (schema.window.clone(), schema))
            .collect();
        self.resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
        self.forget_plan();
    }

    fn forget_plan(&self) {
        *self.plan.lock().unwrap_or_else(PoisonError::into_inner) = None;
    }

    /// The named elements of `window` in the running game, sorted by name.
    ///
    /// # Errors
    /// `unavailable` when the map names no elements of the window or does
    /// not know the game's layout file of it; the game data's errors
    /// otherwise.
    pub fn elements(&self, window: &str, data: &GameData) -> Result<Vec<NamedId>> {
        if let Some(known) = self
            .resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(window)
        {
            return known
                .clone()
                .map_err(|reason| Error::new(Kind::Unavailable, reason));
        }
        let result = {
            let schemas = self.schemas.read().unwrap_or_else(PoisonError::into_inner);
            match schemas.get(window) {
                None => Err(format!("the game map names no elements of window {window}")),
                Some(schema) => {
                    let layout = data.read(&schema.layout)?;
                    schema
                        .resolve(&window_layout_fingerprint(&layout))
                        .map(|ids| {
                            ids.into_iter()
                                .map(|(name, id)| NamedId {
                                    kind: schema.elements[&name].kind,
                                    name,
                                    id,
                                })
                                .collect()
                        })
                }
            }
        };
        self.resolved
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(window.to_owned(), result.clone());
        result.map_err(|reason| Error::new(Kind::Unavailable, reason))
    }

    fn find(&self, target: &Target, data: &GameData) -> Option<NamedId> {
        self.elements(&target.window, data)
            .ok()?
            .into_iter()
            .find(|named| named.name == target.element)
    }

    /// Replaces the elements `plugin` keeps hidden; an empty list shows
    /// them again. Returns the elements that are not available in this game
    /// build, which stay as the game shows them.
    ///
    /// # Errors
    /// Refuses malformed names and more than [`MAX_HIDDEN`] elements.
    pub fn set_hidden(
        &self,
        plugin: &str,
        targets: Vec<Target>,
        data: &GameData,
    ) -> Result<Vec<Target>> {
        if targets.len() > MAX_HIDDEN {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a plugin hides at most {MAX_HIDDEN} elements"),
            ));
        }
        targets.iter().try_for_each(check_target)?;
        let targets: Vec<Target> = targets
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let unavailable = targets
            .iter()
            .filter(|target| self.find(target, data).is_none())
            .cloned()
            .collect();
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(plugin.to_owned())
            .or_default()
            .hidden = targets;
        self.forget_plan();
        Ok(unavailable)
    }

    /// Replaces the texts `plugin` puts in elements; an empty list removes
    /// them. Returns the elements that are not available in this game
    /// build, which keep the game's text.
    ///
    /// # Errors
    /// Refuses malformed names, an element that does not show text, two
    /// texts for one element, text that is too long or has control
    /// characters, and more than [`MAX_TEXTS`] texts.
    pub fn set_texts(
        &self,
        plugin: &str,
        rules: Vec<TextRule>,
        data: &GameData,
    ) -> Result<Vec<Target>> {
        if rules.len() > MAX_TEXTS {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a plugin puts text in at most {MAX_TEXTS} elements"),
            ));
        }
        let mut seen = BTreeSet::new();
        let mut unavailable = Vec::new();
        for rule in &rules {
            check_target(&rule.target)?;
            let target = &rule.target;
            if !seen.insert(target) {
                return Err(Error::invalid(format!(
                    "{}.{} has two texts",
                    target.window, target.element
                )));
            }
            if rule.text.len() > MAX_TEXT {
                return Err(Error::new(
                    Kind::LimitExceeded,
                    format!(
                        "the text for {}.{} is longer than {MAX_TEXT} bytes",
                        target.window, target.element
                    ),
                ));
            }
            if rule.text.chars().any(char::is_control) {
                return Err(Error::invalid(format!(
                    "the text for {}.{} has control characters",
                    target.window, target.element
                )));
            }
            match self.find(target, data) {
                None => unavailable.push(target.clone()),
                Some(named) if named.kind != ElementKind::Text => {
                    return Err(Error::invalid(format!(
                        "{}.{} does not show text",
                        target.window, target.element
                    )));
                }
                Some(_) => {}
            }
        }
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(plugin.to_owned())
            .or_default()
            .texts = rules;
        self.forget_plan();
        Ok(unavailable)
    }

    /// Removes the plugin's rules; the next frame undoes them.
    pub fn remove_plugin(&self, plugin: &str) {
        if self
            .rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(plugin)
            .is_some()
        {
            self.forget_plan();
        }
    }

    /// Whether a frame has anything to apply or undo.
    #[must_use]
    pub fn idle(&self) -> bool {
        self.rules
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .values()
            .all(|rules| rules.hidden.is_empty() && rules.texts.is_empty())
            && self
                .held
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .is_empty()
    }

    fn plan(&self, data: &GameData) -> Plan {
        if let Some(plan) = self
            .plan
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
        {
            return plan.clone();
        }
        let mut plan = Plan::new();
        {
            let rules = self.rules.lock().unwrap_or_else(PoisonError::into_inner);
            for plugin in rules.values() {
                for target in &plugin.hidden {
                    if let Some(named) = self.find(target, data) {
                        plan.entry((target.window.clone(), named.id))
                            .or_default()
                            .hide = true;
                    }
                }
                for rule in &plugin.texts {
                    if let Some(named) = self.find(&rule.target, data)
                        && named.kind == ElementKind::Text
                    {
                        plan.entry((rule.target.window.clone(), named.id))
                            .or_default()
                            .texts
                            .push((rule.placement, rule.text.clone()));
                    }
                }
            }
        }
        *self.plan.lock().unwrap_or_else(PoisonError::into_inner) = Some(plan.clone());
        plan
    }

    /// One frame: undoes what no plugin asks for any more, hides what is
    /// visible again, and writes the plugins' texts where the game wrote
    /// its own. On the main thread only.
    pub fn apply(&self, screen: &mut dyn Screen, data: &GameData) {
        let plan = self.plan(data);
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.retain(|key, held| {
            let planned = plan.get(key);
            let element = screen
                .element(&key.0, key.1, held.text.is_some())
                .filter(|element| element.handle == held.handle);
            let Some(element) = element else {
                // The game removed the element or built it again as it
                // wants it.
                *held = Held::default();
                return planned.is_some();
            };
            if held.hid && !planned.is_some_and(|planned| planned.hide) {
                screen.set_visible(element, true);
                held.hid = false;
            }
            if planned.is_none_or(|planned| planned.texts.is_empty())
                && let Some((original, written)) = held.text.take()
                && screen.text(element) == written
            {
                screen.set_text(element, &original);
            }
            planned.is_some()
        });
        for (key, planned) in &plan {
            let text = !planned.texts.is_empty();
            let Some(element) = screen.element(&key.0, key.1, text) else {
                continue;
            };
            let entry = held.entry(key.clone()).or_default();
            if entry.handle != element.handle {
                *entry = Held {
                    handle: element.handle,
                    ..Held::default()
                };
            }
            if planned.hide && screen.visible(element) {
                screen.set_visible(element, false);
                entry.hid = true;
            }
            if text {
                let current = screen.text(element);
                let original = match entry.text.take() {
                    Some((original, written)) if written == current => original,
                    // The game wrote its text again.
                    _ => current.clone(),
                };
                let wanted = compose(&original, &planned.texts);
                if wanted != current {
                    screen.set_text(element, &wanted);
                }
                entry.text = Some((original, wanted));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A live element: its handle, whether it shows text, whether it is
    /// visible, and its text.
    type Live = (usize, bool, bool, Vec<u8>);

    /// Windows of live elements by window and id.
    #[derive(Default)]
    struct Windows {
        elements: BTreeMap<(String, u32), Live>,
        writes: usize,
    }

    impl Windows {
        fn with(mut self, window: &str, id: u32, text: Option<&str>) -> Self {
            let handle = self.elements.len() + 1;
            self.elements.insert(
                (window.into(), id),
                (
                    handle,
                    text.is_some(),
                    true,
                    text.unwrap_or_default().into(),
                ),
            );
            self
        }

        fn at(&mut self, handle: usize) -> &mut Live {
            self.elements
                .values_mut()
                .find(|element| element.0 == handle)
                .unwrap()
        }

        fn text_of(&self, window: &str, id: u32) -> String {
            String::from_utf8(self.elements[&(window.to_owned(), id)].3.clone()).unwrap()
        }

        fn game_writes(&mut self, window: &str, id: u32, text: &str) {
            self.elements.get_mut(&(window.to_owned(), id)).unwrap().3 = text.into();
        }
    }

    impl Screen for Windows {
        fn element(&mut self, window: &str, id: u32, text: bool) -> Option<Element> {
            self.elements
                .get(&(window.to_owned(), id))
                .filter(|element| element.1 || !text)
                .map(|element| Element { handle: element.0 })
        }
        fn visible(&mut self, element: Element) -> bool {
            self.at(element.handle).2
        }
        fn set_visible(&mut self, element: Element, visible: bool) {
            self.at(element.handle).2 = visible;
            self.writes += 1;
        }
        fn text(&mut self, element: Element) -> Vec<u8> {
            self.at(element.handle).3.clone()
        }
        fn set_text(&mut self, element: Element, text: &[u8]) {
            self.at(element.handle).3 = text.to_vec();
            self.writes += 1;
        }
    }

    const LAYOUT: &[u8] = b"uldh0100 the experience bar";

    fn schema(fingerprint: &str) -> WindowSchema {
        let text = format!(
            r#"
window = "_Exp"
doc = "The experience bar."
layout = "ui/uld/Exp.uld"

[elements.text]
kind = "text"
doc = "The class, level, and experience."

[elements.bar]
kind = "other"
doc = "The bar."

[[layouts]]
fingerprint = "{fingerprint}"
elements = {{ text = 4, bar = 6 }}
"#
        );
        angelica_map::Map::from_sources("format = 2", &[("windows/_Exp.toml".into(), text)])
            .unwrap()
            .window("_Exp")
            .unwrap()
            .clone()
    }

    fn game() -> (tempfile::TempDir, GameData) {
        let folder = tempfile::tempdir().unwrap();
        angelica_sqpack::testing::FakeGame::new("1")
            .with_file("ui/uld/Exp.uld", LAYOUT.to_vec())
            .write(folder.path())
            .unwrap();
        let data = GameData::new(Some(folder.path()));
        (folder, data)
    }

    fn target(element: &str) -> Target {
        Target {
            window: "_Exp".into(),
            element: element.into(),
        }
    }

    fn text(element: &str, text: &str, placement: Placement) -> TextRule {
        TextRule {
            target: target(element),
            text: text.into(),
            placement,
        }
    }

    fn ui() -> GameUi {
        GameUi::new([schema(&window_layout_fingerprint(LAYOUT))])
    }

    #[test]
    fn names_resolve_only_for_the_layout_file_the_map_knows() {
        let (_folder, data) = game();
        let named = ui().elements("_Exp", &data).unwrap();
        assert_eq!(
            named
                .iter()
                .map(|named| (named.name.as_str(), named.id))
                .collect::<Vec<_>>(),
            [("bar", 6), ("text", 4)]
        );
        let other = GameUi::new([schema("0123456789abcdef")]);
        let error = other.elements("_Exp", &data).unwrap_err();
        assert_eq!(error.kind, Kind::Unavailable);
        assert!(error.message.contains("does not know layout"));
        assert_eq!(
            other.set_hidden("a", vec![target("bar")], &data).unwrap(),
            [target("bar")]
        );
        assert_eq!(
            ui().elements("_Money", &data).unwrap_err().kind,
            Kind::Unavailable
        );
    }

    #[test]
    fn a_hidden_element_stays_hidden_and_shows_again_without_the_rule() {
        let (_folder, data) = game();
        let ui = ui();
        let mut windows = Windows::default().with("_Exp", 6, None);
        assert!(ui.idle());
        assert!(
            ui.set_hidden("a", vec![target("bar"), target("bar")], &data)
                .unwrap()
                .is_empty()
        );
        ui.apply(&mut windows, &data);
        assert!(!windows.elements[&("_Exp".into(), 6)].2);
        let writes = windows.writes;
        ui.apply(&mut windows, &data);
        assert_eq!(windows.writes, writes, "nothing to do while it is hidden");
        // The game shows it again.
        windows.elements.get_mut(&("_Exp".into(), 6)).unwrap().2 = true;
        ui.apply(&mut windows, &data);
        assert!(!windows.elements[&("_Exp".into(), 6)].2);
        ui.remove_plugin("a");
        ui.apply(&mut windows, &data);
        assert!(windows.elements[&("_Exp".into(), 6)].2);
        assert!(ui.idle());
    }

    #[test]
    fn texts_follow_the_games_text_and_leave_with_the_rule() {
        let (_folder, data) = game();
        let ui = ui();
        let mut windows = Windows::default().with("_Exp", 4, Some("EXP 10/20"));
        ui.set_texts("b", vec![text("text", " (50%)", Placement::After)], &data)
            .unwrap();
        ui.set_texts("a", vec![text("text", "» ", Placement::Before)], &data)
            .unwrap();
        ui.apply(&mut windows, &data);
        assert_eq!(windows.text_of("_Exp", 4), "» EXP 10/20 (50%)");
        let writes = windows.writes;
        ui.apply(&mut windows, &data);
        assert_eq!(windows.writes, writes);
        windows.game_writes("_Exp", 4, "EXP 15/20");
        ui.apply(&mut windows, &data);
        assert_eq!(windows.text_of("_Exp", 4), "» EXP 15/20 (50%)");
        // The plugin's value changes.
        ui.set_texts("b", vec![text("text", " (75%)", Placement::After)], &data)
            .unwrap();
        ui.apply(&mut windows, &data);
        assert_eq!(windows.text_of("_Exp", 4), "» EXP 15/20 (75%)");
        ui.set_texts("b", vec![text("text", "75%", Placement::Instead)], &data)
            .unwrap();
        ui.apply(&mut windows, &data);
        assert_eq!(windows.text_of("_Exp", 4), "» 75%");
        ui.remove_plugin("a");
        ui.remove_plugin("b");
        ui.apply(&mut windows, &data);
        assert_eq!(windows.text_of("_Exp", 4), "EXP 15/20");
        assert!(ui.idle());
    }

    #[test]
    fn a_rebuilt_window_is_treated_as_the_game_left_it() {
        let (_folder, data) = game();
        let ui = ui();
        let mut windows = Windows::default().with("_Exp", 4, Some("EXP 1/2"));
        ui.set_texts("a", vec![text("text", "!", Placement::After)], &data)
            .unwrap();
        ui.apply(&mut windows, &data);
        // The window closes: nothing is there to change.
        let mut closed = Windows::default();
        ui.apply(&mut closed, &data);
        assert_eq!(closed.writes, 0);
        // It opens again with new elements.
        let mut reopened =
            Windows::default()
                .with("_Other", 1, None)
                .with("_Exp", 4, Some("EXP 2/2"));
        ui.apply(&mut reopened, &data);
        assert_eq!(reopened.text_of("_Exp", 4), "EXP 2/2!");
        ui.remove_plugin("a");
        ui.apply(&mut reopened, &data);
        assert_eq!(reopened.text_of("_Exp", 4), "EXP 2/2");
    }

    #[test]
    fn rules_are_validated_and_bounded() {
        let (_folder, data) = game();
        let ui = ui();
        let invalid = |result: Result<Vec<Target>>| result.unwrap_err().kind;
        assert_eq!(
            invalid(ui.set_hidden(
                "a",
                vec![Target {
                    window: "_Exp".into(),
                    element: "no name".into()
                }],
                &data
            )),
            Kind::InvalidArgument
        );
        assert_eq!(
            invalid(ui.set_hidden("a", vec![target("bar"); MAX_HIDDEN + 1], &data)),
            Kind::LimitExceeded
        );
        assert_eq!(
            invalid(ui.set_texts("a", vec![text("bar", "x", Placement::After)], &data)),
            Kind::InvalidArgument,
            "not a text element"
        );
        assert_eq!(
            invalid(ui.set_texts(
                "a",
                vec![
                    text("text", "x", Placement::After),
                    text("text", "y", Placement::Before)
                ],
                &data
            )),
            Kind::InvalidArgument
        );
        assert_eq!(
            invalid(ui.set_texts("a", vec![text("text", "a\u{2}b", Placement::After)], &data)),
            Kind::InvalidArgument
        );
        assert_eq!(
            invalid(ui.set_texts(
                "a",
                vec![text("text", &"x".repeat(MAX_TEXT + 1), Placement::After)],
                &data
            )),
            Kind::LimitExceeded
        );
        // An element the map does not name is reported, not an error.
        assert_eq!(
            ui.set_hidden("a", vec![target("gauge")], &data).unwrap(),
            [target("gauge")]
        );
        let mut windows = Windows::default().with("_Exp", 4, Some(&"x".repeat(MAX_ELEMENT_TEXT)));
        ui.set_texts("a", vec![text("text", "!", Placement::After)], &data)
            .unwrap();
        ui.apply(&mut windows, &data);
        assert_eq!(windows.text_of("_Exp", 4).len(), MAX_ELEMENT_TEXT);
    }
}
