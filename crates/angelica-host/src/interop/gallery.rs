//! The documentation's gallery: every example of
//! `angelica_view::examples`, drawn by the host's own renderer over a
//! stand-in for the game, in the light and the dark theme, as PNG files with
//! `gallery.json` beside them, which the website's build turns into a page
//! (`docs/development/website.md`). Runs on Windows:
//!
//! ```text
//! GALLERY_OUT=site/gallery cargo test -p angelica-host --lib gallery -- --ignored
//! ```
use super::painter::Painter;
use crate::{
    host::{Directories, Host},
    language::Language,
    menu::Look,
    panel::Icons,
    places,
    sandbox::{Limits, Sandbox, Services},
    theme::{self, Theme},
};
use angelica_view::examples::{Example, examples};
use std::{
    collections::HashMap,
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};
use windows::Win32::Graphics::{
    Direct3D::D3D_DRIVER_TYPE_WARP,
    Dxgi::{Common::DXGI_FORMAT_R8G8B8A8_UNORM, DXGI_SWAP_CHAIN_FLAG, IDXGISwapChain},
};

/// The size of the frame each example is drawn in.
const WIDTH: u16 = 1000;
const HEIGHT: u16 = 700;
/// Where an example is drawn, and the room around it kept in its picture
/// for its shadow.
const AT: f32 = 48.0;
const MARGIN: f32 = 28.0;

struct NoMemory;

impl crate::lens::Memory for NoMemory {
    fn read(&self, _: u64, _: &mut [u8]) -> bool {
        false
    }
}

/// No game: the gallery draws Angelica's interface alone.
struct NoGame;

impl crate::game::Game for NoGame {
    fn version(&self) -> String {
        "2026.10.06.0000.0000".into()
    }
    fn enable_excel_text(&self) -> Result<(), String> {
        Ok(())
    }
    fn enable_text_case(&self) -> Result<(), String> {
        Ok(())
    }
    fn enable_file_replacements(&self) -> Result<(), String> {
        Ok(())
    }
    fn enable_commands(&self) -> Result<(), String> {
        Ok(())
    }

    fn enable_menus(&self) -> Result<(), String> {
        Ok(())
    }
    fn print(&self, _: &[u8]) -> Result<(), String> {
        Ok(())
    }
    fn query(&self, _: &crate::lens::Prepared) -> crate::error::Result<Vec<crate::lens::Row>> {
        Err(crate::error::Error::new(
            crate::error::Kind::Unavailable,
            "no game",
        ))
    }
    fn memory(&self) -> &dyn crate::lens::Memory {
        &NoMemory
    }
    fn enable_watch(&self, _: usize, _: &str, _: u64) -> Result<(), String> {
        Ok(())
    }
}

/// A host without plugins in `root`.
fn host(root: &Path) -> Host {
    let services = Arc::new(Services {
        intercepts: Arc::default(),
        gpu: Arc::default(),
        memory: Arc::default(),
        data: Arc::new(crate::game_data::GameData::new(Some(
            &root.join("game-files"),
        ))),
        files: Arc::new(crate::file_replacements::FileReplacements::new(
            &root.join("cache"),
        )),
        excel: Arc::new(crate::excel_text::ExcelText::default()),
        case: Arc::new(crate::text_case::TextCase::default()),
        commands: Arc::new(crate::commands::Commands::default()),
        chat: Arc::new(crate::chat::Chat::default()),
        sheets: Arc::new(crate::sheets::Sheets::default()),
        lens: Arc::new(crate::lens::Lens::default()),
        draw: Arc::new(crate::draw::Drawings::default()),
        messages: Arc::new(crate::messages::Messages::default()),
        clipboard: Arc::new(crate::clipboard::Clipboard::default()),
        links: Arc::new(crate::links::Links::default()),
        menus: Arc::new(crate::menus::Menus::default()),
        notifications: Arc::new(crate::notifications::Notifications::default()),
        sounds: Arc::new(crate::sound::Sounds::default()),
        sockets: Arc::new(crate::sockets::Sockets::dialing(Arc::new(
            |_: &crate::http::Client, _: &str| {
                Err(crate::error::Error::new(
                    crate::error::Kind::Network,
                    "offline",
                ))
            },
        ))),
        game: Arc::new(NoGame),
        starting: AtomicBool::new(false),
        russian: AtomicBool::new(false),
    });
    Host::new(
        Arc::new(Sandbox::new().expect("a sandbox")),
        Limits::default(),
        services,
        Directories {
            plugins: root.join("plugins"),
            grants: root.join("grants"),
            state: root.join("state"),
            storage: root.join("storage"),
            cache: root.join("cache").join("plugins"),
        },
        Arc::new(|_: &str| {}),
        Box::new(|_: &Path| {}),
    )
}

