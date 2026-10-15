//! `angelica exe`: the game's executable, for writing map entries:
//! disassembly, signature search, and unique signatures.
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use angelica_map::Map;
use angelica_pe::{Image, Layout, Signature};
use clap::{Args, Subcommand};
use iced_x86::{Decoder, DecoderOptions, Formatter, Instruction, IntelFormatter};
use serde_json::{Value as Json, json};

use crate::Output;

const EXECUTABLE: &str = "ffxiv_dx11.exe";
/// The longest signature `signature` proposes.
const MAX_SIGNATURE: usize = 96;

#[derive(Debug, Subcommand)]
pub enum ExeCommand {
    /// Disassemble from an address or a map entry.
    Disasm(DisasmArgs),
    /// Every address in the code sections where a signature matches.
    Find(FindArgs),
    /// The shortest unique signature that starts at an instruction.
    Signature(SignatureArgs),
}

#[derive(Debug, Args)]
pub struct Target {
    /// The game's `game` folder, with `ffxiv_dx11.exe`.
    #[arg(long)]
    game: PathBuf,
    /// An address relative to the image base, such as `0x9B27A0`.
    #[arg(long, value_parser = parse_rva, conflicts_with = "entry")]
    rva: Option<u32>,
    /// A function, location, global, or patch entry of the map.
    #[arg(long, requires = "map")]
    entry: Option<String>,
    /// The game map directory for `--entry`.
    #[arg(long)]
    map: Option<PathBuf>,
}

#[derive(Debug, Args)]
pub struct DisasmArgs {
    #[command(flatten)]
    target: Target,
    /// The most instructions to show; a function is shown to its end.
    #[arg(long, default_value_t = 64)]
    count: usize,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, Args)]
pub struct FindArgs {
    /// The game's `game` folder, with `ffxiv_dx11.exe`.
    #[arg(long)]
    game: PathBuf,
    /// Hexadecimal bytes with `??` for any byte.
    #[arg(long)]
    signature: String,
    #[command(flatten)]
    output: Output,
}

#[derive(Debug, Args)]
pub struct SignatureArgs {
    #[command(flatten)]
    target: Target,
    /// Make the signature unique inside the function containing the
    /// instruction, for an `in` locator, and leave out the instruction's
    /// own displacement, which the locator reads.
    #[arg(long)]
    within: bool,
    /// With `--within`: the start of the function, as the `in` locator's
    /// function entry resolves; needed for a leaf function, which the
    /// exception directory does not record.
    #[arg(long, value_parser = parse_rva, requires = "within")]
    function: Option<u32>,
    #[command(flatten)]
    output: Output,
}

fn parse_rva(text: &str) -> Result<u32, String> {
    let digits = text
        .strip_prefix("0x")
        .or_else(|| text.strip_prefix("0X"))
        .unwrap_or(text);
    u32::from_str_radix(digits, 16)
        .map_err(|e| format!("{text:?} is not a hexadecimal address: {e}"))
}

pub fn run(command: &ExeCommand) -> ExitCode {
    let (json_output, result) = match command {
        ExeCommand::Disasm(args) => (args.output.json, disasm(args)),
        ExeCommand::Find(args) => (args.output.json, find(args)),
        ExeCommand::Signature(args) => (args.output.json, signature(args)),
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

fn executable(game: &Path) -> Result<Vec<u8>, String> {
    let path = game.join(EXECUTABLE);
    std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))
}

fn image(bytes: &[u8]) -> Result<Image<'_>, String> {
    Image::parse(bytes, Layout::File).map_err(|e| e.to_string())
}

