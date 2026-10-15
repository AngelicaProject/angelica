//! `angelica schema`, `angelica query`, and `angelica look` over a small
//! map and executable.
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use angelica_pe::testing::{CODE, PeBuilder};
use serde_json::{Value, json};

const ENTRIES: &str = r#"
[global."inventory.manager.instance"]
locator = { signature = "90 C3" }
type = "struct"

[function."inventory.container.get_size"]
locator = { signature = "48 89 5C 24 08" }
abi = { args = ["ptr"], ret = "u32" }
thread = "main"

[field."inventory.manager.containers"]
of = "inventory.manager"
type = "ptr"
offset = 0x1E08

[field."inventory.container.kind"]
of = "inventory.container"
type = "u32"
offset = 0x08

[field."inventory.container.size"]
of = "inventory.container"
type = "u32"
locator = { in = "inventory.container.get_size", signature = "8B 41 ??", operand = "disp" }
"#;

const TYPES: &str = r#"
[types."inventory.manager"]
doc = "Every container of the player's items."
domain = "inventory"

[types."inventory.manager".fields.containers]
doc = "The containers."
array = { of = "inventory.container", length = 4, stride = 24 }

[types."inventory.container"]
doc = "One container: a bag, the crystals, the currency."
domain = "inventory"

[types."inventory.container".fields.kind]
doc = "Which container this is."
enum = "inventory.container_kind"

[types."inventory.container".fields.size]
doc = "How many slots it has."
"#;

const DOMAINS: &str = r#"
[domains.inventory]
doc = "The items the player carries."
class = "you"
read = { en = "Sees the items you carry", ru = "Видит предметы, которые вы носите" }
"#;

const ROOTS: &str = r#"
[roots.inventory]
doc = "The player's items."
global = "inventory.manager.instance"
type = "inventory.manager"
"#;

/// A map and a game folder; the size's function is not in the executable.
fn fixture(root: &Path) -> (PathBuf, PathBuf) {
    let map = root.join("карта игры");
    let game = root.join("игра").join("game");
    for folder in ["entries", "types", "enums"] {
        std::fs::create_dir_all(map.join(folder)).unwrap();
    }
    std::fs::create_dir_all(&game).unwrap();
    std::fs::write(map.join("map.toml"), "format = 2\n").unwrap();
    std::fs::write(map.join("entries/inventory.toml"), ENTRIES).unwrap();
    std::fs::write(map.join("types/inventory.toml"), TYPES).unwrap();
    std::fs::write(map.join("domains.toml"), DOMAINS).unwrap();
    std::fs::write(map.join("roots.toml"), ROOTS).unwrap();
    std::fs::write(
        map.join("enums/inventory.container_kind.toml"),
        "doc = \"The kinds of container.\"\nvalues = { bag_1 = 0, bag_2 = 1, crystals = 2001 }\n",
    )
    .unwrap();
    let executable = PeBuilder::new()
        .section(".text", CODE, &[0x90, 0xC3])
        .build_file();
    std::fs::write(game.join("ffxiv_dx11.exe"), executable).unwrap();
    std::fs::write(game.join("ffxivgame.ver"), "2026.09.15.0000.0000").unwrap();
    (map, game)
}

fn run(arguments: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_angelica"))
        .args(arguments)
        .output()
        .unwrap()
}

fn json_of(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout)
        .unwrap_or_else(|_| panic!("{}", String::from_utf8_lossy(&output.stdout)))
}

