//! The `angelica` command.

mod compile;
mod dev;
mod verify;

use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "angelica", version, about = "Angelica tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Game map tools.
    #[command(subcommand)]
    Map(MapCommand),
    /// Plugin tools.
    #[command(subcommand)]
    Plugin(PluginCommand),
    /// A signed local installation for development.
    #[command(subcommand)]
    Dev(dev::DevCommand),
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Compile a component outside the game for the trusted package builder.
    Compile(compile::CompileArgs),
}

#[derive(Debug, Subcommand)]
enum MapCommand {
    /// Resolve every entry of a map against the game's executable.
    Verify(verify::VerifyArgs),
}

/// Output options shared by every command.
#[derive(Debug, Args)]
struct Output {
    /// Write one JSON document to standard output instead of text.
    #[arg(long)]
    json: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Map(MapCommand::Verify(args)) => verify::run(&args),
        Command::Plugin(PluginCommand::Compile(args)) => compile::run(&args),
        Command::Dev(command) => dev::run(&command),
    }
}