/// The address the target names.
fn address(target: &Target, image: &Image<'_>) -> Result<u32, String> {
    if let Some(rva) = target.rva {
        return Ok(rva);
    }
    let (Some(name), Some(map)) = (&target.entry, &target.map) else {
        return Err("give --rva, or --entry with --map".into());
    };
    let map = Map::load(map).map_err(|e| e.to_string())?;
    let resolution = map.resolve(image, image.image_base());
    match resolution.results().get(name) {
        Some(Ok(angelica_map::Value::Address(rva))) => Ok(*rva),
        Some(Ok(angelica_map::Value::Offset(_))) => {
            Err(format!("{name} is a field; it has no address"))
        }
        Some(Err(absence)) => Err(format!("{name} is absent: {absence}")),
        None => Err(format!("the map has no entry {name}")),
    }
}

fn decoder<'a>(image: &Image<'a>, rva: u32, end: u32) -> Result<Decoder<'a>, String> {
    if !image.is_code(rva) {
        return Err(format!("0x{rva:X} is not in a code section"));
    }
    let bytes = image
        .bytes_at(rva, (end - rva) as usize)
        .ok_or_else(|| format!("0x{rva:X} is outside the image"))?;
    Ok(Decoder::with_ip(
        64,
        bytes,
        image.image_base() + u64::from(rva),
        DecoderOptions::NONE,
    ))
}

/// The end of the code to decode from `rva`: its function's end, or at most
/// 4 KiB.
fn code_end(image: &Image<'_>, rva: u32) -> u32 {
    let section_end = image.section_at(rva).map_or(rva, |section| {
        section.virtual_address + section.virtual_size
    });
    image
        .function_at(rva)
        .map_or_else(|| rva.saturating_add(4096), |range| range.end)
        .min(section_end)
}

fn disasm(args: &DisasmArgs) -> Result<(String, Json), String> {
    let bytes = executable(&args.target.game)?;
    let image = image(&bytes)?;
    let rva = address(&args.target, &image)?;
    let end = code_end(&image, rva);
    let function = image.function_at(rva);
    let mut decoder = decoder(&image, rva, end)?;
    let mut formatter = IntelFormatter::new();
    let base = image.image_base();
    let mut text = match &function {
        Some(range) => format!("function 0x{:X}..0x{:X}\n", range.start, range.end),
        None => "no function entry at this address\n".to_owned(),
    };
    let mut lines = Vec::new();
    let mut instruction = Instruction::default();
    while decoder.can_decode() && lines.len() < args.count {
        decoder.decode_out(&mut instruction);
        let at = u32::try_from(instruction.ip() - base).unwrap_or(u32::MAX);
        let start = (at - rva) as usize;
        let raw = &decoder_bytes(&image, at, instruction.len());
        let mut assembly = String::new();
        formatter.format(&instruction, &mut assembly);
        let hex = hex(raw);
        let _ = writeln!(text, "0x{at:08X}  {hex:<36} {assembly}");
        lines.push(json!({ "rva": at, "offset": start, "bytes": hex, "text": assembly }));
        if instruction.flow_control() == iced_x86::FlowControl::Return && function.is_none() {
            break;
        }
    }
    Ok((
        text,
        json!({
            "rva": rva,
            "function": function.map(|range| json!({ "start": range.start, "end": range.end })),
            "instructions": lines,
        }),
    ))
}

fn decoder_bytes<'a>(image: &Image<'a>, rva: u32, len: usize) -> &'a [u8] {
    image.bytes_at(rva, len).unwrap_or_default()
}

fn hex(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|byte| format!("{byte:02X}"))
        .collect::<Vec<_>>()
        .join(" ")
}

fn find(args: &FindArgs) -> Result<(String, Json), String> {
    let bytes = executable(&args.game)?;
    let image = image(&bytes)?;
    let signature = Signature::parse(&args.signature).map_err(|e| e.to_string())?;
    let matches = image.scan_code(&signature);
    let mut text = format!("{} matches\n", matches.len());
    for rva in &matches {
        let function = image.function_at(*rva).map_or_else(String::new, |range| {
            format!(" in function 0x{:X}", range.start)
        });
        let _ = writeln!(text, "  0x{rva:X}{function}");
    }
    Ok((text, json!({ "matches": matches })))
}

