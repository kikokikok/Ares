use crate::error::{NovaError, NovaResult};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Engine configuration structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineConfig {
    pub engine: EngineSettings,
    pub memory: MemoryConfig,
    pub threading: ThreadingConfig,
    pub events: EventConfig,
    pub plugins: PluginConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngineSettings {
    pub name: String,
    pub version: String,
    pub debug: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    pub heap_size: String,
    pub pool_count: usize,
    pub gc_threshold: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadingConfig {
    pub worker_threads: usize,
    pub main_thread_affinity: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventConfig {
    pub queue_size: usize,
    pub priority_levels: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub directory: PathBuf,
    pub auto_load: bool,
    pub hot_reload: bool,
    pub enabled: Vec<String>,
    pub disabled: Vec<String>,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            engine: EngineSettings {
                name: "Nova Engine".to_string(),
                version: "1.0.0".to_string(),
                debug: false,
            },
            memory: MemoryConfig {
                heap_size: "1GB".to_string(),
                pool_count: 16,
                gc_threshold: 0.8,
            },
            threading: ThreadingConfig {
                worker_threads: num_cpus::get(),
                main_thread_affinity: None,
            },
            events: EventConfig {
                queue_size: 10000,
                priority_levels: 5,
            },
            plugins: PluginConfig {
                directory: PathBuf::from("plugins"),
                auto_load: true,
                hot_reload: true,
                enabled: Vec::new(),
                disabled: Vec::new(),
            },
        }
    }
}

impl EngineConfig {
    /// Load configuration from TOML file
    pub fn load_from_file<P: AsRef<std::path::Path>>(path: P) -> NovaResult<Self> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| NovaError::config(format!("Failed to read config file: {}", e)))?;

        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to TOML file
    pub fn save_to_file<P: AsRef<std::path::Path>>(&self, path: P) -> NovaResult<()> {
        let content = toml::to_string_pretty(self)
            .map_err(|e| NovaError::config(format!("Failed to serialize config: {}", e)))?;

        std::fs::write(path, content)
            .map_err(|e| NovaError::config(format!("Failed to write config file: {}", e)))?;

        Ok(())
    }

    /// Merge with another configuration, preferring values from `other`
    pub fn merge(&mut self, other: EngineConfig) {
        // Simple merge - in a real implementation, this would be more sophisticated
        *self = other;
    }

    /// Validate configuration values
    pub fn validate(&self) -> NovaResult<()> {
        if self.memory.pool_count == 0 {
            return Err(NovaError::config("Memory pool count cannot be zero"));
        }

        if self.memory.gc_threshold < 0.1 || self.memory.gc_threshold > 1.0 {
            return Err(NovaError::config(
                "GC threshold must be between 0.1 and 1.0",
            ));
        }

        if self.threading.worker_threads == 0 {
            return Err(NovaError::config("Worker thread count cannot be zero"));
        }

        if self.events.queue_size == 0 {
            return Err(NovaError::config("Event queue size cannot be zero"));
        }

        if self.events.priority_levels == 0 {
            return Err(NovaError::config("Priority levels cannot be zero"));
        }

        Ok(())
    }
}

/// Helper function to parse memory size strings like "1GB", "512MB"
pub fn parse_memory_size(size_str: &str) -> NovaResult<usize> {
    let size_str = size_str.trim().to_uppercase();

    if let Some(num_str) = size_str.strip_suffix("GB") {
        let num: f64 = num_str
            .parse()
            .map_err(|_| NovaError::config("Invalid memory size format"))?;
        Ok((num * 1024.0 * 1024.0 * 1024.0) as usize)
    } else if let Some(num_str) = size_str.strip_suffix("MB") {
        let num: f64 = num_str
            .parse()
            .map_err(|_| NovaError::config("Invalid memory size format"))?;
        Ok((num * 1024.0 * 1024.0) as usize)
    } else if let Some(num_str) = size_str.strip_suffix("KB") {
        let num: f64 = num_str
            .parse()
            .map_err(|_| NovaError::config("Invalid memory size format"))?;
        Ok((num * 1024.0) as usize)
    } else if let Some(num_str) = size_str.strip_suffix("B") {
        let num: usize = num_str
            .parse()
            .map_err(|_| NovaError::config("Invalid memory size format"))?;
        Ok(num)
    } else {
        // Assume bytes if no suffix
        let num: usize = size_str
            .parse()
            .map_err(|_| NovaError::config("Invalid memory size format"))?;
        Ok(num)
    }
}

// Add num_cpus dependency for worker thread detection
// This would be added to Cargo.toml in a real implementation
