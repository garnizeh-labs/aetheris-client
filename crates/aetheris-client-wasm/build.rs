fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("cargo:rerun-if-changed=proto/auth.proto");
    println!("cargo:rerun-if-changed=build.rs");
    // Generate only the prost message types (AuthRequest / AuthResponse).
    // We deliberately skip client and server stubs because:
    //   - The generated client stub's connect() method references tonic::transport::Channel,
    //     which requires hyper → mio — both incompatible with wasm32-unknown-unknown.
    //   - The WASM transport is provided by tonic-web-wasm-client instead.
    //   - A hand-written, WASM-compatible AuthServiceClient lives in src/auth.rs.
    tonic_prost_build::configure()
        .build_client(false)
        .build_server(false)
        .compile_protos(&["proto/auth.proto"], &["proto"])?;
    Ok(())
}
