//! Direct access to the game's objects through the SDK's views: a plugin
//! reads the player and the bags as the game map describes them, under the
//! domains the player approved; an element of a domain it does not hold
//! fails alone; a handle lives for its frame unless it is held, and a held
//! one fails once its object is gone; a central field is written only
//! where the Lens catalog offers a change, and is put back when the plugin
//! stops.
use angelica_fake_game::ui::WidgetRef;
use angelica_fake_game::{TestHost, build_component};
use runtime::{
    host::{Action, Event, Status},
    sandbox::Limits,
};
use std::sync::OnceLock;

fn component() -> &'static [u8] {
    static COMPONENT: OnceLock<Vec<u8>> = OnceLock::new();
    COMPONENT.get_or_init(|| build_component("angelica-memory"))
}

/// The player, the bags, and the camera as the game map describes them,
/// with constant offsets of the scripted build.
const MAP: [(&str, &str); 11] = [
    (
        "entries/character.toml",
        r#"
[global."character.local_player"]
locator = { signature = "C3" }
type = "ptr"


[field."character.character.data"]
of = "character.character"
type = "struct"
offset = 0x1A0


[field."character.character_data.level"]
of = "character.character_data"
type = "u8"
offset = 0x2B

"#,
    ),
    (
        "entries/object.toml",
        r#"
[field."object.game_object.name"]
of = "object.game_object"
type = "struct"
offset = 0x30

"#,
    ),
    (
        "entries/inventory.toml",
        r#"
[global."inventory.inventory_manager.instance"]
locator = { signature = "C3" }
type = "struct"


[field."inventory.inventory_manager.inventories"]
of = "inventory.inventory_manager"
type = "ptr"
offset = 0x1E08


[field."inventory.inventory_container.items"]
of = "inventory.inventory_container"
type = "ptr"
offset = 0x08


[field."inventory.inventory_container.kind"]
of = "inventory.inventory_container"
type = "u32"
offset = 0x10


[field."inventory.inventory_container.size"]
of = "inventory.inventory_container"
type = "u32"
offset = 0x14


[field."inventory.inventory_item.item_id"]
of = "inventory.inventory_item"
type = "u32"
offset = 0x10


[field."inventory.inventory_item.quantity"]
of = "inventory.inventory_item"
type = "u32"
offset = 0x14

"#,
    ),
    (
        "entries/camera.toml",
        r#"
[global."camera.control.instance"]
locator = { signature = "C3" }
type = "struct"


[field."camera.camera_manager.camera"]
of = "camera.camera_manager"
type = "ptr"
offset = 0x0


[field."camera.camera.max_distance"]
of = "camera.camera"
type = "f32"
offset = 0x12C
"#,
    ),
    (
        "types/object.toml",
        r#"
[types."object.game_object"]
doc = "An object."
domain = "character"


[types."object.game_object".fields.name]
doc = "Its name."
text = 64

"#,
    ),
    (
        "types/character.toml",
        r#"
[types."character.character"]
doc = "A character."
domain = "character"
extends = "object.game_object"


[types."character.character".fields.data]
doc = "Numbers."
embed = "character.character_data"


[types."character.character_data"]
doc = "Numbers."
domain = "character"


[types."character.character_data".fields.level]
doc = "Level."


[types."character.battle_chara"]
doc = "A battle character."
domain = "character"
extends = "character.character"

"#,
    ),
    (
        "types/inventory.toml",
        r#"
[types."inventory.inventory_manager"]
doc = "Every container."
domain = "inventory"


[types."inventory.inventory_manager".fields.containers]
doc = "The containers."
at = "inventory.inventory_manager.inventories"
array = { of = "inventory.inventory_container", length = 84, stride = 32, partition = { field = "kind", domains = { armoury = ["armoury_head"] } } }


[types."inventory.inventory_container"]
doc = "One container."
domain = "inventory"


[types."inventory.inventory_container".fields.kind]
doc = "Which container this is."
enum = "inventory.container_kind"


[types."inventory.inventory_container".fields.size]
doc = "How many slots it has."


[types."inventory.inventory_container".fields.items]
doc = "The slots."
array = { of = "inventory.inventory_item", count = "size", most = 200, stride = 72 }


[types."inventory.inventory_item"]
doc = "One slot."
domain = "inventory"


[types."inventory.inventory_item".fields.item]
doc = "The item."
at = "inventory.inventory_item.item_id"


[types."inventory.inventory_item".fields.quantity]
doc = "How many."

"#,
    ),
    (
        "types/camera.toml",
        r#"
[types."camera.camera_manager"]
doc = "The cameras."
domain = "camera"


[types."camera.camera_manager".fields.camera]
doc = "The camera."
ref = "camera.camera"


[types."camera.camera"]
doc = "A camera."
domain = "camera"


[types."camera.camera".fields.max_distance]
doc = "The zoom limit."
set = { range = [1.5, 100.0] }
"#,
    ),
    (
        "enums/inventory.container_kind.toml",
        "doc = \"Containers.\"\nvalues = { bag_1 = 0, bag_2 = 1, armoury_head = 3201 }\n",
    ),
    (
        "domains.toml",
        r#"
[domains.character]
doc = "The player's character."
class = "you"
read = { en = "Sees your character", ru = "Видит вашего персонажа" }

[domains.inventory]
doc = "The items the player carries."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }

[domains.armoury]
doc = "The armoury chest."
class = "you"
read = { en = "Sees your armoury chest", ru = "Видит ваш оружейный сундук" }

[domains.camera]
doc = "The camera."
class = "display"
read = { en = "Sees your camera", ru = "Видит вашу камеру" }
change = { en = "Changes your camera's zoom limits", ru = "Меняет пределы приближения камеры" }
"#,
    ),
    (
        "roots.toml",
        r#"
[roots.player]
doc = "The player's character."
global = "character.local_player"
type = "character.battle_chara"
whose = "you"

[roots.inventory]
doc = "The player's items."
global = "inventory.inventory_manager.instance"
type = "inventory.inventory_manager"
whose = "you"

[roots.camera]
doc = "The cameras."
global = "camera.control.instance"
type = "camera.camera_manager"
whose = "you"
"#,
    ),
];

