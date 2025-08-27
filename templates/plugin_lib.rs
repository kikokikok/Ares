use nova_core::*;
use async_trait::async_trait;

pub struct {{plugin_name}}Plugin;

#[async_trait]
impl Plugin for {{plugin_name}}Plugin {
    fn name(&self) -> &str {
        "{{plugin_name}}"
    }
    
    fn version(&self) -> &str {
        "1.0.0"
    }
    
    fn description(&self) -> &str {
        "A sample Nova Core Engine plugin"
    }
    
    async fn initialize(&mut self, context: &PluginContext) -> NovaResult<()> {
        log::info!("Initializing {{plugin_name}} plugin");
        Ok(())
    }
    
    async fn update(&mut self, delta_time: f64) -> NovaResult<()> {
        // Plugin update logic here
        Ok(())
    }
    
    async fn shutdown(&mut self) -> NovaResult<()> {
        log::info!("Shutting down {{plugin_name}} plugin");
        Ok(())
    }
}