use std::io::Result;

fn main() -> Result<()> {
    // Configure prost build for nova-server
    prost_build::Config::new()
        .compile_protos(
            &["../proto/nova_protocol.proto"],
            &["../proto/"],
        )?;
    
    println!("cargo:rerun-if-changed=../proto/nova_protocol.proto");
    
    Ok(())
}