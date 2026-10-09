//! `angelica excel`: the installed game's Excel sheets, for writing plugins
//! and sheet schemas.
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use angelica_map::Map;
use clap::{Args, Subcommand};
use runtime::game_data::{Cell, GameData, Value};
use serde_json::{Value as Json, json};

use crate::Output;

#[derive(Debug, Subcommand)]
pub enum ExcelCommand {
    /// List the game's sheets.
    Sheets(SheetsArgs),
    /// Show a sheet's layout, the map's names for its columns, and rows.
    Show(ShowArgs),
}

#[derive(Debug, Args)]
pub struct SheetsArgs {
    /// The game's `game` folder, with `ffxiv_dx11.exe` and `sqpack`.
    #[arg(long)]
    game: PathBuf,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, Args)]
pub struct ShowArgs {
    /// The sheet's name, such as `Item`.
    sheet: String,
    /// The game's `game` folder, with `ffxiv_dx11.exe` and `sqpack`.
    #[arg(long)]
    game: PathBuf,
    /// A game map directory whose schema names the sheet's columns.
    #[arg(long)]
    map: Option<PathBuf>,
    /// The language: `ja`, `en`, `de`, `fr`, or the game's number.
    #[arg(long, default_value = "en")]
    lang: String,
    /// The first row to show.
    #[arg(long)]
    row: Option<u32>,
    /// How many rows to show, 0 to 1024.
    #[arg(long, default_value_t = 5)]
    count: u32,
    #[command(flatten)]
    output: Output,
}

pub fn run(command: &ExcelCommand) -> ExitCode {
    let (json_output, result) = match command {
        ExcelCommand::Sheets(args) => (args.output.json, sheets(args)),
        ExcelCommand::Show(args) => (args.output.json, show(args)),
    };
    match result {
        Ok((text, document)) => {
            if json_output {
                println!("{document}");
            } else {
                print!("{text}");
            }
            ExitCode::SUCCESS
        }
        Err(message) => {
            if json_output {
                println!("{}", json!({ "error": { "message": message } }));
            } else {
                eprintln!("error: {message}");
            }
            ExitCode::FAILURE
        }
    }
}

fn data(game: &std::path::Path) -> GameData {
    GameData::new(game.parent())
}

fn sheets(args: &SheetsArgs) -> Result<(String, Json), String> {
    let names = data(&args.game).sheets().map_err(|e| e.message)?;
    let text = names.iter().fold(String::new(), |mut text, name| {
        let _ = writeln!(text, "{name}");
        text
    });
    Ok((text, json!({ "sheets": names })))
}

fn language(text: &str) -> Result<u8, String> {
    Ok(match text {
        "ja" => 1,
        "en" => 2,
        "de" => 3,
        "fr" => 4,
        other => other
            .parse()
            .map_err(|_| format!("{other:?} is not a language: ja, en, de, fr, or a number"))?,
    })
}

/// A cell as readable text: game strings with their macros as `<…>`.
fn cell_text(cell: &Cell) -> String {
    match cell {
        Cell::Text(bytes) => {
            let mut out = String::new();
            let mut at = 0;
            while at < bytes.len() {
                if bytes[at] == 0x02 && at + 2 < bytes.len() {
                    let _ = write!(out, "<{:02X}>", bytes[at + 1]);
                    let length = usize::from(bytes[at + 2]);
                    at += 3 + length;
                    continue;
                }
                let end = bytes[at..]
                    .iter()
                    .position(|&b| b == 0x02)
                    .map_or(bytes.len(), |end| at + end);
                out.push_str(&String::from_utf8_lossy(&bytes[at..end]));
                at = end;
            }
            out
        }
        Cell::Value(Value::Boolean(value)) => value.to_string(),
        Cell::Value(Value::Signed(value)) => value.to_string(),
        Cell::Value(Value::Unsigned(value)) => value.to_string(),
        Cell::Value(Value::Float(value)) => value.to_string(),
    }
}

fn show(args: &ShowArgs) -> Result<(String, Json), String> {
    if args.count > 1024 {
        return Err("--count is 0 to 1024".into());
    }
    let data = data(&args.game);
    let info = data.sheet(&args.sheet).map_err(|e| e.message)?;
    let layout = info.layout();
    let fingerprint = layout.fingerprint();
    let mut names = BTreeMap::new();
    let mut schema_status = Json::Null;
    if let Some(map) = &args.map {
        let map = Map::load(map).map_err(|e| e.to_string())?;
        schema_status = match map.sheet(&args.sheet) {
            None => json!({ "known": false, "reason": "the map has no schema of this sheet" }),
            Some(schema) => match schema.resolve(&layout) {
                Ok(columns) => {
                    names = columns
                        .into_iter()
                        .map(|(name, index)| (index, name))
                        .collect();
                    json!({ "known": true })
                }
                Err(reason) => json!({ "known": false, "reason": reason }),
            },
        };
    }
    let code = language(&args.lang)?;
    let rows = if args.count == 0 {
        Vec::new()
    } else {
        let after = args
            .row
            .and_then(|row| row.checked_sub(1))
            .map(|row| (row, u16::MAX));
        data.rows(&args.sheet, code, after, args.count)
            .map_err(|e| e.message)?
    };

    let mut text = format!(
        "{} ({}), {} rows, languages {:?}, layout {fingerprint}\n",
        info.name,
        if info.subrows { "subrows" } else { "rows" },
        info.rows,
        info.languages
    );
    if let Some(reason) = schema_status.get("reason") {
        let _ = writeln!(text, "map: {}", reason.as_str().unwrap_or_default());
    }
    let columns: Vec<Json> = layout
        .columns
        .iter()
        .enumerate()
        .map(|(index, (column_type, offset))| {
            let index = u16::try_from(index).unwrap_or(u16::MAX);
            let name = names.get(&index);
            let _ = writeln!(
                text,
                "  {index:>4} {:<14} @{offset:<5} {}",
                column_type.name(),
                name.map_or("", String::as_str)
            );
            json!({ "index": index, "type": column_type.name(), "offset": offset, "name": name })
        })
        .collect();
    let rows: Vec<Json> = rows
        .iter()
        .map(|row| {
            let _ = writeln!(text, "row {}.{}", row.row, row.subrow);
            let cells: Vec<String> = row.cells.iter().map(cell_text).collect();
            for (index, cell) in cells.iter().enumerate() {
                let _ = writeln!(text, "  {index:>4} {cell}");
            }
            json!({ "row": row.row, "subrow": row.subrow, "cells": cells })
        })
        .collect();
    Ok((
        text,
        json!({
            "sheet": info.name,
            "subrows": info.subrows,
            "rows": info.rows,
            "languages": info.languages,
            "layout": fingerprint,
            "map": schema_status,
            "columns": columns,
            "data": rows,
        }),
    ))
}