/// A dusk landscape standing in for the game behind the interface.
fn scenery(context: &egui::Context) {
    let painter = context.layer_painter(egui::LayerId::background());
    let screen = context.content_rect();
    let mut mesh = egui::Mesh::default();
    let stops = [
        (0.0, egui::Color32::from_rgb(0x8f, 0xa6, 0xc0)),
        (0.55, egui::Color32::from_rgb(0xc9, 0xb9, 0xa6)),
        (0.70, egui::Color32::from_rgb(0xd8, 0xc3, 0xa5)),
        (0.71, egui::Color32::from_rgb(0x9a, 0xa0, 0x80)),
        (1.0, egui::Color32::from_rgb(0x4c, 0x5a, 0x3f)),
    ];
    for (index, (at, color)) in stops.iter().enumerate() {
        let y = screen.top() + screen.height() * at;
        mesh.colored_vertex(egui::pos2(screen.left(), y), *color);
        mesh.colored_vertex(egui::pos2(screen.right(), y), *color);
        if index > 0 {
            let base = u32::try_from(index * 2).expect("few stops");
            mesh.add_triangle(base - 2, base - 1, base + 1);
            mesh.add_triangle(base - 2, base + 1, base);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Writes the part of `rgba` (rows of `width` pixels) inside `rect` as an
/// opaque PNG.
fn write_png(path: &Path, width: u32, rgba: &[u8], rect: egui::Rect) {
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (left, top, right, bottom) = (
        rect.left().max(0.0) as u32,
        rect.top().max(0.0) as u32,
        rect.right().min(f32::from(WIDTH)) as u32,
        rect.bottom().min(f32::from(HEIGHT)) as u32,
    );
    let mut pixels = Vec::new();
    for y in top..bottom {
        let row = (y * width) as usize * 4;
        for x in left..right {
            let at = row + x as usize * 4;
            pixels.extend_from_slice(&[rgba[at], rgba[at + 1], rgba[at + 2], 255]);
        }
    }
    let file = std::io::BufWriter::new(std::fs::File::create(path).expect("a picture file"));
    let mut encoder = png::Encoder::new(file, right - left, bottom - top);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::High);
    let mut writer = encoder.write_header().expect("a PNG header");
    writer.write_image_data(&pixels).expect("the picture");
}

/// Draws `example` in `theme` over a few frames, so sizes settle and nothing
/// is still fading in, and returns the rectangle it took.
fn draw(
    context: &egui::Context,
    host: &Host,
    chain: &IDXGISwapChain,
    painter: &mut Painter,
    theme: Theme,
    example: &Example,
) -> egui::Rect {
    let mut inputs = HashMap::new();
    let mut icons = Icons::default();
    let mut height = 120.0;
    let mut rect = egui::Rect::NOTHING;
    for frame in 0..4 {
        let time = 100.0 + f64::from(frame);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(f32::from(WIDTH), f32::from(HEIGHT)),
            )),
            time: Some(time),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            scenery(ui.ctx());
            let mut f = places::Frame {
                host,
                context: ui.ctx(),
                look: Look {
                    k: 1.0,
                    p: theme.palette(),
                },
                strings: Language::English.strings(),
                inputs: &mut inputs,
                icons: &mut icons,
                now: time,
                screen: ui.ctx().content_rect(),
            };
            rect = places::draw_example(
                &mut f,
                example.name,
                &example.shows,
                egui::pos2(AT, AT),
                &mut height,
            );
        });
        let primitives =
            context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        painter
            .paint(
                chain,
                &primitives,
                &mut output.textures_delta,
                output.pixels_per_point,
            )
            .expect("a painted frame");
    }
    rect
}

