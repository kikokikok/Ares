use anyhow::Result;
use log::{info, warn};
use nova_core::NovaEngine;
use nova_client::NovaClient;
use nova_server::NovaServer;
use std::path::PathBuf;

pub struct CliHandler;

impl CliHandler {
    pub fn new() -> Self {
        Self
    }

    pub async fn run_engine(&self, config: PathBuf) -> Result<()> {
        info!("Starting Nova Core Engine with config: {:?}", config);
        
        let engine = NovaEngine::from_config(config).await?;
        engine.run().await?;
        
        Ok(())
    }

    pub async fn start_server(&self, config: PathBuf, port: u16) -> Result<()> {
        info!("Starting Nova Server on port {} with config: {:?}", port, config);
        
        let server = NovaServer::from_config(config, port).await?;
        server.start().await?;
        
        Ok(())
    }

    pub async fn start_client(&self, config: PathBuf, server: String) -> Result<()> {
        info!("Starting Nova Client connecting to {} with config: {:?}", server, config);
        
        let client = NovaClient::from_config(config, server).await?;
        client.connect().await?;
        
        Ok(())
    }

    pub async fn create_project(&self, name: String, path: Option<PathBuf>) -> Result<()> {
        let project_path = path.unwrap_or_else(|| PathBuf::from(&name));
        info!("Creating new Nova project '{}' at {:?}", name, project_path);
        
        std::fs::create_dir_all(&project_path)?;
        
        // Create project structure
        let src_dir = project_path.join("src");
        std::fs::create_dir_all(&src_dir)?;
        
        // Create main.rs
        let main_rs = src_dir.join("main.rs");
        std::fs::write(&main_rs, include_str!("../templates/project_main.rs"))?;
        
        // Create Cargo.toml
        let cargo_toml = project_path.join("Cargo.toml");
        let cargo_content = include_str!("../templates/project_cargo.toml")
            .replace("{{project_name}}", &name);
        std::fs::write(&cargo_toml, cargo_content)?;
        
        // Create engine.toml
        let engine_toml = project_path.join("engine.toml");
        std::fs::write(&engine_toml, include_str!("../templates/engine.toml"))?;
        
        info!("Nova project '{}' created successfully!", name);
        Ok(())
    }

    pub async fn list_plugins(&self) -> Result<()> {
        info!("Listing installed plugins...");
        
        let plugins_dir = PathBuf::from("plugins");
        if !plugins_dir.exists() {
            info!("No plugins directory found");
            return Ok(());
        }
        
        for entry in std::fs::read_dir(plugins_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                info!("Plugin: {}", entry.file_name().to_string_lossy());
            }
        }
        
        Ok(())
    }

    pub async fn install_plugin(&self, plugin: String) -> Result<()> {
        info!("Installing plugin: {}", plugin);
        warn!("Plugin installation not yet implemented");
        Ok(())
    }

    pub async fn uninstall_plugin(&self, plugin: String) -> Result<()> {
        info!("Uninstalling plugin: {}", plugin);
        warn!("Plugin uninstallation not yet implemented");
        Ok(())
    }

    pub async fn create_plugin(&self, name: String) -> Result<()> {
        info!("Creating new plugin: {}", name);
        
        let plugin_dir = PathBuf::from("plugins").join(&name);
        std::fs::create_dir_all(&plugin_dir)?;
        
        let src_dir = plugin_dir.join("src");
        std::fs::create_dir_all(&src_dir)?;
        
        // Create lib.rs
        let lib_rs = src_dir.join("lib.rs");
        let lib_content = include_str!("../templates/plugin_lib.rs")
            .replace("{{plugin_name}}", &name);
        std::fs::write(&lib_rs, lib_content)?;
        
        // Create Cargo.toml
        let cargo_toml = plugin_dir.join("Cargo.toml");
        let cargo_content = include_str!("../templates/plugin_cargo.toml")
            .replace("{{plugin_name}}", &name);
        std::fs::write(&cargo_toml, cargo_content)?;
        
        // Create plugin.toml
        let plugin_toml = plugin_dir.join("plugin.toml");
        let plugin_content = include_str!("../templates/plugin.toml")
            .replace("{{plugin_name}}", &name);
        std::fs::write(&plugin_toml, plugin_content)?;
        
        info!("Plugin '{}' created successfully!", name);
        Ok(())
    }
}