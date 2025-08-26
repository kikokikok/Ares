use std::any::Any;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;
use crate::config::parse_memory_size;
use crate::error::{NovaError, NovaResult};
use crate::protocol::{ProtoSerialize, resources as proto_resources};

/// Resource trait that all resources must implement
/// Now using high-performance protobuf serialization for blazing fast performance
pub trait Resource: Send + Sync + 'static {
    /// Resource type name
    fn resource_type(&self) -> &'static str;
    
    /// Resource size in bytes
    fn size(&self) -> usize;
    
    /// Serialize resource data using high-performance protobuf
    fn serialize(&self) -> NovaResult<Vec<u8>>;
    
    /// Deserialize resource data using high-performance protobuf
    fn deserialize(data: &[u8]) -> NovaResult<Self>
    where
        Self: Sized;
}

/// Resource handle for safe resource access
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceHandle<T> {
    id: Uuid,
    _phantom: std::marker::PhantomData<T>,
}

impl<T> ResourceHandle<T> {
    fn new(id: Uuid) -> Self {
        Self {
            id,
            _phantom: std::marker::PhantomData,
        }
    }
    
    pub fn id(&self) -> Uuid {
        self.id
    }
}

/// Resource entry in the resource manager
struct ResourceEntry {
    resource: Box<dyn Any + Send + Sync>,
    type_name: &'static str,
    size: usize,
    ref_count: usize,
    last_accessed: std::time::Instant,
    dependencies: Vec<Uuid>,
}

/// Memory statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    pub total_allocated: usize,
    pub total_used: usize,
    pub resource_count: usize,
    pub cache_hits: u64,
    pub cache_misses: u64,
    pub gc_runs: u64,
}

/// Resource loader trait for loading resources from different sources
pub trait ResourceLoader<T: Resource>: Send + Sync {
    fn load(&self, path: &Path) -> NovaResult<T>;
    fn can_load(&self, path: &Path) -> bool;
}

/// Resource manager for centralized resource allocation and lifecycle management
pub struct ResourceManager {
    /// Resource storage
    resources: Arc<RwLock<HashMap<Uuid, ResourceEntry>>>,
    /// Resource loaders by file extension
    loaders: Arc<RwLock<HashMap<String, Box<dyn Any + Send + Sync>>>>,
    /// Memory configuration
    max_memory: usize,
    gc_threshold: f32,
    /// Statistics
    stats: Arc<RwLock<MemoryStats>>,
    /// Resource caches
    cache: Arc<RwLock<HashMap<PathBuf, Uuid>>>,
}

