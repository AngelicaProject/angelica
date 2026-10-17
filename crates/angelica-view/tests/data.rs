use angelica_view::data::{
    RawPart, RawPlace, Value, decode, decode_event, decode_popup, encode_event, encode_places,
    encode_popup,
};
use angelica_view::{
    Action, ActionKind, Anchor, Backing, Button, Checkbox, Choice, Chosen, Countdown, Dialog,
    Edited, Entry, Event, Fact, Icon, Mark, Meter, Navigated, Number, NumberSet, Panel, Place,
    Popup, PopupAt, Readout, Ring, Role, Section, Shown, Sparkline, Split, Status, Table, Tabs,
    Text, TextInput, Toggled, Tone, Widget, WidgetRef, Window,
};

fn every_part() -> Vec<Widget> {
    vec![
        Widget::Heading("Packs".into()),
        Widget::Text(Text {
            text: "35 250".into(),
            tone: Tone::Warning,
            role: Role::Big,
        }),
        Widget::Tooltip("HP".into()),
        Widget::BeginRow,
        Widget::Tile("Retainers".into()),
        Widget::Spacer,
        Widget::Countdown(Countdown {
            until: 1_792_000_000_000,
            done: "back".into(),
            role: Role::Value,
        }),
        Widget::EndRow,
        Widget::BeginColumn,
        Widget::Meter(Meter {
            label: "HP".into(),
            text: "35 250 / 52 000".into(),
            value: 35250.0,
            max: 52000.0,
            segments: 0,
            tone: Tone::Good,
        }),
        Widget::Meter(Meter {
            label: "Charges".into(),
            text: String::new(),
            value: 2.0,
            max: 3.0,
            segments: 3,
            tone: Tone::Normal,
        }),
        Widget::EndColumn,
        Widget::BeginGrid(3),
        Widget::Ring(Ring {
            value: 18.0,
            max: 60.0,
            size: 44,
            icon: Some(62_001),
            text: String::new(),
            tone: Tone::Normal,
        }),
        Widget::Ring(Ring {
            value: 7.0,
            max: 10.0,
            size: 40,
            icon: None,
            text: "70%".into(),
            tone: Tone::Good,
        }),
        Widget::Readout(Readout {
            id: "damage".into(),
            value: 48_210.0,
            decimals: 0,
            unit: String::new(),
            role: Role::Value,
            tone: Tone::Normal,
        }),
        Widget::EndGrid,
    ]
}

fn more_parts() -> Vec<Widget> {
    vec![
        // Required texts survive when empty.
        Widget::Text(Text {
            text: String::new(),
            tone: Tone::Normal,
            role: Role::Body,
        }),
        Widget::Heading(String::new()),
        Widget::Sparkline(Sparkline {
            values: vec![3.0, 5.0, 4.0, 8.0],
            height: 36,
        }),
        Widget::Separator,
        Widget::BeginSection(Section {
            id: "more".into(),
            title: "More".into(),
            open_by_default: false,
        }),
        Widget::Fact(Fact {
            name: "Version".into(),
            value: "1.2".into(),
        }),
        Widget::EndSection,
        Widget::Tabs(Tabs {
            id: "tabs".into(),
            labels: vec!["One".into(), "Two".into()],
            selected: 1,
        }),
        Widget::BeginTable(Table {
            id: "items".into(),
            columns: vec!["Item".into(), "Count".into()],
        }),
        Widget::Icon(Icon {
            icon: 65_002,
            size: 20,
        }),
        Widget::Text(Text {
            text: "3".into(),
            tone: Tone::Normal,
            role: Role::Body,
        }),
        Widget::EndTable,
        Widget::Entry(Entry {
            id: "garden".into(),
            title: "Garden".into(),
            note: "by the house".into(),
            value: "1:28".into(),
            mark: Mark::On,
            chosen: true,
            progress: Some(0.5),
            icon: Some(60_001),
            plain: true,
            actions: vec![Action {
                id: "water".into(),
                label: "Water".into(),
                kind: ActionKind::Normal,
                enabled: true,
            }],
            facts: vec![Fact {
                name: "Seeds".into(),
                value: "Thavnairian onion".into(),
            }],
            more: vec![Action {
                id: "remove".into(),
                label: "Remove".into(),
                kind: ActionKind::Danger,
                enabled: false,
            }],
        }),
        Widget::Button(Button {
            id: "start".into(),
            label: "Start".into(),
            enabled: true,
            kind: ActionKind::Main,
        }),
        Widget::TextInput(TextInput {
            id: "feed".into(),
            label: "Feed".into(),
            value: "https://".into(),
            hint: "a link".into(),
        }),
        Widget::Choice(Choice {
            id: "language".into(),
            label: "Language".into(),
            options: vec!["en".into(), "ru".into()],
            selected: None,
            hint: "the interface's language".into(),
        }),
        Widget::Number(Number {
            id: "volume".into(),
            label: "Volume".into(),
            value: 2.5,
            min: 0.0,
            max: 10.0,
            step: 0.5,
            decimals: 1,
            slider: true,
        }),
    ]
}

