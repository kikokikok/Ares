use log::{error, info, warn};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

use crate::config::EngineConfig;
use crate::error::NovaResult;
use crate::events::{EngineStartEvent, EngineStopEvent, EventSystem};
use crate::plugins::PluginManager;
use crate::resources::ResourceManager;
use crate::threading::ThreadEngine;

/// Main Nova Engine instance that coordinates all subsystems
pub struct NovaEngine {
    config: EngineConfig,
    resource_manager: Arc<ResourceManager>,
    event_system: Arc<EventSystem>,
    thread_engine: Arc<ThreadEngine>,
    plugin_manager: Arc<Mutex<PluginManager>>,
    running: Arc<std::sync::atomic::AtomicBool>,
    start_time: Instant,
}

impl NovaEngine {
    /// Create a new Nova Engine with default configuration
    pub async fn new() -> NovaResult<Self> {
        let config = EngineConfig::default();
        Self::with_config(config).await
    }

    /// Create a new Nova Engine with custom configuration
    pub async fn with_config(config: EngineConfig) -> NovaResult<Self> {
        info!("Initializing Nova Core Engine v{}", crate::VERSION);

        // Validate configuration
        config.validate()?;

        // Initialize subsystems
        let resource_manager = Arc::new(ResourceManager::new(
            &config.memory.heap_size,
            config.memory.gc_threshold,
        )?);

        let event_system = Arc::new(EventSystem::new(
            config.events.queue_size,
            config.events.priority_levels,
        ));

        let thread_engine = Arc::new(ThreadEngine::new(
            config.threading.worker_threads,
            config.threading.main_thread_affinity,
        )?);

        let plugin_manager = Arc::new(Mutex::new(PluginManager::new(
            config.plugins.directory.clone(),
            config.plugins.auto_load,
            config.plugins.hot_reload,
            crate::VERSION.to_string(),
        )));

        let engine = Self {
            config,
            resource_manager,
            event_system,
            thread_engine,
            plugin_manager,
            running: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            start_time: Instant::now(),
        };

        info!("Nova Core Engine initialized successfully");
        Ok(engine)
    }

    /// Create Nova Engine from configuration file
    pub async fn from_config(config_path: PathBuf) -> NovaResult<Self> {
        let config = EngineConfig::load_from_file(config_path)?;
        Self::with_config(config).await
    }

    /// Start the engine and run the main loop
    pub async fn run(&self) -> NovaResult<()> {
        info!("Starting Nova Core Engine...");

        self.running
            .store(true, std::sync::atomic::Ordering::SeqCst);

        // Initialize plugins
        {
            let plugin_manager = self.plugin_manager.lock().await;
            plugin_manager.auto_load_plugins().await?;
        }

        // Dispatch engine start event
        self.event_system.dispatch(EngineStartEvent)?;

        // Main engine loop
        self.main_loop().await?;

        // Dispatch engine stop event
        self.event_system.dispatch(EngineStopEvent)?;

        info!("Nova Core Engine stopped");
        Ok(())
    }

