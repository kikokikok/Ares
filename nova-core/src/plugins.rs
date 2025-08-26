use crate::error::{NovaError, NovaResult};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Plugin trait that all plugins must implement
#[async_trait]
pub trait Plugin: Send + Sync {
    /// Plugin name
    fn name(&self) -> &str;

    /// Plugin version
    fn version(&self) -> &str;

    /// Plugin description
    fn description(&self) -> &str;

    /// Initialize the plugin
    async fn initialize(&mut self, context: &PluginContext) -> NovaResult<()>;

    /// Update the plugin (called every frame)
    async fn update(&mut self, delta_time: f64) -> NovaResult<()>;

    /// Shutdown the plugin
    async fn shutdown(&mut self) -> NovaResult<()>;

    /// Plugin dependencies
    fn dependencies(&self) -> Vec<String> {
        Vec::new()
    }

    /// Plugin capabilities
    fn capabilities(&self) -> Vec<String> {
        Vec::new()
    }
}

/// Plugin context provided during initialization
pub struct PluginContext {
    pub engine_version: String,
    pub config: PluginConfig,
    pub data_directory: PathBuf,
    pub shared_data: Arc<RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>>,
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub name: String,
    pub version: String,
    pub author: String,
    pub description: String,
    pub dependencies: Vec<String>,
    pub permissions: PluginPermissions,
    pub settings: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginPermissions {
    pub resource_access: Vec<String>,
    pub event_access: Vec<String>,
    pub network_access: bool,
    pub file_system_access: bool,
}

/// Plugin registry entry
pub struct PluginEntry {
    pub id: Uuid,
    pub plugin: Box<dyn Plugin>,
    pub config: PluginConfig,
    pub status: PluginStatus,
    pub load_time: std::time::Instant,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PluginStatus {
    Loaded,
    Initialized,
    Running,
    Stopped,
    Error(String),
}

/// Plugin manager responsible for loading, managing, and coordinating plugins
pub struct PluginManager {
    plugins: Arc<RwLock<HashMap<String, PluginEntry>>>,
    plugin_directory: PathBuf,
    auto_load: bool,
    hot_reload: bool,
    context: PluginContext,
}

impl PluginManager {
    /// Create a new plugin manager
    pub fn new(
        plugin_directory: PathBuf,
        auto_load: bool,
        hot_reload: bool,
        engine_version: String,
    ) -> Self {
        let context = PluginContext {
            engine_version,
            config: PluginConfig {
                name: String::new(),
                version: String::new(),
                author: String::new(),
                description: String::new(),
                dependencies: Vec::new(),
                permissions: PluginPermissions {
                    resource_access: Vec::new(),
                    event_access: Vec::new(),
                    network_access: false,
                    file_system_access: false,
                },
                settings: HashMap::new(),
            },
            data_directory: plugin_directory.clone(),
            shared_data: Arc::new(RwLock::new(HashMap::new())),
        };

        Self {
            plugins: Arc::new(RwLock::new(HashMap::new())),
            plugin_directory,
            auto_load,
            hot_reload,
            context,
        }
    }

    /// Load a plugin from a directory
    pub async fn load_plugin<P: AsRef<Path>>(&self, plugin_path: P) -> NovaResult<Uuid> {
        let plugin_path = plugin_path.as_ref();
        let config_path = plugin_path.join("plugin.toml");

        if !config_path.exists() {
            return Err(NovaError::plugin(format!(
                "Plugin configuration not found: {:?}",
                config_path
            )));
        }

        let config_content = std::fs::read_to_string(&config_path)?;
        let config: PluginConfig = toml::from_str(&config_content)?;

        // Check dependencies
        self.check_dependencies(&config.dependencies).await?;

        // For this implementation, we'll create a sample plugin
        // In a real implementation, this would use dynamic library loading
        let plugin = SamplePlugin::new(&config.name, &config.version, &config.description);

        let id = Uuid::new_v4();
        let entry = PluginEntry {
            id,
            plugin: Box::new(plugin),
            config: config.clone(),
            status: PluginStatus::Loaded,
            load_time: std::time::Instant::now(),
        };

        let mut plugins = self.plugins.write().await;
        plugins.insert(config.name.clone(), entry);

        log::info!("Loaded plugin: {} v{}", config.name, config.version);
        Ok(id)
    }

    /// Initialize a loaded plugin
    pub async fn initialize_plugin(&self, plugin_name: &str) -> NovaResult<()> {
        let mut plugins = self.plugins.write().await;

        if let Some(entry) = plugins.get_mut(plugin_name) {
            if entry.status != PluginStatus::Loaded {
                return Err(NovaError::plugin(format!(
                    "Plugin {} is not in loaded state",
                    plugin_name
                )));
            }

            match entry.plugin.initialize(&self.context).await {
                Ok(()) => {
                    entry.status = PluginStatus::Initialized;
                    log::info!("Initialized plugin: {}", plugin_name);
                    Ok(())
                }
                Err(e) => {
                    entry.status = PluginStatus::Error(e.to_string());
                    Err(e)
                }
            }
        } else {
            Err(NovaError::plugin(format!(
                "Plugin {} not found",
                plugin_name
            )))
        }
    }