fn window() -> Place {
    Place::Window(Window {
        id: "main".into(),
        title: "Timers".into(),
        path: vec!["Settings".into()],
        status: Some(Status {
            text: "3 running".into(),
            tone: Tone::Good,
        }),
        widgets: every_part()
            .into_iter()
            .chain(more_parts())
            .chain([
                Widget::Checkbox(Checkbox {
                    id: "daily".into(),
                    label: "Daily".into(),
                    checked: true,
                    words: None,
                    hint: "Every day at the same time.".into(),
                }),
                Widget::Checkbox(Checkbox {
                    id: "names".into(),
                    label: "Names".into(),
                    checked: false,
                    words: Some(["Translated".into(), "Original".into()]),
                    hint: String::new(),
                }),
                Widget::BeginSplit(Split {
                    id: "groups".into(),
                    labels: vec!["Items".into(), "Places".into(), "Crafting".into()],
                    values: vec!["½".into()],
                    groups: vec![String::new(), String::new(), "Mine".into()],
                    selected: 1,
                    width: 180,
                    action: "+ Group".into(),
                }),
                Widget::Heading("Places".into()),
                Widget::EndSplit,
            ])
            .collect(),
        dialog: Some(Dialog {
            id: "trust".into(),
            title: "Trust this publisher?".into(),
            widgets: vec![
                Widget::Fact(Fact {
                    name: "Key".into(),
                    value: "3F9A".into(),
                }),
                Widget::Button(Button {
                    id: "yes".into(),
                    label: "Trust".into(),
                    enabled: true,
                    kind: ActionKind::Main,
                }),
            ],
        }),
    })
}

fn panel() -> Place {
    Place::Panel(Panel {
        id: "health".into(),
        name: "Health".into(),
        backing: Backing::Bare,
        anchor: Anchor::Bottom,
        shown: Shown::InCombat,
        widgets: vec![Widget::Meter(Meter {
            label: "HP".into(),
            text: String::new(),
            value: 1.0,
            max: 2.0,
            segments: 0,
            tone: Tone::Normal,
        })],
    })
}

#[test]
fn every_place_and_part_survives_the_data() {
    let places = vec![window(), panel()];
    let raw = encode_places(&places);
    assert_eq!(raw.len(), 3, "the window's dialog is a place of its own");
    assert_eq!(decode(&raw, 0), Ok(places));

    let popup = Popup {
        at: PopupAt::Panel("health".into()),
        seconds: 3,
        attention: true,
        widgets: vec![Widget::Text(Text {
            text: "HP below a third".into(),
            tone: Tone::Warning,
            role: Role::Title,
        })],
    };
    assert_eq!(decode_popup(&encode_popup(&popup), 0), Ok(popup));
}

#[test]
fn events_survive_the_data_and_unknown_ones_are_skipped() {
    let r = |id: &str| WidgetRef {
        window: "main".into(),
        id: id.into(),
    };
    for event in [
        Event::Clicked(r("start")),
        Event::Toggled(Toggled {
            window: "main".into(),
            id: "daily".into(),
            checked: true,
        }),
        Event::Edited(Edited {
            window: "main".into(),
            id: "feed".into(),
            value: "x".into(),
        }),
        Event::Chosen(Chosen {
            window: "main".into(),
            id: "tabs".into(),
            index: 1,
        }),
        Event::Set(NumberSet {
            window: "main".into(),
            id: "volume".into(),
            value: 3.0,
        }),
        Event::Closed("main".into()),
        Event::Navigated(Navigated {
            window: "main".into(),
            depth: 0,
        }),
        Event::OpenRequested,
    ] {
        assert_eq!(decode_event(&encode_event(&event)), Some(event));
    }
    let mut later = encode_event(&Event::OpenRequested);
    later.kind = "dragged".into();
    assert_eq!(decode_event(&later), None);
}

