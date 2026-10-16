//! Checks against the installed game executable. Ignored by default; run with
//! `ANGELICA_GAME_PATH` set to the game's `game` folder:
//!
//! ```text
//! cargo test -p angelica-pe --test game -- --ignored
//! ```

use std::path::PathBuf;

use angelica_pe::testing::map_file;
use angelica_pe::{Image, Layout, Signature};

fn executable() -> Vec<u8> {
    let folder =
        std::env::var_os("ANGELICA_GAME_PATH").expect("ANGELICA_GAME_PATH names the game folder");
    std::fs::read(PathBuf::from(folder).join("ffxiv_dx11.exe"))
        .expect("the game executable is readable")
}

#[test]
#[ignore = "needs the installed game"]
fn reads_the_game_executable() {
    let file = executable();
    let image = Image::parse(&file, Layout::File).unwrap();
    let text = image.section(".text").expect("a .text section");
    assert!(text.is_code());
    assert_eq!(
        image
            .section_at(image.entry_point())
            .map(|section| section.name.as_str()),
        Some(".text")
    );
}

#[test]
#[ignore = "needs the installed game"]
fn file_and_mapped_layouts_find_the_same_code() {
    let file = executable();
    let mapped = map_file(&file);
    let on_disk = Image::parse(&file, Layout::File).unwrap();
    let in_memory = Image::parse(&mapped, Layout::Mapped).unwrap();

    // The entry point's first bytes, as a signature, must be found at the
    // same RVAs in both layouts, and the entry point must be one of them.
    let prologue = on_disk.bytes_at(on_disk.entry_point(), 16).unwrap();
    let text = prologue
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ");
    let signature = Signature::parse(&text).unwrap();
    let found = on_disk.scan_code(&signature);
    assert!(found.contains(&on_disk.entry_point()));
    assert_eq!(found, in_memory.scan_code(&signature));
}

#[test]
#[ignore = "needs the installed game"]
fn the_game_imports_set_cursor_once_in_both_layouts() {
    let file = executable();
    let mapped = map_file(&file);
    let on_disk = Image::parse(&file, Layout::File).unwrap();
    let in_memory = Image::parse(&mapped, Layout::Mapped).unwrap();
    let slots = on_disk.import_slots("USER32.dll", "SetCursor");
    assert_eq!(slots.len(), 1, "{slots:x?}");
    assert_eq!(in_memory.import_slots("USER32.dll", "SetCursor"), slots);
}