impl ResourceManager {
    /// Create a new resource manager
    pub fn new(max_memory_str: &str, gc_threshold: f32) -> NovaResult<Self> {
        let max_memory = parse_memory_size(max_memory_str)?;
        
        Ok(Self {
            resources: Arc::new(RwLock::new(HashMap::new())),
            loaders: Arc::new(RwLock::new(HashMap::new())),
            max_memory,
            gc_threshold,
            stats: Arc::new(RwLock::new(MemoryStats {
                total_allocated: max_memory,
                total_used: 0,
                resource_count: 0,
                cache_hits: 0,
                cache_misses: 0,
                gc_runs: 0,
            })),
            cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }
    
    /// Register a resource loader
    pub async fn register_loader<T, L>(&self, extension: &str, loader: L)
    where
        T: Resource,
        L: ResourceLoader<T> + 'static,
    {
        let mut loaders = self.loaders.write().await;
        loaders.insert(extension.to_lowercase(), Box::new(loader));
        log::debug!("Registered resource loader for extension: {}", extension);
    }
    
    /// Load a resource from file
    pub fn load_resource<T: Resource>(&self, path: &Path) -> NovaResult<ResourceHandle<T>> {
        // Check cache first
        {
            let cache = self.cache.read();
            if let Some(&resource_id) = cache.get(path) {
                let mut stats = self.stats.write();
                stats.cache_hits += 1;
                return Ok(ResourceHandle::new(resource_id));
            }
        }
        
        // Get file extension
        let extension = path.extension()
            .and_then(|ext| ext.to_str())
            .ok_or_else(|| NovaError::resource("No file extension found"))?
            .to_lowercase();
        
        // Find appropriate loader
        let loaders = self.loaders.read();
        let loader_any = loaders.get(&extension)
            .ok_or_else(|| NovaError::resource(format!("No loader registered for extension: {}", extension)))?;
        
        let loader = loader_any.downcast_ref::<Box<dyn ResourceLoader<T>>>()
            .ok_or_else(|| NovaError::resource("Loader type mismatch"))?;
        
        // Load the resource
        let resource = loader.load(path)?;
        let resource_size = resource.size();
        
        // Check memory limits
        self.check_memory_limits(resource_size)?;
        
        let resource_id = Uuid::new_v4();
        let entry = ResourceEntry {
            resource: Box::new(resource),
            type_name: std::any::type_name::<T>(),
            size: resource_size,
            ref_count: 1,
            last_accessed: std::time::Instant::now(),
            dependencies: Vec::new(),
        };
        
        // Store resource
        {
            let mut resources = self.resources.write();
            resources.insert(resource_id, entry);
        }
        
        // Update cache
        {
            let mut cache = self.cache.write();
            cache.insert(path.to_path_buf(), resource_id);
        }
        
        // Update statistics
        {
            let mut stats = self.stats.write();
            stats.total_used += resource_size;
            stats.resource_count += 1;
            stats.cache_misses += 1;
        }
        
        log::debug!("Loaded resource: {:?} (size: {} bytes)", path, resource_size);
        Ok(ResourceHandle::new(resource_id))
    }
    
    /// Create a resource directly
    pub fn create_resource<T: Resource>(&self, resource: T) -> NovaResult<ResourceHandle<T>> {
        let resource_size = resource.size();
        self.check_memory_limits(resource_size)?;
        
        let resource_id = Uuid::new_v4();
        let entry = ResourceEntry {
            resource: Box::new(resource),
            type_name: std::any::type_name::<T>(),
            size: resource_size,
            ref_count: 1,
            last_accessed: std::time::Instant::now(),
            dependencies: Vec::new(),
        };
        
        {
            let mut resources = self.resources.write();
            resources.insert(resource_id, entry);
        }
        
        {
            let mut stats = self.stats.write();
            stats.total_used += resource_size;
            stats.resource_count += 1;
        }
        
        log::debug!("Created resource with ID: {} (size: {} bytes)", resource_id, resource_size);
        Ok(ResourceHandle::new(resource_id))
    }
    
    /// Get a resource by handle
    pub fn get_resource<T: Resource>(&self, handle: &ResourceHandle<T>) -> NovaResult<Arc<T>> {
        let mut resources = self.resources.write();
        
        if let Some(entry) = resources.get_mut(&handle.id) {
            entry.last_accessed = std::time::Instant::now();
            entry.ref_count += 1;
            
            let resource = entry.resource.downcast_ref::<T>()
                .ok_or_else(|| NovaError::resource("Resource type mismatch"))?;
            
            // Clone the resource data for now - in a real implementation,
            // this would return a proper shared reference
            Ok(Arc::new(unsafe { std::ptr::read(resource) }))
        } else {
            Err(NovaError::resource("Resource not found"))
        }
    }
    
    /// Unload a resource
    pub fn unload_resource<T>(&self, handle: ResourceHandle<T>) -> NovaResult<()> {
        let mut resources = self.resources.write();
        
        if let Some(entry) = resources.get_mut(&handle.id) {
            entry.ref_count = entry.ref_count.saturating_sub(1);
            
            if entry.ref_count == 0 {
                let size = entry.size;
                resources.remove(&handle.id);
                
                // Update statistics
                let mut stats = self.stats.write();
                stats.total_used -= size;
                stats.resource_count -= 1;
                
                log::debug!("Unloaded resource: {}", handle.id);
            }
            
            Ok(())
        } else {
            Err(NovaError::resource("Resource not found"))
        }
    }
    
    /// Run garbage collection
    pub fn garbage_collect(&self) -> NovaResult<()> {
        let mut resources = self.resources.write();
        let mut cache = self.cache.write();
        let current_time = std::time::Instant::now();
        
        let mut to_remove = Vec::new();
        let mut freed_memory = 0;
        
        for (id, entry) in resources.iter() {
            // Remove resources that haven't been accessed in a while and have no references
            if entry.ref_count == 0 && current_time.duration_since(entry.last_accessed).as_secs() > 60 {
                to_remove.push(*id);
                freed_memory += entry.size;
            }
        }
        
        let removed_count = to_remove.len();
        
        for id in to_remove {
            resources.remove(&id);
            
            // Remove from cache as well
            cache.retain(|_, &mut cached_id| cached_id != id);
        }
        
        if freed_memory > 0 {
            let mut stats = self.stats.write();
            stats.total_used -= freed_memory;
            stats.resource_count = resources.len();
            stats.gc_runs += 1;
            
            log::info!("Garbage collection freed {} bytes, {} resources removed", 
                      freed_memory, removed_count);
        }
        
        Ok(())
    }
    
    /// Get memory statistics
    pub fn get_memory_stats(&self) -> MemoryStats {
        self.stats.read().clone()
    }
    
    /// Check if we're approaching memory limits and trigger GC if needed
    fn check_memory_limits(&self, additional_size: usize) -> NovaResult<()> {
        let stats = self.stats.read();
        let projected_usage = stats.total_used + additional_size;
        let usage_ratio = projected_usage as f32 / self.max_memory as f32;
        
        if usage_ratio > self.gc_threshold {
            drop(stats); // Release the lock before GC
            log::warn!("Memory usage at {:.1}%, triggering garbage collection", usage_ratio * 100.0);
            self.garbage_collect()?;
            
            // Check again after GC
            let stats = self.stats.read();
            let new_projected_usage = stats.total_used + additional_size;
            if new_projected_usage > self.max_memory {
                return Err(NovaError::resource("Out of memory"));
            }
        }
        
        Ok(())
    }
}

// High-performance resource implementations using Protocol Buffers

#[derive(Debug, Clone)]
pub struct TextureResource {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
    pub format: String,
}

impl ProtoSerialize for TextureResource {
    type Proto = proto_resources::TextureResource;
    
    fn to_proto(&self) -> Self::Proto {
        // Convert to protobuf format for optimal wire performance
        let format = match self.format.as_str() {
            "RGBA8" => proto_resources::TextureFormat::Rgba8,
            "RGB8" => proto_resources::TextureFormat::Rgb8,
            "RGBA16F" => proto_resources::TextureFormat::Rgba16f,
            "RGBA32F" => proto_resources::TextureFormat::Rgba32f,
            "DXT1" => proto_resources::TextureFormat::Dxt1,
            "DXT5" => proto_resources::TextureFormat::Dxt5,
            "BC7" => proto_resources::TextureFormat::Bc7,
            _ => proto_resources::TextureFormat::Unspecified,
        };
        
        proto_resources::TextureResource {
            width: self.width,
            height: self.height,
            format: format as i32,
            mip_levels: 1,
            data: vec![proto_resources::TextureData {
                level: 0,
                width: self.width,
                height: self.height,
                data: self.data.clone().into(),
            }],
            flags: Some(proto_resources::TextureFlags {
                is_srgb: false,
                generate_mipmaps: false,
                is_cubemap: false,
                is_array: false,
            }),
        }
    }
    
    fn from_proto(proto: Self::Proto) -> NovaResult<Self> {
        use std::convert::TryFrom;
        let format = match proto_resources::TextureFormat::try_from(proto.format) {
            Ok(proto_resources::TextureFormat::Rgba8) => "RGBA8",
            Ok(proto_resources::TextureFormat::Rgb8) => "RGB8",
            Ok(proto_resources::TextureFormat::Rgba16f) => "RGBA16F",
            Ok(proto_resources::TextureFormat::Rgba32f) => "RGBA32F",
            Ok(proto_resources::TextureFormat::Dxt1) => "DXT1",
            Ok(proto_resources::TextureFormat::Dxt5) => "DXT5",
            Ok(proto_resources::TextureFormat::Bc7) => "BC7",
            _ => "RGBA8", // Default fallback
        }.to_string();
        
        let data = if let Some(texture_data) = proto.data.first() {
            texture_data.data.to_vec()
        } else {
            vec![]
        };
        
        Ok(TextureResource {
            width: proto.width,
            height: proto.height,
            data,
            format,
        })
    }
}

impl Resource for TextureResource {
    fn resource_type(&self) -> &'static str {
        "Texture"
    }
    
    fn size(&self) -> usize {
        self.data.len() + std::mem::size_of::<Self>()
    }
    
    fn serialize(&self) -> NovaResult<Vec<u8>> {
        // Use protobuf for blazing fast serialization
        self.serialize_proto()
    }
    
    fn deserialize(data: &[u8]) -> NovaResult<Self> {
        // Use protobuf for blazing fast deserialization
        Self::deserialize_proto(data)
    }
}

#[derive(Debug, Clone)]
pub struct AudioResource {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

impl ProtoSerialize for AudioResource {
    type Proto = proto_resources::AudioResource;
    