fn part(kind: &str, id: &str, properties: &[(&str, Value)]) -> RawPart {
    RawPart {
        kind: kind.into(),
        id: id.into(),
        properties: properties
            .iter()
            .map(|(name, value)| ((*name).into(), value.clone()))
            .collect(),
    }
}

fn window_of(parts: Vec<RawPart>) -> Vec<RawPlace> {
    vec![RawPlace {
        kind: "window".into(),
        id: "main".into(),
        properties: vec![("title".into(), Value::Text("Main".into()))],
        parts,
    }]
}

#[test]
fn kinds_properties_and_shapes_the_api_does_not_list_are_refused() {
    let text = |t: &str| Value::Text(t.into());
    for (parts, says) in [
        (vec![part("gauge", "", &[])], "no kind \"gauge\""),
        (
            vec![part(
                "text",
                "",
                &[("text", text("a")), ("color", text("red"))],
            )],
            "no property \"color\"",
        ),
        (
            vec![part("text", "", &[("text", Value::Integer(3))])],
            "is not a text",
        ),
        (vec![part("text", "", &[])], "needs text"),
        (
            vec![part(
                "text",
                "",
                &[("text", text("a")), ("text", text("b"))],
            )],
            "given twice",
        ),
        (
            vec![part(
                "text",
                "",
                &[("text", text("a")), ("role", text("huge"))],
            )],
            "not a role",
        ),
        (
            vec![part(
                "meter",
                "",
                &[("value", Value::Number(1.0)), ("max", Value::Number(0.0))],
            )],
            "max above 0",
        ),
        (
            vec![part(
                "ring",
                "",
                &[
                    ("value", Value::Number(1.0)),
                    ("max", Value::Number(2.0)),
                    ("size", Value::Integer(8)),
                ],
            )],
            "not 16 to 128",
        ),
        (
            vec![part("button", "", &[("label", text("Go"))])],
            "needs an id",
        ),
        (vec![part("text", "x", &[("text", text("a"))])], "has no id"),
        (vec![part("row", "", &[])], "not ended"),
        (vec![part("end", "", &[])], "closes nothing"),
        (vec![part("window", "", &[])], "is a place"),
        (
            vec![part(
                "button",
                "go",
                &[("label", text("Go")), ("in-row", Value::Flag(true))],
            )],
            "in an entry",
        ),
    ] {
        let error = decode(&window_of(parts), 0).unwrap_err();
        assert!(error.contains(says), "{says}: {error}");
    }
}

#[test]
fn the_old_rules_of_windows_still_hold() {
    let tooltip = part("tooltip", "", &[("text", Value::Text("alone".into()))]);
    assert!(
        decode(&window_of(vec![tooltip]), 0)
            .unwrap_err()
            .contains("tooltip")
    );
    let tabs = part(
        "tabs",
        "t",
        &[
            ("labels", Value::Texts(vec!["One".into()])),
            ("selected", Value::Integer(1)),
        ],
    );
    assert!(
        decode(&window_of(vec![tabs]), 0)
            .unwrap_err()
            .contains("missing tab")
    );
    let nested = vec![
        part("table", "a", &[("columns", Value::Texts(vec!["a".into()]))]),
        part("table", "b", &[("columns", Value::Texts(vec!["b".into()]))]),
        part("end", "", &[]),
        part("end", "", &[]),
    ];
    assert!(
        decode(&window_of(nested), 0)
            .unwrap_err()
            .contains("no sections or tables")
    );
    let mut long = window_of(Vec::new());
    long[0].properties.push((
        "path".into(),
        Value::Texts(vec![
            "a".into(),
            "b".into(),
            "c".into(),
            "d".into(),
            "e".into(),
        ]),
    ));
    assert!(decode(&long, 0).unwrap_err().contains("path"));
}

