use clap::{Parser, Subcommand};
use log::{error, info};
use std::path::PathBuf;

mod cli;

use cli::CliHandler;

#[derive(Parser)]
#[command(name = "nova")]
#[command(about = "Nova Core Engine - A high-performance modular game engine")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the Nova Core Engine
    Run {
        /// Configuration file path
        #[arg(short, long, default_value = "engine.toml")]
        config: PathBuf,
        /// Enable verbose logging
        #[arg(short, long)]
        verbose: bool,
    },
    /// Start the Nova Server
    Server {
        /// Server configuration file
        #[arg(short, long, default_value = "server.toml")]
        config: PathBuf,
        /// Server port
        #[arg(short, long, default_value = "8080")]
        port: u16,
    },
    /// Start the Nova Client
    Client {
        /// Client configuration file
        #[arg(short, long, default_value = "client.toml")]
        config: PathBuf,
        /// Server address to connect to
        #[arg(short, long, default_value = "127.0.0.1:8080")]
        server: String,
    },
    /// Create a new Nova project
    New {
        /// Project name
        name: String,
        /// Project directory
        #[arg(short, long)]
        path: Option<PathBuf>,
    },
    /// Plugin management commands
    Plugin {
        #[command(subcommand)]
        command: PluginCommands,
    },
}

#[derive(Subcommand)]
enum PluginCommands {
    /// List installed plugins
    List,
    /// Install a plugin
    Install {
        /// Plugin name or path
        plugin: String,
    },
    /// Uninstall a plugin
    Uninstall {
        /// Plugin name
        plugin: String,
    },
    /// Create a new plugin template
    New {
        /// Plugin name
        name: String,
    },
}

#[tokio::main]
async fn main() {
    env_logger::init();
    
    let cli = Cli::parse();
    let handler = CliHandler::new();
    
    let result = match cli.command {
        Commands::Run { config, verbose } => {
            if verbose {
                std::env::set_var("RUST_LOG", "debug");
            }
            handler.run_engine(config).await
        }
        Commands::Server { config, port } => {
            handler.start_server(config, port).await
        }
        Commands::Client { config, server } => {
            handler.start_client(config, server).await
        }
        Commands::New { name, path } => {
            handler.create_project(name, path).await
        }
        Commands::Plugin { command } => {
            match command {
                PluginCommands::List => handler.list_plugins().await,
                PluginCommands::Install { plugin } => handler.install_plugin(plugin).await,
                PluginCommands::Uninstall { plugin } => handler.uninstall_plugin(plugin).await,
                PluginCommands::New { name } => handler.create_plugin(name).await,
            }
        }
    };
    
    if let Err(e) = result {
        error!("Error: {}", e);
        std::process::exit(1);
    }
    
    info!("Nova Core Engine completed successfully");
}