#[test]
fn schema_search_and_show_say_what_the_map_holds_and_what_the_build_lacks() {
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(directory.path());
    let output = run(&[
        "schema".as_ref(),
        "search".as_ref(),
        "CONTAINER".as_ref(),
        "kind".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--json".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let found = json_of(&output);
    let names: Vec<&str> = found["matches"]
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        ["inventory.container.kind", "inventory.container_kind"]
    );
    assert_eq!(found["matches"][0]["available"], Value::Null, "no build");

    let output = run(&[
        "schema".as_ref(),
        "show".as_ref(),
        "inventory".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--game".as_ref(),
        game.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(
        text.contains("root inventory at inventory.manager.instance"),
        "{text}"
    );
    assert!(
        text.contains("inventory.manager.containers: array of 4 inventory.container, behind a pointer [inventory] The containers."),
        "{text}"
    );

    let output = run(&[
        "schema".as_ref(),
        "show".as_ref(),
        "inventory.container".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
        "--game".as_ref(),
        game.as_os_str(),
        "--json".as_ref(),
    ]);
    let shown = json_of(&output);
    let size = shown["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["name"] == "inventory.container.size")
        .unwrap();
    assert_eq!(size["available"], false);
    assert!(
        size["reason"]
            .as_str()
            .unwrap()
            .contains("inventory.container.get_size"),
        "{size}"
    );

    let output = run(&[
        "schema".as_ref(),
        "show".as_ref(),
        "party".as_ref(),
        "--map".as_ref(),
        map.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn query_checks_names_domains_and_the_build_without_the_game() {
    let directory = tempfile::tempdir().unwrap();
    let (map, game) = fixture(directory.path());
    let query = |text: &str, extra: &[&std::ffi::OsStr]| {
        let mut arguments: Vec<&std::ffi::OsStr> = vec![
            "query".as_ref(),
            text.as_ref(),
            "--map".as_ref(),
            map.as_os_str(),
            "--json".as_ref(),
        ];
        arguments.extend_from_slice(extra);
        run(&arguments)
    };
    let output = query("inventory.containers[kind = bag_1] { kind }", &[]);
    assert_eq!(output.status.code(), Some(0));
    let checked = json_of(&output);
    assert_eq!(checked["domains"], json!(["inventory"]));
    assert_eq!(checked["columns"][0]["field"], "inventory.container.kind");
    assert_eq!(checked["columns"][0]["meaning"], "a named value");
    assert_eq!(checked["checked_build"], false);

    let output = query(
        "inventory.containers { kind, size }",
        &["--game".as_ref(), game.as_os_str()],
    );
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(json_of(&output)["error"]["code"], "Unavailable");

    let output = query("inventory.containers[kind = bag_9] { kind }", &[]);
    assert_eq!(json_of(&output)["error"]["code"], "InvalidArgument");

    let plugin = directory.path().join("plugin");
    std::fs::create_dir_all(&plugin).unwrap();
    std::fs::write(
        plugin.join("angelica.toml"),
        "manifest = 1\n[plugin]\nid = \"bags\"\nname = \"Bags\"\nversion = \"0.1.0\"\napi = \"1.0\"\ndescription = \"Bags.\"\n",
    )
    .unwrap();
    let output = query(
        "inventory.containers { kind }",
        &["--plugin".as_ref(), plugin.as_os_str()],
    );
    assert_eq!(json_of(&output)["error"]["code"], "PermissionDenied");
}

#[test]
fn look_writes_a_request_and_prints_the_games_answer() {
    use runtime::look::{REQUEST, RESPONSE, Request, Response};
    let directory = tempfile::tempdir().unwrap();
    let installation = directory.path().join("установка");
    let folder = installation.join("look");
    // The game's side: answers the first request.
    let answering = folder.clone();
    let game = std::thread::spawn(move || {
        let path = answering.join(REQUEST);
        for _ in 0..400 {
            if let Ok(bytes) = std::fs::read(&path)
                && let Ok(request) = serde_json::from_slice::<Request>(&bytes)
            {
                std::fs::remove_file(&path).unwrap();
                let response = Response {
                    format: 1,
                    id: request.id,
                    columns: Some(vec![runtime::look::Column {
                        name: "kind".into(),
                        field: "inventory.container.kind".into(),
                        doc: "Which.".into(),
                        domain: "inventory".into(),
                    }]),
                    rows: Some(vec![runtime::look::Row {
                        indexes: vec![2],
                        values: vec![json!("crystals")],
                    }]),
                    error: None,
                    ..Response::default()
                };
                runtime::look::write_atomically(
                    &answering.join(RESPONSE),
                    &serde_json::to_vec(&response).unwrap(),
                )
                .unwrap();
                return request.query;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        String::new()
    });
    let output = run(&[
        "look".as_ref(),
        "inventory.containers[2] { kind }".as_ref(),
        "--installation".as_ref(),
        installation.as_os_str(),
    ]);
    assert_eq!(game.join().unwrap(), "inventory.containers[2] { kind }");
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[2] kind=\"crystals\"\n1 rows\n"
    );

    let output = run(&[
        "look".as_ref(),
        "inventory { x }".as_ref(),
        "--installation".as_ref(),
        installation.as_os_str(),
        "--timeout".as_ref(),
        "0".as_ref(),
    ]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("development build of Angelica"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn compare_names_the_bytes_that_changed_between_two_dumps() {
    let directory = tempfile::tempdir().unwrap();
    let dump = |name: &str, hex: &str| {
        let path = directory.path().join(name);
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"format": 1, "id": "1", "objects": [
                {"indexes": [0, 3], "address": 4096, "bytes": hex},
                {"indexes": [1], "address": 8192, "bytes": null},
            ]}))
            .unwrap(),
        )
        .unwrap();
        path
    };
    let before = dump("before.json", "0c000000ff01");
    let after = dump("after.json", "0d000000ff02");
    let output = run(&[
        "look".as_ref(),
        "--compare".as_ref(),
        before.as_os_str(),
        after.as_os_str(),
    ]);
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "[0,3] +0x0000: 0c -> 0d
[0,3] +0x0005: 01 -> 02
"
    );
}