    /// Main engine loop
    async fn main_loop(&self) -> NovaResult<()> {
        let mut last_frame_time = Instant::now();
        let mut frame_count = 0u64;
        let target_fps = 60.0;
        let target_frame_time = Duration::from_secs_f64(1.0 / target_fps);

        info!("Entering main loop (target FPS: {})", target_fps);

        while self.running.load(std::sync::atomic::Ordering::SeqCst) {
            let frame_start = Instant::now();
            let delta_time = frame_start.duration_since(last_frame_time).as_secs_f64();

            // Update subsystems
            self.update(delta_time).await?;

            // Process events
            self.event_system.process_events()?;

            // Maintain target frame rate
            let frame_duration = frame_start.elapsed();
            if frame_duration < target_frame_time {
                let sleep_duration = target_frame_time - frame_duration;
                tokio::time::sleep(sleep_duration).await;
            }

            last_frame_time = frame_start;
            frame_count += 1;

            // Log performance stats every 5 seconds
            if frame_count % (target_fps as u64 * 5) == 0 {
                self.log_performance_stats(frame_count, delta_time);
            }

            // Run garbage collection periodically
            if frame_count % (target_fps as u64 * 30) == 0 {
                if let Err(e) = self.resource_manager.garbage_collect().await {
                    warn!("Garbage collection failed: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Update all engine subsystems
    async fn update(&self, delta_time: f64) -> NovaResult<()> {
        // Update plugins
        {
            let plugin_manager = self.plugin_manager.lock().await;
            plugin_manager.update_plugins(delta_time).await?;
        }

        // Submit background tasks
        self.submit_background_tasks()?;

        Ok(())
    }

    /// Submit background maintenance tasks
    fn submit_background_tasks(&self) -> NovaResult<()> {
        // Example: submit periodic resource cleanup task
        let _resource_manager = self.resource_manager.clone();
        self.thread_engine.submit_function(
            "resource_maintenance".to_string(),
            crate::threading::TaskPriority::Low,
            move || {
                // Perform lightweight maintenance
                Ok(())
            },
        )?;

        Ok(())
    }

    /// Log performance statistics
    fn log_performance_stats(&self, frame_count: u64, delta_time: f64) {
        let uptime = self.start_time.elapsed();
        let fps = 1.0 / delta_time;

        // Use tokio::spawn to handle the async call
        let resource_manager = self.resource_manager.clone();
        let thread_engine = self.thread_engine.clone();
        let event_system = self.event_system.clone();

        tokio::spawn(async move {
            let memory_stats = resource_manager.get_memory_stats().await;
            let threading_stats = thread_engine.get_stats();
            let event_stats = event_system.get_stats();

            log::info!(
                "Performance Stats - Frame: {}, FPS: {:.1}, Uptime: {:.1}s",
                frame_count,
                fps,
                uptime.as_secs_f64()
            );
            log::info!(
                "Memory: {:.1}MB used / {:.1}MB total, Resources: {}",
                memory_stats.total_used as f64 / 1024.0 / 1024.0,
                memory_stats.total_allocated as f64 / 1024.0 / 1024.0,
                memory_stats.resource_count
            );
            log::info!(
                "Threading: {} tasks submitted, {} completed, {} pending",
                threading_stats.total_tasks_submitted,
                threading_stats.total_tasks_completed,
                threading_stats.pending_tasks
            );
            log::info!(
                "Events: {} dispatched, {} handled, avg dispatch: {:.1}μs",
                event_stats.events_dispatched,
                event_stats.events_handled,
                event_stats.avg_dispatch_time_us
            );
        });
    }

    /// Stop the engine
    pub fn stop(&self) {
        info!("Stopping Nova Core Engine...");
        self.running
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if the engine is running
    pub fn is_running(&self) -> bool {
        self.running.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Get engine configuration
    pub fn config(&self) -> &EngineConfig {
        &self.config
    }

    /// Get resource manager
    pub fn resource_manager(&self) -> &Arc<ResourceManager> {
        &self.resource_manager
    }

    /// Get event system
    pub fn event_system(&self) -> &Arc<EventSystem> {
        &self.event_system
    }

    /// Get thread engine
    pub fn thread_engine(&self) -> &Arc<ThreadEngine> {
        &self.thread_engine
    }

    /// Get plugin manager
    pub fn plugin_manager(&self) -> &Arc<Mutex<PluginManager>> {
        &self.plugin_manager
    }

    /// Get engine uptime
    pub fn uptime(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Shutdown the engine gracefully
    pub async fn shutdown(&self) -> NovaResult<()> {
        info!("Shutting down Nova Core Engine...");

        self.stop();

        // Stop plugins
        {
            let plugin_manager = self.plugin_manager.lock().await;
            let plugin_names = plugin_manager.list_plugins().await;
            for plugin_name in plugin_names {
                if let Err(e) = plugin_manager.stop_plugin(&plugin_name).await {
                    error!("Failed to stop plugin {}: {}", plugin_name, e);
                }
            }
        }

        // Shutdown thread engine
        self.thread_engine.shutdown()?;

        // Clear event system
        self.event_system.clear();

        // Run final garbage collection
        self.resource_manager.garbage_collect().await?;

        let total_uptime = self.uptime();
        info!(
            "Nova Core Engine shutdown complete (uptime: {:.1}s)",
            total_uptime.as_secs_f64()
        );

        Ok(())
    }
}

impl Drop for NovaEngine {
    fn drop(&mut self) {
        if self.is_running() {
            warn!("Nova Engine dropped while still running - performing emergency shutdown");
            // Note: Can't call async shutdown in Drop, so we just stop
            self.stop();
        }
    }
}

/// Engine builder for custom engine configuration
pub struct EngineBuilder {
    config: EngineConfig,
}

impl EngineBuilder {
    /// Create a new engine builder
    pub fn new() -> Self {
        Self {
            config: EngineConfig::default(),
        }
    }

    /// Set engine name
    pub fn name(mut self, name: String) -> Self {
        self.config.engine.name = name;
        self
    }

    /// Set memory configuration
    pub fn memory(mut self, heap_size: String, pool_count: usize, gc_threshold: f32) -> Self {
        self.config.memory.heap_size = heap_size;
        self.config.memory.pool_count = pool_count;
        self.config.memory.gc_threshold = gc_threshold;
        self
    }

    /// Set threading configuration
    pub fn threading(mut self, worker_threads: usize, main_thread_affinity: Option<usize>) -> Self {
        self.config.threading.worker_threads = worker_threads;
        self.config.threading.main_thread_affinity = main_thread_affinity;
        self
    }

    /// Set event system configuration
    pub fn events(mut self, queue_size: usize, priority_levels: usize) -> Self {
        self.config.events.queue_size = queue_size;
        self.config.events.priority_levels = priority_levels;
        self
    }

    /// Set plugin configuration
    pub fn plugins(mut self, directory: PathBuf, auto_load: bool, hot_reload: bool) -> Self {
        self.config.plugins.directory = directory;
        self.config.plugins.auto_load = auto_load;
        self.config.plugins.hot_reload = hot_reload;
        self
    }

    /// Enable debug mode
    pub fn debug(mut self, debug: bool) -> Self {
        self.config.engine.debug = debug;
        self
    }

    /// Build the engine
    pub async fn build(self) -> NovaResult<NovaEngine> {
        NovaEngine::with_config(self.config).await
    }
}

impl Default for EngineBuilder {
    fn default() -> Self {
        Self::new()
    }
}