const MANIFEST: &str = r#"manifest = 1

[plugin]
id = "memory"
name = "Memory"
version = "0.1.0"
api = "1.0"
description = "Reads the game's objects through views."

[lens]
read = ["character", "inventory", "camera"]
change = ["camera"]
"#;

fn started() -> TestHost {
    let mut test = TestHost::new(Limits::default());
    test.use_map(&MAP, &[]);
    test.install(MANIFEST, component()).unwrap();
    test.host.queue(Action::Approve("memory".into()));
    test.frames(1);
    assert_eq!(
        test.host.plugin("memory").unwrap().status(),
        Status::Running
    );
    test
}

/// Clicks `id` and gives the line the plugin logged for it.
fn click(test: &TestHost, id: &str) -> String {
    test.event(
        "memory",
        Event::Clicked(WidgetRef {
            window: "main".into(),
            id: id.into(),
        }),
    );
    let prefix = format!("{}:", id.split(':').next().unwrap_or(id));
    test.host
        .plugin("memory")
        .unwrap()
        .log
        .lines()
        .into_iter()
        .map(|line| line.text)
        .rfind(|line| line.starts_with(&prefix))
        .unwrap_or_else(|| panic!("the plugin logged nothing for {id}"))
}

#[test]
fn a_plugin_reads_the_player_through_views() {
    let test = started();
    test.game.set(
        "player",
        &[("name", "Alisaie".into()), ("data.level", 90.into())],
    );
    assert_eq!(click(&test, "player"), "player: Alisaie level 90");
    assert!(
        click(&test, "batch").contains("Unsigned(90)"),
        "a batch reads the same values in one call"
    );
}

#[test]
fn before_the_player_logs_in_there_is_no_player() {
    let test = started();
    assert_eq!(click(&test, "player"), "player: none");
}

