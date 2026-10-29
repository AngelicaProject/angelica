//! Draws on a surface of its own with the interface kit: a window with the
//! player's name, level, and position, in Angelica's look, and a mark over
//! the player's character in the world, which follows the camera. The
//! window takes the mouse; the mark lets it through to the game.

use angelica_kit::{Kit, egui};
use angelica_sdk::game::character;
use angelica_sdk::gpu::{self, SurfaceEvent};

struct Surface {
    kit: Kit,
    /// The player's name and level, and where the character is.
    seen: Option<(String, u8, [f32; 3])>,
    clicks: u32,
}

/// What the plugin reads of the player this frame; none before the player
/// logs in.
fn player() -> Result<Option<(String, u8, [f32; 3])>, angelica_sdk::Error> {
    let Some(player) = character::local_player()? else {
        return Ok(None);
    };
    let name = String::from_utf8_lossy(&player.name()?).into_owned();
    let level = player.data()?.level()?;
    let at = [
        player.position_x()?,
        player.position_y()?,
        player.position_z()?,
    ];
    Ok(Some((name, level, at)))
}

impl angelica_sdk::Plugin for Surface {
    fn start() -> Result<Self, String> {
        Ok(Self {
            kit: Kit::new()?,
            seen: None,
            clicks: 0,
        })
    }

    fn surface_input(&mut self, events: Vec<SurfaceEvent>) {
        self.kit.input(&events);
    }

    fn tick(&mut self, _elapsed: f64) -> Result<(), String> {
        self.seen = player().unwrap_or(None);
        let seen = self.seen.clone();
        let clicks = &mut self.clicks;
        self.kit.frame(|context| {
            egui::Window::new("Where am I").show(context, |ui| match &seen {
                Some((name, level, [x, y, z])) => {
                    ui.label(format!("{name}, level {level}"));
                    ui.label(format!("{x:.1}, {y:.1}, {z:.1}"));
                    if ui.button("Mark").clicked() {
                        *clicks += 1;
                    }
                    ui.label(format!("Marked {clicks} times"));
                }
                None => {
                    ui.label("Not logged in");
                }
            });
            if let Some((_, _, at)) = &seen
                && let Some([x, y]) = gpu::project(*at)
            {
                let scale = context.pixels_per_point();
                let center = egui::pos2(x / scale, y / scale);
                let painter = context.layer_painter(egui::LayerId::background());
                painter.circle_stroke(
                    center,
                    12.0,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(220, 60, 70)),
                );
            }
        })?;
        Ok(())
    }
}

angelica_sdk::plugin!(Surface);
