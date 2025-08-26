use std::io::Result;

fn main() -> Result<()> {
    // Configure prost build for nova-core
    prost_build::Config::new()
        .compile_protos(
            &[
                "../proto/nova_protocol.proto",
                "../proto/nova_resources.proto",
            ],
            &["../proto/"],
        )?;
    
    println!("cargo:rerun-if-changed=../proto/nova_protocol.proto");
    println!("cargo:rerun-if-changed=../proto/nova_resources.proto");
    
    Ok(())
}