#[test]
fn a_plugin_reads_the_bags_and_only_the_domains_it_holds() {
    let test = started();
    test.game.set(
        "inventory.containers[kind = bag_1].items[0]",
        &[("item", 5057.into()), ("quantity", 12.into())],
    );
    test.game.set(
        "inventory.containers[kind = bag_2].items[1]",
        &[("item", 5111.into()), ("quantity", 2.into())],
    );
    test.game.set(
        "inventory.containers[kind = armoury_head].items[0]",
        &[("item", 5111.into()), ("quantity", 1.into())],
    );
    let bags = click(&test, "bags");
    assert!(bags.contains("kind 0 5057x12"), "{bags}");
    assert!(bags.contains("kind 1 5111x2"), "{bags}");
    assert!(
        bags.contains("2: PermissionDenied"),
        "the armoury chest is another domain: {bags}"
    );
    assert!(!bags.contains("5111x1"), "{bags}");
}

#[test]
fn a_frame_handle_ends_with_its_frame_and_a_held_one_lives_on() {
    let test = started();
    test.game.set("player", &[("data.level", 50.into())]);
    test.game
        .set("inventory.containers[kind = bag_1]", &[("size", 35.into())]);
    assert_eq!(click(&test, "keep"), "keep: kept");
    assert_eq!(click(&test, "hold"), "hold: held");
    test.frames(1);
    assert_eq!(click(&test, "stale"), "stale: InvalidArgument");
    assert_eq!(click(&test, "held"), "held: size 35");
    test.game
        .set("inventory.containers[kind = bag_1]", &[("size", 40.into())]);
    test.frames(1);
    assert_eq!(click(&test, "held"), "held: size 40");
    assert_eq!(click(&test, "release"), "release: released");
    assert_eq!(click(&test, "held"), "held: nothing");
}

#[test]
fn a_held_handle_fails_once_its_path_reaches_another_object() {
    let test = started();
    test.game
        .set("inventory.containers[kind = bag_1]", &[("size", 35.into())]);
    assert_eq!(click(&test, "hold"), "hold: held");
    // The first container is another kind now: still reachable, but the
    // plugin may not read the armoury chest.
    test.game
        .set("inventory.containers[0]", &[("kind", 3201.into())]);
    test.frames(1);
    assert_eq!(click(&test, "held"), "held: Unavailable");
}

#[test]
fn a_central_field_is_written_only_where_a_change_is_offered() {
    let test = started();
    test.game.set("player", &[("data.level", 50.into())]);
    test.game.set(
        "inventory.containers[kind = bag_1].items[0]",
        &[("item", 5057.into()), ("quantity", 12.into())],
    );
    test.game
        .set("camera.camera", &[("max_distance", 20.0.into())]);
    assert_eq!(click(&test, "level:99"), "level: PermissionDenied");
    assert_eq!(click(&test, "quantity:999"), "quantity: PermissionDenied");
    assert_eq!(
        click(&test, "zoom:500"),
        "zoom: InvalidArgument",
        "past the range"
    );
    assert_eq!(click(&test, "zoom:40"), "zoom: written 40");
    assert_eq!(click(&test, "player"), "player:  level 50");
}

#[test]
fn a_written_central_value_is_put_back_when_the_plugin_stops() {
    let test = started();
    test.game
        .set("camera.camera", &[("max_distance", 20.0.into())]);
    assert_eq!(click(&test, "zoom:40"), "zoom: written 40");
    test.host.queue(Action::Disable("memory".into()));
    test.frames(1);
    test.host.queue(Action::Enable("memory".into()));
    test.frames(1);
    assert_eq!(click(&test, "zoomed"), "zoomed: 20");
}

#[test]
fn without_a_domain_the_plugin_reads_nothing_of_it() {
    let mut test = TestHost::new(Limits::default());
    test.use_map(&MAP, &[]);
    let manifest = MANIFEST.replace("\"character\", ", "");
    test.install(&manifest, component()).unwrap();
    test.host.queue(Action::Approve("memory".into()));
    test.frames(1);
    test.game.set("player", &[("data.level", 50.into())]);
    assert_eq!(click(&test, "player"), "player: PermissionDenied");
}
