use std::path::PathBuf;

use anyhow::Result;
use clap::{ArgAction, Args, Parser, Subcommand};
use serde_json::to_string_pretty;
use viper_core::{
    CliConfigCommand, CliGlobalOptions, CliOperation, OperationRequest, OperationResult, execute,
};

#[derive(Parser, Debug)]
#[command(
    name = "viper",
    version,
    about = "Conda-compatible environment manager (micromamba-focused)"
)]
struct Cli {
    #[arg(short = 'r', long = "root-prefix", global = true)]
    root_prefix: Option<PathBuf>,

    #[arg(short = 'p', long = "prefix", global = true)]
    prefix: Option<PathBuf>,

    #[arg(short = 'n', long = "name", global = true)]
    name: Option<String>,

    #[arg(short = 'c', long = "channel", global = true)]
    channels: Vec<String>,

    #[arg(long = "json", global = true)]
    json: bool,

    #[arg(short = 'y', long = "yes", global = true)]
    yes: bool,

    #[arg(long = "dry-run", global = true)]
    dry_run: bool,

    #[arg(long = "no-rc", global = true)]
    no_rc: bool,

    #[arg(long = "offline", global = true)]
    offline: bool,

    #[arg(long = "repodata-ttl", global = true)]
    repodata_ttl: Option<usize>,

    #[arg(short = 'v', action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    Create(PackageArgs),
    Install(PackageArgs),
    Remove(RemoveArgs),
    List,
    Info,
    Config(ConfigArgs),
}

#[derive(Args, Debug)]
struct PackageArgs {
    #[arg(value_name = "SPEC")]
    specs: Vec<String>,

    #[arg(short = 'f', long = "file")]
    file: Option<PathBuf>,
}

#[derive(Args, Debug)]
struct RemoveArgs {
    #[arg(value_name = "SPEC")]
    specs: Vec<String>,

    #[arg(long = "all")]
    all: bool,
}

#[derive(Args, Debug)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommands,
}

#[derive(Subcommand, Debug)]
enum ConfigCommands {
    List,
    Get { key: String },
    Set { key: String, value: String },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let globals = CliGlobalOptions {
        root_prefix: cli.root_prefix,
        prefix: cli.prefix,
        name: cli.name,
        channels: cli.channels,
        json: cli.json,
        yes: cli.yes,
        dry_run: cli.dry_run,
        no_rc: cli.no_rc,
        offline: cli.offline,
        repodata_ttl: cli.repodata_ttl,
        verbose: cli.verbose,
    };

    let op = match cli.command {
        Commands::Create(args) => CliOperation::Create {
            specs: args.specs,
            file: args.file,
        },
        Commands::Install(args) => CliOperation::Install {
            specs: args.specs,
            file: args.file,
        },
        Commands::Remove(args) => CliOperation::Remove {
            specs: args.specs,
            all: args.all,
        },
        Commands::List => CliOperation::List,
        Commands::Info => CliOperation::Info,
        Commands::Config(args) => CliOperation::Config(match args.command {
            ConfigCommands::List => CliConfigCommand::List,
            ConfigCommands::Get { key } => CliConfigCommand::Get { key },
            ConfigCommands::Set { key, value } => CliConfigCommand::Set { key, value },
        }),
    };

    let print_json = globals.json;
    let request = OperationRequest { globals, op };

    match execute(request) {
        Ok(result) => {
            print_result(&result, print_json)?;
            Ok(())
        }
        Err(err) => {
            if print_json {
                let body = serde_json::json!({
                    "success": false,
                    "error": err.to_string(),
                });
                println!("{}", to_string_pretty(&body)?);
            } else {
                eprintln!("error: {err}");
            }
            std::process::exit(1);
        }
    }
}

fn print_result(result: &OperationResult, as_json: bool) -> Result<()> {
    if as_json {
        println!("{}", to_string_pretty(result)?);
        return Ok(());
    }

    println!("{}", result.message);
    if result.data != serde_json::Value::Null {
        println!("{}", to_string_pretty(&result.data)?);
    }
    Ok(())
}
