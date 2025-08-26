use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;
use parking_lot::RwLock;
use uuid::Uuid;
use crate::error::{NovaError, NovaResult};

/// Event trait that all events must implement
pub trait Event: Send + Sync + 'static {
    fn event_type(&self) -> &'static str;
}

/// Event handler trait
pub trait EventHandler<T: Event>: Send + Sync {
    fn handle(&self, event: &T) -> NovaResult<()>;
}

/// Event priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPriority {
    Low = 0,
    Normal = 1,
    High = 2,
    Critical = 3,
    Immediate = 4,
}

/// Event wrapper for internal handling
struct EventWrapper {
    event: Box<dyn Any + Send + Sync>,
    type_id: TypeId,
    type_name: &'static str,
    priority: EventPriority,
    timestamp: std::time::Instant,
}

/// Handler wrapper for type erasure
type HandlerFn = Box<dyn Fn(&dyn Any) -> NovaResult<()> + Send + Sync>;

struct HandlerWrapper {
    id: Uuid,
    handler: HandlerFn,
    type_id: TypeId,
}

/// High-performance event system with priority queues and type safety
pub struct EventSystem {
    /// Event queues by priority
    queues: Arc<RwLock<Vec<Vec<EventWrapper>>>>,
    /// Registered event handlers
    handlers: Arc<RwLock<HashMap<TypeId, Vec<HandlerWrapper>>>>,
    /// Configuration
    max_queue_size: usize,
    priority_levels: usize,
    /// Statistics
    stats: Arc<RwLock<EventStats>>,
}

#[derive(Debug, Default, Clone)]
pub struct EventStats {
    pub events_dispatched: u64,
    pub events_handled: u64,
    pub events_dropped: u64,
    pub avg_dispatch_time_us: f64,
}

impl EventSystem {
    /// Create a new event system
    pub fn new(max_queue_size: usize, priority_levels: usize) -> Self {
        let queues = (0..priority_levels).map(|_| Vec::new()).collect();
        
        Self {
            queues: Arc::new(RwLock::new(queues)),
            handlers: Arc::new(RwLock::new(HashMap::new())),
            max_queue_size,
            priority_levels,
            stats: Arc::new(RwLock::new(EventStats::default())),
        }
    }
    
    /// Register an event handler
    pub fn register_handler<T, H>(&self, handler: H) -> Uuid
    where
        T: Event,
        H: EventHandler<T> + 'static,
    {
        let id = Uuid::new_v4();
        let type_id = TypeId::of::<T>();
        
        let handler_fn: HandlerFn = Box::new(move |event_any| {
            let event = event_any.downcast_ref::<T>()
                .ok_or_else(|| NovaError::event("Failed to downcast event"))?;
            handler.handle(event)
        });
        
        let wrapper = HandlerWrapper {
            id,
            handler: handler_fn,
            type_id,
        };
        
        let mut handlers = self.handlers.write();
        handlers.entry(type_id).or_insert_with(Vec::new).push(wrapper);
        
        log::debug!("Registered event handler {} for type {:?}", id, std::any::type_name::<T>());
        id
    }
    
    /// Unregister an event handler
    pub fn unregister_handler(&self, handler_id: Uuid) -> NovaResult<()> {
        let mut handlers = self.handlers.write();
        
        for handlers_list in handlers.values_mut() {
            if let Some(pos) = handlers_list.iter().position(|h| h.id == handler_id) {
                handlers_list.remove(pos);
                log::debug!("Unregistered event handler {}", handler_id);
                return Ok(());
            }
        }
        
        Err(NovaError::event(format!("Handler {} not found", handler_id)))
    }
    
    /// Dispatch an event with default priority
    pub fn dispatch<T: Event>(&self, event: T) -> NovaResult<()> {
        self.dispatch_with_priority(event, EventPriority::Normal)
    }
    
