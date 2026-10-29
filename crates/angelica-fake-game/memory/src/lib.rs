//! A test plugin that reads and writes the scripted game's objects through
//! the SDK's views of the game map. The host's tests click a button and the
//! log tells what the plugin found:
//!
//! - `player`: the player's name and level;
//! - `bags`: every container the plugin may read, with its items, and why
//!   it may not read the others;
//! - `hold` and `held`: keeps the first container across frames, then reads
//!   its size through the held view;
//! - `keep` and `stale`: keeps a handle of this frame, then uses it in a
//!   later one;
//! - `quantity:<n>`: writes the first item's quantity;
//! - `level:<n>`: writes the player's level, which no change offers;
//! - `zoom:<n>`: writes the camera's zoom limit, which a change offers;
//! - `zoomed`: reads the camera's zoom limit;
//! - `batch`: reads the player's level and name in one call.
use angelica_sdk::{
    Error, Plugin,
    game::{camera, character, inventory},
    memory::{self, Field, Held},
    ui::{Event, Window},
};

#[derive(Default)]
struct Memory {
    held: Option<Held<inventory::InventoryContainer>>,
    kept: Option<character::BattleChara>,
}

fn kind(error: &Error) -> String {
    let kind = format!("{:?}", error.kind);
    kind.trim_start_matches("ErrorKind::").to_owned()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn player() -> Result<String, Error> {
    let Some(player) = character::local_player()? else {
        return Ok("none".into());
    };
    let level = player.data()?.level()?;
    let name = player.name()?;
    Ok(format!("{} level {level}", text(&name)))
}

fn bags() -> Result<Vec<String>, Error> {
    let manager = inventory::inventory_manager_instance()?;
    let Some(containers) = manager.containers()? else {
        return Ok(vec!["no containers".into()]);
    };
    let mut lines = Vec::new();
    for (index, container) in containers.iter().enumerate() {
        let container = match container {
            Ok(container) => container,
            Err(error) => {
                lines.push(format!("{index}: {}", kind(&error)));
                continue;
            }
        };
        let Some(items) = container.items()? else {
            continue;
        };
        let mut found = Vec::new();
        for item in items.iter() {
            let item = item?;
            let id = item.item()?;
            if id != 0 {
                found.push(format!("{id}x{}", item.quantity()?));
            }
        }
        if !found.is_empty() {
            lines.push(format!(
                "{index}: kind {} {}",
                container.kind()?,
                found.join(" ")
            ));
        }
    }
    Ok(lines)
}

fn first_container() -> Result<inventory::InventoryContainer, Error> {
    let manager = inventory::inventory_manager_instance()?;
    let containers = manager
        .containers()?
        .ok_or_else(|| missing("no containers"))?;
    containers.get(0)
}

fn missing(message: &str) -> Error {
    Error {
        kind: angelica_sdk::ErrorKind::NotFound,
        message: message.into(),
    }
}

fn batch() -> Result<String, Error> {
    static LEVEL: Field = Field::new("character.character_data.level");
    static NAME: Field = Field::new("object.game_object.name");
    let player = character::local_player()?.ok_or_else(|| missing("no player"))?;
    let data = player.data()?;
    let values = memory::read_batch(&[
        (memory::View::handle(&data), &LEVEL),
        (memory::View::handle(&player), &NAME),
    ])?;
    Ok(format!("{values:?}"))
}

/// Writes the camera's zoom limit, which the Lens catalog offers as a
/// change, and reads it back.
fn zoom(limit: f32) -> Result<String, Error> {
    let camera = camera::control_instance()?
        .camera()?
        .ok_or_else(|| missing("no camera"))?;
    camera.set_max_distance(limit)?;
    Ok(format!("written {}", camera.max_distance()?))
}
impl Memory {
    fn clicked(&mut self, id: &str) -> String {
        match id {
            "player" => format!("player: {}", player().unwrap_or_else(|error| kind(&error))),
            "bags" => match bags() {
                Ok(lines) => format!("bags: {}", lines.join("; ")),
                Err(error) => format!("bags: {} {}", kind(&error), error.message),
            },
            "hold" => match first_container().and_then(|container| memory::hold(&container)) {
                Ok(held) => {
                    self.held = Some(held);
                    "hold: held".into()
                }
                Err(error) => format!("hold: {}", kind(&error)),
            },
            "held" => match &self.held {
                Some(held) => match held.get().size() {
                    Ok(size) => format!("held: size {size}"),
                    Err(error) => format!("held: {}", kind(&error)),
                },
                None => "held: nothing".into(),
            },
            "release" => {
                self.held = None;
                "release: released".into()
            }
            "keep" => match character::local_player() {
                Ok(player) => {
                    self.kept = player;
                    "keep: kept".into()
                }
                Err(error) => format!("keep: {}", kind(&error)),
            },
            "stale" => match self.kept.map(|player| player.data()) {
                Some(Ok(_)) => "stale: read".into(),
                Some(Err(error)) => format!("stale: {}", kind(&error)),
                None => "stale: nothing".into(),
            },
            "batch" => format!("batch: {}", batch().unwrap_or_else(|error| kind(&error))),
            "zoomed" => {
                let limit = camera::control_instance()
                    .and_then(|manager| manager.camera()?.ok_or_else(|| missing("no camera")))
                    .and_then(|camera| camera.max_distance());
                format!(
                    "zoomed: {}",
                    limit.map_or_else(|error| kind(&error), |limit| limit.to_string())
                )
            }
            _ => {
                if let Some(number) = id.strip_prefix("quantity:") {
                    let result = first_container()
                        .and_then(|container| container.items()?.ok_or_else(|| missing("no items")))
                        .and_then(|items| items.get(0))
                        .and_then(|item| item.set_quantity(number.parse().unwrap_or(0)));
                    return format!(
                        "quantity: {}",
                        result.map_or_else(|error| kind(&error), |()| "written".into())
                    );
                }
                if let Some(number) = id.strip_prefix("zoom:") {
                    return format!(
                        "zoom: {}",
                        zoom(number.parse().unwrap_or(0.0)).unwrap_or_else(|error| kind(&error))
                    );
                }
                if let Some(number) = id.strip_prefix("level:") {
                    let result = character::local_player()
                        .and_then(|player| player.ok_or_else(|| missing("no player")))
                        .and_then(|player| player.data())
                        .and_then(|data| data.set_level(number.parse().unwrap_or(0)));
                    return format!(
                        "level: {}",
                        result.map_or_else(|error| kind(&error), |()| "written".into())
                    );
                }
                format!("{id}: unknown")
            }
        }
    }
}

impl Plugin for Memory {
    fn start() -> Result<Self, String> {
        Ok(Self::default())
    }

    fn ui(&mut self) -> Vec<Window> {
        vec![Window::new("main", "Memory")]
    }

    fn ui_event(&mut self, event: Event) {
        if let Event::Clicked(target) = event {
            angelica_sdk::info(&self.clicked(&target.id));
        }
    }
}

angelica_sdk::plugin!(Memory);