    fn to_proto(&self) -> Self::Proto {
        // Convert samples to bytes for efficient wire transfer
        let mut sample_bytes = Vec::with_capacity(self.samples.len() * 4);
        for sample in &self.samples {
            sample_bytes.extend_from_slice(&sample.to_le_bytes());
        }
        
        proto_resources::AudioResource {
            format: proto_resources::AudioFormat::Wav as i32,
            sample_rate: self.sample_rate,
            channels: self.channels as u32,
            bit_depth: 32, // f32 samples
            data: sample_bytes.into(),
            duration: self.samples.len() as f32 / (self.sample_rate * self.channels as u32) as f32,
            is_looping: false,
        }
    }
    
    fn from_proto(proto: Self::Proto) -> NovaResult<Self> {
        // Convert bytes back to f32 samples
        let mut samples = Vec::with_capacity(proto.data.len() / 4);
        for chunk in proto.data.chunks(4) {
            if chunk.len() == 4 {
                let sample = f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
                samples.push(sample);
            }
        }
        
        Ok(AudioResource {
            sample_rate: proto.sample_rate,
            channels: proto.channels as u16,
            samples,
        })
    }
}

impl Resource for AudioResource {
    fn resource_type(&self) -> &'static str {
        "Audio"
    }
    
    fn size(&self) -> usize {
        self.samples.len() * std::mem::size_of::<f32>() + std::mem::size_of::<Self>()
    }
    
    fn serialize(&self) -> NovaResult<Vec<u8>> {
        // Use protobuf for blazing fast serialization
        self.serialize_proto()
    }
    
    fn deserialize(data: &[u8]) -> NovaResult<Self> {
        // Use protobuf for blazing fast deserialization
        Self::deserialize_proto(data)
    }
}

// Sample loader implementation
pub struct TextureLoader;

impl ResourceLoader<TextureResource> for TextureLoader {
    fn load(&self, path: &Path) -> NovaResult<TextureResource> {
        // Simplified texture loading - in a real implementation, 
        // this would use an image loading library
        let data = std::fs::read(path)?;
        
        Ok(TextureResource {
            width: 256,  // Placeholder values
            height: 256,
            data,
            format: "RGBA8".to_string(),
        })
    }
    
    fn can_load(&self, path: &Path) -> bool {
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            matches!(ext.to_lowercase().as_str(), "png" | "jpg" | "jpeg" | "bmp")
        } else {
            false
        }
    }
}