//! The `angelica` command.

mod author;
mod compile;
mod dev;
mod excel;
mod exe;
mod new;
mod release;
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
    /// The installed game's Excel sheets.
    #[command(subcommand)]
    Excel(excel::ExcelCommand),
    /// The game's executable: disassembly and signatures for map entries.
    #[command(subcommand)]
    Exe(exe::ExeCommand),
    /// A signed local installation for development.
    #[command(subcommand)]
    Dev(dev::DevCommand),
    /// Release signing and update feeds, for the release workflows.
    #[command(subcommand)]
    Release(release::ReleaseCommand),
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    /// Compile a component outside the game for the trusted package builder.
    Compile(compile::CompileArgs),
    /// Create a plugin project: code, manifest, tests, and agent guidance.
    New(new::NewArgs),
    /// Create the author key that signs plugin files, or show it.
    Key(author::KeyArgs),
    /// Sign a plugin into one file players install, outside the catalog.
    Pack(author::PackArgs),
    /// Publish a plugin behind a link that stays the same (GitHub release).
    Publish(author::PublishArgs),
    /// Install a plugin file or link into an installation: check the author, compile.
    Install(author::InstallArgs),
    /// Update the plugins installed from links to newer versions of the same author.
    Update(author::UpdateArgs),
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
        Command::Plugin(PluginCommand::New(args)) => new::run(&args),
        Command::Plugin(PluginCommand::Key(args)) => author::key(&args),
        Command::Plugin(PluginCommand::Pack(args)) => author::pack(&args),
        Command::Plugin(PluginCommand::Publish(args)) => author::publish(&args),
        Command::Plugin(PluginCommand::Install(args)) => author::install(&args),
        Command::Plugin(PluginCommand::Update(args)) => author::update(&args),
        Command::Dev(command) => dev::run(&command),
        Command::Release(command) => release::run(&command),
        Command::Excel(command) => excel::run(&command),
        Command::Exe(command) => exe::run(&command),
    }
}
