use nova_core::*;
use log::{info, warn};
use std::time::Duration;
use rand::Rng;

/// Simple game entity
#[derive(Debug)]
struct GameEntity {
    id: u32,
    x: f32,
    y: f32,
    velocity_x: f32,
    velocity_y: f32,
    health: u32,
}

impl GameEntity {
    fn new(id: u32) -> Self {
        let mut rng = rand::thread_rng();
        Self {
            id,
            x: rng.gen_range(0.0..800.0),
            y: rng.gen_range(0.0..600.0),
            velocity_x: rng.gen_range(-50.0..50.0),
            velocity_y: rng.gen_range(-50.0..50.0),
            health: 100,
        }
    }
    
    fn update(&mut self, delta_time: f64) {
        self.x += self.velocity_x * delta_time as f32;
        self.y += self.velocity_y * delta_time as f32;
        
        // Bounce off screen edges
        if self.x < 0.0 || self.x > 800.0 {
            self.velocity_x = -self.velocity_x;
        }
        if self.y < 0.0 || self.y > 600.0 {
            self.velocity_y = -self.velocity_y;
        }
    }
}

/// Custom game events
#[derive(Debug)]
struct EntitySpawnEvent {
    entity_id: u32,
}

impl Event for EntitySpawnEvent {
    fn event_type(&self) -> &'static str {
        "entity_spawn"
    }
}

#[derive(Debug)]
struct EntityUpdateEvent {
    entity_count: usize,
}

impl Event for EntityUpdateEvent {
    fn event_type(&self) -> &'static str {
        "entity_update"
    }
}

/// Basic game implementation
struct BasicGame {
    entities: Vec<GameEntity>,
    next_entity_id: u32,
    engine: std::sync::Arc<NovaEngine>,
}

impl BasicGame {
    async fn new() -> NovaResult<Self> {
        info!("Creating basic game...");
        
        let engine = std::sync::Arc::new(
            EngineBuilder::new()
                .name("Basic Game".to_string())
                .memory("256MB".to_string(), 8, 0.7)
                .threading(4, None)
                .events(2000, 4)
                .debug(true)
                .build()
                .await?
        );
        
        let mut game = Self {
            entities: Vec::new(),
            next_entity_id: 1,
            engine,
        };
        
        // Register event handlers
        game.register_event_handlers();
        
        // Spawn initial entities
        game.spawn_entities(10).await?;
        
        Ok(game)
    }
    
    fn register_event_handlers(&self) {
        // Handle entity spawn events
        let spawn_handler = SimpleEventHandler::new(|event: &EntitySpawnEvent| {
            info!("Entity {} spawned", event.entity_id);
            Ok(())
        });
        self.engine.event_system().register_handler(spawn_handler);
        
        // Handle entity update events
        let update_handler = SimpleEventHandler::new(|event: &EntityUpdateEvent| {
            if event.entity_count % 100 == 0 {
                info!("Updated {} entities", event.entity_count);
            }
            Ok(())
        });
        self.engine.event_system().register_handler(update_handler);
    }
    
    async fn spawn_entities(&mut self, count: u32) -> NovaResult<()> {
        for _ in 0..count {
            let entity = GameEntity::new(self.next_entity_id);
            
            // Dispatch spawn event
            self.engine.event_system().dispatch(EntitySpawnEvent {
                entity_id: entity.id,
            })?;
            
            self.entities.push(entity);
            self.next_entity_id += 1;
        }
        
        info!("Spawned {} entities", count);
        Ok(())
    }
    
    async fn update_entities(&mut self, delta_time: f64) -> NovaResult<()> {
        // Update entities in parallel using the thread engine
        let entity_count = self.entities.len();
        
        // For this example, we'll update entities sequentially
        // In a real game, you'd batch them and use the thread engine
        for entity in &mut self.entities {
            entity.update(delta_time);
        }
        
        // Dispatch update event
        self.engine.event_system().dispatch(EntityUpdateEvent {
            entity_count,
        })?;
        
        Ok(())
    }
    