/// `gallery.json`: the newest API version, every kind with the version that
/// added it and each of its properties, and the examples with their code
/// and pictures.
fn index(examples: &[Example]) -> serde_json::Value {
    let version = |minor: u16| format!("1.{minor}");
    let shown: Vec<serde_json::Value> = examples
        .iter()
        .map(|example| {
            serde_json::json!({
                "name": example.name,
                "kind": example.kind,
                "about": example.about,
                "code": example.code,
                "light": format!("{}-light.png", example.name),
                "dark": format!("{}-dark.png", example.name),
            })
        })
        .collect();
    let kinds: Vec<serde_json::Value> = angelica_view::data::kinds()
        .map(|(kind, added)| {
            let properties: Vec<serde_json::Value> = angelica_view::data::properties(kind)
                .into_iter()
                .map(|(name, added)| serde_json::json!({ "name": name, "since": version(added) }))
                .collect();
            serde_json::json!({ "kind": kind, "since": version(added), "properties": properties })
        })
        .collect();
    let newest = angelica_view::data::kinds()
        .flat_map(|(kind, added)| {
            std::iter::once(added).chain(
                angelica_view::data::properties(kind)
                    .into_iter()
                    .map(|(_, added)| added),
            )
        })
        .max()
        .unwrap_or(0);
    serde_json::json!({ "api": version(newest), "kinds": kinds, "examples": shown })
}

#[test]
#[ignore = "writes the documentation's gallery; needs Windows' software renderer"]
fn gallery() {
    let out = std::path::PathBuf::from(
        std::env::var_os("GALLERY_OUT").expect("GALLERY_OUT names the folder to write"),
    );
    std::fs::create_dir_all(&out).expect("the gallery folder");
    let folder = tempfile::tempdir().expect("a folder");
    let host = host(folder.path());
    let (window, chain) = super::overlay::create_chain(D3D_DRIVER_TYPE_WARP).expect("a swap chain");
    // SAFETY: the swap chain is live and no view holds its buffers yet.
    unsafe {
        chain.ResizeBuffers(
            1,
            u32::from(WIDTH),
            u32::from(HEIGHT),
            DXGI_FORMAT_R8G8B8A8_UNORM,
            DXGI_SWAP_CHAIN_FLAG(0),
        )
    }
    .expect("the frame's size");
    let mut painter = Painter::new(&chain).expect("a painter");
    // The countdowns count towards a time after now, as in the game.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| u64::try_from(since.as_millis()).unwrap_or(0));
    let examples = examples(now);
    for theme in [Theme::Light, Theme::Dark] {
        let context = egui::Context::default();
        context.set_fonts(crate::fonts::definitions(
            super::overlay::system_fonts().expect("the system's fonts"),
        ));
        theme::apply(&context, theme);
        let suffix = if theme == Theme::Dark {
            "dark"
        } else {
            "light"
        };
        for example in &examples {
            let rect = draw(&context, &host, &chain, &mut painter, theme, example);
            let (width, _, rgba) = painter.read_back(&chain).expect("the frame's pixels");
            write_png(
                &out.join(format!("{}-{suffix}.png", example.name)),
                width,
                &rgba,
                rect.expand(MARGIN),
            );
        }
    }
    std::fs::write(
        out.join("gallery.json"),
        serde_json::to_vec_pretty(&index(&examples)).expect("the index"),
    )
    .expect("gallery.json");
    drop(painter);
    drop(chain);
    // SAFETY: the hidden window belongs to this test and nothing uses it now.
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(window as _) };
}