/// The pattern of one instruction: its bytes, with `None` for the bytes of
/// a branch target, a RIP-relative displacement, and, when `displacement`,
/// any displacement.
fn pattern(
    decoder: &Decoder<'_>,
    instruction: &Instruction,
    raw: &[u8],
    displacement: bool,
) -> Vec<Option<u8>> {
    let mut pattern: Vec<Option<u8>> = raw.iter().copied().map(Some).collect();
    let offsets = decoder.get_constant_offsets(instruction);
    let mut blank = |offset: usize, size: usize| {
        for byte in pattern.iter_mut().skip(offset).take(size) {
            *byte = None;
        }
    };
    if offsets.has_displacement() && (instruction.is_ip_rel_memory_operand() || displacement) {
        blank(offsets.displacement_offset(), offsets.displacement_size());
    }
    let branch = matches!(
        instruction.flow_control(),
        iced_x86::FlowControl::Call
            | iced_x86::FlowControl::UnconditionalBranch
            | iced_x86::FlowControl::ConditionalBranch
    );
    if branch && offsets.has_immediate() {
        blank(offsets.immediate_offset(), offsets.immediate_size());
    }
    pattern
}

fn pattern_text(pattern: &[Option<u8>]) -> String {
    pattern
        .iter()
        .map(|byte| byte.map_or_else(|| "??".to_owned(), |byte| format!("{byte:02X}")))
        .collect::<Vec<_>>()
        .join(" ")
}

fn signature(args: &SignatureArgs) -> Result<(String, Json), String> {
    let bytes = executable(&args.target.game)?;
    let image = image(&bytes)?;
    let rva = address(&args.target, &image)?;
    // The code an `in` locator searches: the function's code as the map
    // reads it, every chunk of an exception directory entry or a leaf
    // function's flow.
    let scope = if args.within {
        let code = match args.function {
            Some(start) => angelica_map::function_code(&image, start).ok_or_else(|| {
                format!("no function code starts at 0x{start:X}, as an `in` locator reads it")
            })?,
            None => angelica_map::function_holding(&image, rva).ok_or_else(|| {
                format!(
                    "0x{rva:X} is in no function of the exception directory; for a leaf function, give its start with --function"
                )
            })?,
        };
        if !code.ranges.iter().any(|range| range.contains(&rva)) {
            return Err(format!(
                "0x{rva:X} is not in the code of the function at 0x{:X}",
                code.start
            ));
        }
        Some(code)
    } else {
        None
    };
    let end = code_end(&image, rva);
    let mut decoder = decoder(&image, rva, end)?;
    let base = image.image_base();
    let mut instruction = Instruction::default();
    let mut pattern: Vec<Option<u8>> = Vec::new();
    let mut first = true;
    while decoder.can_decode() && pattern.len() < MAX_SIGNATURE {
        decoder.decode_out(&mut instruction);
        let at = u32::try_from(instruction.ip() - base).unwrap_or(u32::MAX);
        let raw = decoder_bytes(&image, at, instruction.len());
        pattern.extend(self::pattern(
            &decoder,
            &instruction,
            raw,
            first && args.within,
        ));
        first = false;
        let text = pattern_text(&pattern);
        let signature = Signature::parse(&text).map_err(|e| e.to_string())?;
        let matches: Vec<u32> = image
            .scan_code(&signature)
            .into_iter()
            .filter(|found| {
                scope
                    .as_ref()
                    .is_none_or(|code| code.ranges.iter().any(|range| range.contains(found)))
            })
            .collect();
        if matches == [rva] {
            let scope_text = scope.as_ref().map_or_else(
                || "the code sections".to_owned(),
                |code| format!("function 0x{:X}", code.start),
            );
            return Ok((
                format!("{text}\nunique in {scope_text}, {} bytes\n", pattern.len()),
                json!({ "signature": text, "bytes": pattern.len(), "within": scope.map(|code| code.start) }),
            ));
        }
    }
    Err(format!(
        "no unique signature of at most {MAX_SIGNATURE} bytes starts at 0x{rva:X}"
    ))
}