    async fn simulate_game_logic(&mut self) -> NovaResult<()> {
        let mut frame_count = 0u64;
        let mut last_spawn_time = std::time::Instant::now();
        
        // Game simulation loop
        loop {
            let frame_start = std::time::Instant::now();
            let delta_time = 1.0 / 60.0; // 60 FPS
            
            // Update game entities
            self.update_entities(delta_time).await?;
            
            // Spawn new entities occasionally
            if last_spawn_time.elapsed() > Duration::from_secs(5) {
                self.spawn_entities(2).await?;
                last_spawn_time = std::time::Instant::now();
                
                // Remove entities if we have too many
                if self.entities.len() > 50 {
                    let to_remove = self.entities.len() - 40;
                    self.entities.drain(0..to_remove);
                    info!("Removed {} entities to maintain performance", to_remove);
                }
            }
            
            // Submit background tasks
            self.submit_background_tasks(frame_count)?;
            
            frame_count += 1;
            
            // Log stats every 5 seconds
            if frame_count % 300 == 0 {
                self.log_game_stats();
            }
            
            // Maintain 60 FPS
            let frame_time = frame_start.elapsed();
            let target_frame_time = Duration::from_millis(16); // ~60 FPS
            if frame_time < target_frame_time {
                tokio::time::sleep(target_frame_time - frame_time).await;
            }
            
            // Break after a while for this example
            if frame_count > 1800 { // Run for 30 seconds at 60 FPS
                break;
            }
        }
        
        Ok(())
    }
    
    fn submit_background_tasks(&self, frame_count: u64) -> NovaResult<()> {
        // Submit periodic maintenance tasks
        if frame_count % 600 == 0 { // Every 10 seconds
            let entity_count = self.entities.len();
            self.engine.thread_engine().submit_function(
                "game_maintenance".to_string(),
                TaskPriority::Low,
                move || {
                    info!("Running game maintenance for {} entities", entity_count);
                    // Simulate some maintenance work
                    std::thread::sleep(Duration::from_millis(10));
                    Ok(())
                }
            )?;
        }
        
        Ok(())
    }
    
    fn log_game_stats(&self) {
        let memory_stats = self.engine.resource_manager().get_memory_stats();
        let threading_stats = self.engine.thread_engine().get_stats();
        let event_stats = self.engine.event_system().get_stats();
        
        info!("Game Stats:");
        info!("  Entities: {}", self.entities.len());
        info!("  Memory: {:.1}MB used", memory_stats.total_used as f64 / 1024.0 / 1024.0);
        info!("  Tasks: {} completed, {} pending", 
              threading_stats.total_tasks_completed, threading_stats.pending_tasks);
        info!("  Events: {} dispatched, {} handled", 
              event_stats.events_dispatched, event_stats.events_handled);
    }
    
    async fn run(mut self) -> NovaResult<()> {
        info!("Starting basic game simulation...");
        
        // Start the engine in the background
        let engine = self.engine.clone();
        let engine_task = tokio::spawn(async move {
            if let Err(e) = engine.run().await {
                warn!("Engine error: {}", e);
            }
        });
        
        // Run game simulation
        let game_task = self.simulate_game_logic();
        
        // Wait for either task to complete
        tokio::select! {
            result = engine_task => {
                if let Err(e) = result {
                    warn!("Engine task failed: {}", e);
                }
            }
            result = game_task => {
                if let Err(e) = result {
                    warn!("Game simulation failed: {}", e);
                }
            }
        }
        
        // Shutdown gracefully
        self.engine.shutdown().await?;
        
        info!("Basic game simulation completed");
        Ok(())
    }
}

#[tokio::main]
async fn main() -> NovaResult<()> {
    env_logger::init();
    
    info!("Starting Nova Core Engine Basic Game Example");
    
    let game = BasicGame::new().await?;
    game.run().await?;
    
    Ok(())
}