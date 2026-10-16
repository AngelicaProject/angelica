use angelica_view::data::{encode_places, kinds};
use angelica_view::examples::{Shows, examples};
use angelica_view::{Place, Window};
use std::collections::BTreeSet;

#[test]
fn every_kind_has_an_example_with_its_code() {
    let examples = examples(0);
    let shown: BTreeSet<&str> = examples.iter().map(|example| example.kind).collect();
    for (kind, _) in kinds() {
        // `end` closes a container; every container's example shows it.
        if kind != "end" {
            assert!(shown.contains(kind), "no example of {kind}");
        }
    }
    let mut names = BTreeSet::new();
    for example in &examples {
        assert!(names.insert(example.name), "{} twice", example.name);
        assert!(!example.code.is_empty(), "{} has no code", example.name);
        assert!(
            !example.code.contains("// ["),
            "{}: {}",
            example.name,
            example.code
        );
    }
}

#[test]
fn every_example_passes_the_hosts_checks() {
    for example in examples(0) {
        let place = match example.shows {
            Shows::Parts(widgets) => {
                let mut window = Window::new("example", "Example");
                window.widgets = widgets;
                Place::Window(window)
            }
            Shows::Window(window) => Place::Window(window),
            Shows::Panel(panel) => Place::Panel(panel),
            Shows::Popup(popup) => {
                let raw = angelica_view::data::encode_popup(&popup);
                assert!(
                    angelica_view::data::decode_popup(&raw, 0).is_ok(),
                    "{}",
                    example.name
                );
                continue;
            }
        };
        let raw = encode_places(std::slice::from_ref(&place));
        assert_eq!(
            angelica_view::data::decode(&raw, 0),
            Ok(vec![place]),
            "{}",
            example.name
        );
    }
}

#[test]
fn the_code_shown_is_balanced() {
    for example in examples(0) {
        let open = example.code.matches(['(', '[', '{']).count();
        let close = example.code.matches([')', ']', '}']).count();
        assert_eq!(open, close, "{}:\n{}", example.name, example.code);
    }
}