    /// Dispatch an event with specified priority
    pub fn dispatch_with_priority<T: Event>(&self, event: T, priority: EventPriority) -> NovaResult<()> {
        let priority_index = priority as usize;
        if priority_index >= self.priority_levels {
            return Err(NovaError::event("Invalid priority level"));
        }
        
        let wrapper = EventWrapper {
            event: Box::new(event),
            type_id: TypeId::of::<T>(),
            type_name: std::any::type_name::<T>(),
            priority,
            timestamp: std::time::Instant::now(),
        };
        
        let mut queues = self.queues.write();
        let queue = &mut queues[priority_index];
        
        if queue.len() >= self.max_queue_size {
            // Drop oldest event of same priority
            queue.remove(0);
            let mut stats = self.stats.write();
            stats.events_dropped += 1;
            log::warn!("Event queue full, dropping oldest event");
        }
        
        queue.push(wrapper);
        log::trace!("Dispatched event of type {}", std::any::type_name::<T>());
        
        Ok(())
    }
    
    /// Process events in priority order
    pub fn process_events(&self) -> NovaResult<()> {
        let start_time = std::time::Instant::now();
        let mut events_processed = 0u64;
        
        // Process events from highest to lowest priority
        for priority in (0..self.priority_levels).rev() {
            let events = {
                let mut queues = self.queues.write();
                std::mem::take(&mut queues[priority])
            };
            
            for event_wrapper in events {
                self.handle_event(event_wrapper)?;
                events_processed += 1;
            }
        }
        
        // Update statistics
        if events_processed > 0 {
            let elapsed = start_time.elapsed();
            let mut stats = self.stats.write();
            stats.events_dispatched += events_processed;
            stats.avg_dispatch_time_us = 
                (stats.avg_dispatch_time_us + elapsed.as_micros() as f64) / 2.0;
        }
        
        Ok(())
    }
    
    /// Handle a single event
    fn handle_event(&self, event_wrapper: EventWrapper) -> NovaResult<()> {
        let handlers = self.handlers.read();
        
        if let Some(handlers_list) = handlers.get(&event_wrapper.type_id) {
            for handler_wrapper in handlers_list {
                match (handler_wrapper.handler)(&*event_wrapper.event) {
                    Ok(()) => {
                        let mut stats = self.stats.write();
                        stats.events_handled += 1;
                    }
                    Err(e) => {
                        log::error!("Event handler failed: {}", e);
                        // Continue processing other handlers
                    }
                }
            }
        }
        
        Ok(())
    }
    
    /// Get event system statistics
    pub fn get_stats(&self) -> EventStats {
        (*self.stats.read()).clone()
    }
    
    /// Clear all queues and reset statistics
    pub fn clear(&self) {
        let mut queues = self.queues.write();
        for queue in queues.iter_mut() {
            queue.clear();
        }
        
        let mut stats = self.stats.write();
        *stats = EventStats::default();
        
        log::info!("Event system cleared");
    }
}

/// Simple event handler implementation
pub struct SimpleEventHandler<T, F> 
where
    T: Event,
    F: Fn(&T) -> NovaResult<()> + Send + Sync,
{
    handler: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<T, F> SimpleEventHandler<T, F>
where
    T: Event,
    F: Fn(&T) -> NovaResult<()> + Send + Sync,
{
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<T, F> EventHandler<T> for SimpleEventHandler<T, F>
where
    T: Event,
    F: Fn(&T) -> NovaResult<()> + Send + Sync,
{
    fn handle(&self, event: &T) -> NovaResult<()> {
        (self.handler)(event)
    }
}

// Common event types
#[derive(Debug)]
pub struct EngineStartEvent;

impl Event for EngineStartEvent {
    fn event_type(&self) -> &'static str {
        "engine_start"
    }
}

#[derive(Debug)]
pub struct EngineStopEvent;

impl Event for EngineStopEvent {
    fn event_type(&self) -> &'static str {
        "engine_stop"
    }
}

#[derive(Debug)]
pub struct PluginLoadedEvent {
    pub plugin_name: String,
}

impl Event for PluginLoadedEvent {
    fn event_type(&self) -> &'static str {
        "plugin_loaded"
    }
}