#[test]
fn a_split_is_the_last_part_of_a_window_and_a_checkbox_has_two_words() {
    let split = || {
        part(
            "split",
            "s",
            &[("labels", Value::Texts(vec!["One".into(), "Two".into()]))],
        )
    };
    let end = || part("end", "", &[]);
    let text = || part("text", "", &[("text", Value::Text("a".into()))]);
    assert!(decode(&window_of(vec![split(), text(), end()]), 0).is_ok());
    for (parts, says) in [
        (vec![split(), end(), text()], "last part"),
        (
            vec![part("row", "", &[]), split(), end(), end()],
            "in no container",
        ),
        (vec![split(), split(), end(), end()], "in no container"),
        (
            vec![
                part(
                    "split",
                    "s",
                    &[
                        ("labels", Value::Texts(vec!["One".into()])),
                        ("selected", Value::Integer(1)),
                    ],
                ),
                end(),
            ],
            "missing section",
        ),
        (
            vec![
                part(
                    "split",
                    "s",
                    &[
                        ("labels", Value::Texts(vec!["One".into()])),
                        ("values", Value::Texts(vec!["1".into(), "2".into()])),
                    ],
                ),
                end(),
            ],
            "more values",
        ),
        (
            vec![
                part(
                    "split",
                    "s",
                    &[
                        ("labels", Value::Texts(vec!["One".into()])),
                        ("width", Value::Integer(40)),
                    ],
                ),
                end(),
            ],
            "not 96 to 320",
        ),
        (vec![part("split", "s", &[]), end()], "needs labels"),
        (
            vec![part(
                "checkbox",
                "c",
                &[
                    ("label", Value::Text("Names".into())),
                    ("words", Value::Texts(vec!["Yes".into()])),
                ],
            )],
            "2 words",
        ),
        (
            vec![part(
                "checkbox",
                "c",
                &[
                    ("label", Value::Text("Names".into())),
                    ("words", Value::Texts(vec!["Yes".into(), " ".into()])),
                ],
            )],
            "words of 1 to 32",
        ),
    ] {
        let error = decode(&window_of(parts), 0).unwrap_err();
        assert!(error.contains(says), "{says}: {error}");
    }
    let mut in_panel = window_of(vec![split(), end()]);
    in_panel[0].kind = "panel".into();
    in_panel[0].properties = vec![("name".into(), Value::Text("Groups".into()))];
    assert!(
        decode(&in_panel, 0)
            .unwrap_err()
            .contains("only a window holds a split")
    );
}

#[test]
fn a_popup_takes_no_input() {
    let popup = RawPlace {
        kind: "popup".into(),
        id: String::new(),
        properties: Vec::new(),
        parts: vec![part("button", "go", &[("label", Value::Text("Go".into()))])],
    };
    assert!(
        decode_popup(&popup, 0)
            .unwrap_err()
            .contains("takes no input")
    );
}

/// A seeded generator of damage to places as data.
struct Damage(u64);

impl Damage {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next() % bound.max(1) as u64).unwrap_or(0)
    }

    fn value(&mut self) -> Value {
        let n = [0.0, -1.0, 1e300, f64::NAN, f64::INFINITY, 2.0, 0.5][self.below(7)];
        let i = [0, -1, 1, 2, 7, 255, 256, i64::MAX, i64::MIN][self.below(9)];
        match self.below(6) {
            0 => Value::Text(
                ["", "x", "panel", "bottom", "end", &"y".repeat(5000)][self.below(6)].into(),
            ),
            1 => Value::Number(n),
            2 => Value::Integer(i),
            3 => Value::Flag(self.below(2) == 0),
            4 => Value::Texts(vec!["a".into(); self.below(40)]),
            _ => Value::Numbers(vec![n; self.below(300)]),
        }
    }

    fn damage(&mut self, places: &mut Vec<RawPlace>, kinds: &[String], names: &[String]) {
        let at = self.below(places.len());
        let Some(place) = places.get_mut(at) else {
            return;
        };
        match self.below(9) {
            0 => place.kind.clone_from(&kinds[self.below(kinds.len())]),
            1 if !place.parts.is_empty() => {
                let at = self.below(place.parts.len());
                place.parts.remove(at);
            }
            2 if !place.parts.is_empty() => {
                let at = self.below(place.parts.len());
                let copy = place.parts[at].clone();
                place.parts.insert(self.below(place.parts.len()), copy);
            }
            3 if !place.parts.is_empty() => {
                let at = self.below(place.parts.len());
                let kind = &kinds[self.below(kinds.len())];
                place.parts[at].kind.clone_from(kind);
            }
            4 if !place.parts.is_empty() => {
                let at = self.below(place.parts.len());
                let (name, value) = (names[self.below(names.len())].clone(), self.value());
                place.parts[at].properties.push((name, value));
            }
            5 if !place.parts.is_empty() => {
                let at = self.below(place.parts.len());
                let part = &mut place.parts[at];
                if !part.properties.is_empty() {
                    let which = self.below(part.properties.len());
                    part.properties[which].1 = self.value();
                }
            }
            6 if !place.parts.is_empty() => {
                let at = self.below(place.parts.len());
                place.parts[at].id = ["", "x", "main", &"z".repeat(200)][self.below(4)].into();
            }
            7 => {
                let (name, value) = (names[self.below(names.len())].clone(), self.value());
                place.properties.push((name, value));
            }
            _ => {
                let copy = place.clone();
                places.push(copy);
            }
        }
    }
}

