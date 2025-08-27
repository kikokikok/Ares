use nova_core::*;
use log::info;

#[tokio::main]
async fn main() -> NovaResult<()> {
    env_logger::init();
    
    info!("Starting {{project_name}}...");
    
    let engine = EngineBuilder::new()
        .name("{{project_name}}".to_string())
        .debug(true)
        .build()
        .await?;
    
    engine.run().await?;
    
    Ok(())
}