    /// Start a plugin
    pub async fn start_plugin(&self, plugin_name: &str) -> NovaResult<()> {
        let mut plugins = self.plugins.write().await;

        if let Some(entry) = plugins.get_mut(plugin_name) {
            if entry.status != PluginStatus::Initialized {
                return Err(NovaError::plugin(format!(
                    "Plugin {} is not initialized",
                    plugin_name
                )));
            }

            entry.status = PluginStatus::Running;
            log::info!("Started plugin: {}", plugin_name);
            Ok(())
        } else {
            Err(NovaError::plugin(format!(
                "Plugin {} not found",
                plugin_name
            )))
        }
    }

    /// Stop a plugin
    pub async fn stop_plugin(&self, plugin_name: &str) -> NovaResult<()> {
        let mut plugins = self.plugins.write().await;

        if let Some(entry) = plugins.get_mut(plugin_name) {
            match entry.plugin.shutdown().await {
                Ok(()) => {
                    entry.status = PluginStatus::Stopped;
                    log::info!("Stopped plugin: {}", plugin_name);
                    Ok(())
                }
                Err(e) => {
                    entry.status = PluginStatus::Error(e.to_string());
                    Err(e)
                }
            }
        } else {
            Err(NovaError::plugin(format!(
                "Plugin {} not found",
                plugin_name
            )))
        }
    }

    /// Unload a plugin
    pub async fn unload_plugin(&self, plugin_name: &str) -> NovaResult<()> {
        let mut plugins = self.plugins.write().await;

        if plugins.remove(plugin_name).is_some() {
            log::info!("Unloaded plugin: {}", plugin_name);
            Ok(())
        } else {
            Err(NovaError::plugin(format!(
                "Plugin {} not found",
                plugin_name
            )))
        }
    }

    /// Update all running plugins
    pub async fn update_plugins(&self, delta_time: f64) -> NovaResult<()> {
        let mut plugins = self.plugins.write().await;

        for (name, entry) in plugins.iter_mut() {
            if entry.status == PluginStatus::Running {
                if let Err(e) = entry.plugin.update(delta_time).await {
                    log::error!("Plugin {} update failed: {}", name, e);
                    entry.status = PluginStatus::Error(e.to_string());
                }
            }
        }

        Ok(())
    }

    /// Get plugin status
    pub async fn get_plugin_status(&self, plugin_name: &str) -> Option<PluginStatus> {
        let plugins = self.plugins.read().await;
        plugins.get(plugin_name).map(|entry| entry.status.clone())
    }

    /// List all loaded plugins
    pub async fn list_plugins(&self) -> Vec<String> {
        let plugins = self.plugins.read().await;
        plugins.keys().cloned().collect()
    }

    /// Auto-load plugins from directory
    pub async fn auto_load_plugins(&self) -> NovaResult<()> {
        if !self.auto_load {
            return Ok(());
        }

        if !self.plugin_directory.exists() {
            log::warn!(
                "Plugin directory does not exist: {:?}",
                self.plugin_directory
            );
            return Ok(());
        }

        for entry in std::fs::read_dir(&self.plugin_directory)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                let plugin_path = entry.path();
                match self.load_plugin(&plugin_path).await {
                    Ok(_) => {
                        let plugin_name = plugin_path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy();

                        if let Err(e) = self.initialize_plugin(&plugin_name).await {
                            log::error!("Failed to initialize plugin {}: {}", plugin_name, e);
                        } else if let Err(e) = self.start_plugin(&plugin_name).await {
                            log::error!("Failed to start plugin {}: {}", plugin_name, e);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to load plugin from {:?}: {}", plugin_path, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Check plugin dependencies
    async fn check_dependencies(&self, dependencies: &[String]) -> NovaResult<()> {
        let plugins = self.plugins.read().await;

        for dep in dependencies {
            if !plugins.contains_key(dep) {
                return Err(NovaError::plugin(format!("Missing dependency: {}", dep)));
            }
        }

        Ok(())
    }
}

/// Sample plugin implementation for demonstration
pub struct SamplePlugin {
    name: String,
    version: String,
    description: String,
}

impl SamplePlugin {
    pub fn new(name: &str, version: &str, description: &str) -> Self {
        Self {
            name: name.to_string(),
            version: version.to_string(),
            description: description.to_string(),
        }
    }
}

#[async_trait]
impl Plugin for SamplePlugin {
    fn name(&self) -> &str {
        &self.name
    }

    fn version(&self) -> &str {
        &self.version
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn initialize(&mut self, _context: &PluginContext) -> NovaResult<()> {
        log::info!("Initializing sample plugin: {}", self.name);
        Ok(())
    }

    async fn update(&mut self, _delta_time: f64) -> NovaResult<()> {
        // Sample plugin update logic
        Ok(())
    }

    async fn shutdown(&mut self) -> NovaResult<()> {
        log::info!("Shutting down sample plugin: {}", self.name);
        Ok(())
    }
}