#[test]
fn damaged_places_are_refused_without_panicking() {
    let original = encode_places(&[window(), panel()]);
    let mut kinds: Vec<String> = original
        .iter()
        .flat_map(|place| {
            std::iter::once(place.kind.clone()).chain(place.parts.iter().map(|p| p.kind.clone()))
        })
        .collect();
    kinds.extend(["popup", "gauge", ""].map(String::from));
    let mut names: Vec<String> = original
        .iter()
        .flat_map(|place| {
            place
                .properties
                .iter()
                .chain(place.parts.iter().flat_map(|p| &p.properties))
                .map(|(name, _)| name.clone())
        })
        .collect();
    names.extend(["at", "panel", "seconds", "in-row", "unknown"].map(String::from));

    let mut damage = Damage(0x5eed_a11c_e1ca);
    let mut refused = 0;
    for round in 0..20_000 {
        let mut places = original.clone();
        for _ in 0..=damage.below(4) {
            damage.damage(&mut places, &kinds, &names);
        }
        if decode(&places, u16::try_from(round % 3).unwrap()).is_err() {
            refused += 1;
        }
        let mut popup = places[0].clone();
        popup.kind = "popup".into();
        let _ = decode_popup(&popup, 0);
    }
    assert!(refused > 10_000, "only {refused} damaged sets refused");
}

#[test]
fn a_dialog_belongs_to_a_window_of_the_same_ui_and_takes_only_buttons() {
    let dialog = |window: &str, id: &str, parts: Vec<RawPart>| RawPlace {
        kind: "dialog".into(),
        id: id.into(),
        properties: vec![
            ("title".into(), Value::Text("Sure?".into())),
            ("window".into(), Value::Text(window.into())),
        ],
        parts,
    };
    let button = || part("button", "yes", &[("label", Value::Text("Yes".into()))]);
    let mut places = window_of(vec![]);
    places.push(dialog("main", "ask", vec![button()]));
    let decoded = decode(&places, 0).unwrap();
    let angelica_view::Place::Window(window) = &decoded[0] else {
        panic!("a window");
    };
    assert_eq!(window.dialog.as_ref().map(|d| d.id.as_str()), Some("ask"));

    for (places, says) in [
        (
            {
                let mut places = window_of(vec![]);
                places.push(dialog("other", "ask", vec![button()]));
                places
            },
            "no window \"other\"",
        ),
        (
            {
                let mut places = window_of(vec![]);
                places.push(dialog("main", "ask", vec![button()]));
                places.push(dialog("main", "again", vec![button()]));
                places
            },
            "more than one dialog",
        ),
        (
            {
                let mut places = window_of(vec![]);
                places.push(dialog("main", "ask", vec![part("text-field", "name", &[])]));
                places
            },
            "only buttons take input",
        ),
        (
            {
                let mut places = window_of(vec![]);
                places.push(dialog("main", "main", vec![button()]));
                places
            },
            "repeated",
        ),
    ] {
        let error = decode(&places, 0).unwrap_err();
        assert!(error.contains(says), "{error}");
    }
}
