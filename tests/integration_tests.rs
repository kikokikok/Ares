use nova_core::*;
use std::time::Duration;
use tokio::time::timeout;

#[tokio::test]
async fn test_engine_creation() {
    env_logger::init();
    
    let engine = NovaEngine::new().await;
    assert!(engine.is_ok());
}

#[tokio::test]
async fn test_engine_with_custom_config() {
    let config = EngineConfig {
        engine: EngineSettings {
            name: "Test Engine".to_string(),
            version: "1.0.0".to_string(),
            debug: true,
        },
        memory: MemoryConfig {
            heap_size: "256MB".to_string(),
            pool_count: 4,
            gc_threshold: 0.8,
        },
        threading: ThreadingConfig {
            worker_threads: 2,
            main_thread_affinity: None,
        },
        events: EventConfig {
            queue_size: 1000,
            priority_levels: 3,
        },
        plugins: PluginConfig {
            directory: std::path::PathBuf::from("test_plugins"),
            auto_load: false,
            hot_reload: false,
            enabled: vec![],
            disabled: vec![],
        },
    };
    
    let engine = NovaEngine::with_config(config).await;
    assert!(engine.is_ok());
}

#[tokio::test]
async fn test_resource_manager() {
    let resource_manager = ResourceManager::new("100MB", 0.8).unwrap();
    
    // Test creating a resource
    let texture = TextureResource {
        width: 256,
        height: 256,
        data: vec![0u8; 256 * 256 * 4],
        format: "RGBA8".to_string(),
    };
    
    let handle = resource_manager.create_resource(texture).unwrap();
    assert!(resource_manager.get_resource(&handle).is_ok());
    
    // Test memory stats
    let stats = resource_manager.get_memory_stats();
    assert!(stats.resource_count > 0);
    assert!(stats.total_used > 0);
}

#[tokio::test]
async fn test_event_system() {
    let event_system = EventSystem::new(1000, 5);
    
    // Register an event handler
    let handler = SimpleEventHandler::new(|_event: &EngineStartEvent| {
        println!("Engine started!");
        Ok(())
    });
    
    let handler_id = event_system.register_handler(handler);
    
    // Dispatch an event
    let result = event_system.dispatch(EngineStartEvent);
    assert!(result.is_ok());
    
    // Process events
    let result = event_system.process_events();
    assert!(result.is_ok());
    
    // Unregister handler
    let result = event_system.unregister_handler(handler_id);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_thread_engine() {
    let thread_engine = ThreadEngine::new(2, None).unwrap();
    
    // Submit a simple task
    let task_id = thread_engine.submit_function(
        "test_task".to_string(),
        TaskPriority::Normal,
        || {
            println!("Task executed!");
            Ok(())
        }
    ).unwrap();
    
    // Wait a bit for task execution
    tokio::time::sleep(Duration::from_millis(100)).await;
    
    let stats = thread_engine.get_stats();
    assert!(stats.total_tasks_submitted > 0);
}

#[tokio::test]
async fn test_plugin_manager() {
    let plugin_manager = PluginManager::new(
        std::path::PathBuf::from("test_plugins"),
        false,
        false,
        "1.0.0".to_string(),
    );
    
    // Test plugin list (should be empty initially)
    let plugins = plugin_manager.list_plugins();
    assert!(plugins.is_empty());
}

#[tokio::test]
async fn test_engine_builder() {
    let engine = EngineBuilder::new()
        .name("Test Engine".to_string())
        .memory("128MB".to_string(), 4, 0.7)
        .threading(2, None)
        .events(500, 3)
        .debug(true)
        .build()
        .await;
    
    assert!(engine.is_ok());
    let engine = engine.unwrap();
    assert_eq!(engine.config().engine.name, "Test Engine");
}

#[tokio::test]
async fn test_config_validation() {
    let mut config = EngineConfig::default();
    
    // Valid config should pass
    assert!(config.validate().is_ok());
    
    // Invalid configs should fail
    config.memory.pool_count = 0;
    assert!(config.validate().is_err());
    
    config.memory.pool_count = 8;
    config.memory.gc_threshold = 1.5;
    assert!(config.validate().is_err());
    
    config.memory.gc_threshold = 0.8;
    config.threading.worker_threads = 0;
    assert!(config.validate().is_err());
}

#[tokio::test]
async fn test_memory_size_parsing() {
    use nova_core::config::parse_memory_size;
    
    assert_eq!(parse_memory_size("1GB").unwrap(), 1024 * 1024 * 1024);
    assert_eq!(parse_memory_size("512MB").unwrap(), 512 * 1024 * 1024);
    assert_eq!(parse_memory_size("256KB").unwrap(), 256 * 1024);
    assert_eq!(parse_memory_size("1024B").unwrap(), 1024);
    assert_eq!(parse_memory_size("1024").unwrap(), 1024);
    
    assert!(parse_memory_size("invalid").is_err());
}

#[tokio::test]
async fn test_resource_serialization() {
    let texture = TextureResource {
        width: 128,
        height: 128,
        data: vec![255u8; 128 * 128 * 4],
        format: "RGBA8".to_string(),
    };
    
    let serialized = texture.serialize().unwrap();
    let deserialized = TextureResource::deserialize(&serialized).unwrap();
    
    assert_eq!(texture.width, deserialized.width);
    assert_eq!(texture.height, deserialized.height);
    assert_eq!(texture.format, deserialized.format);
    assert_eq!(texture.data, deserialized.data);
}

#[tokio::test]
async fn test_task_priority_ordering() {
    let task_low = ComputeTask::new("low".to_string(), 100);
    let task_high = ComputeTask::new("high".to_string(), 100);
    
    assert_eq!(task_low.priority(), TaskPriority::Normal);
    assert!(TaskPriority::High > TaskPriority::Normal);
    assert!(TaskPriority::Critical > TaskPriority::High);
}