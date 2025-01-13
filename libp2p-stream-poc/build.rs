fn main() {
    /*
    csbindgen::Builder::default()
        .input_extern_file("src/lib.rs")
        .csharp_dll_name("example")
        .generate_csharp_file("bindings/dotnet/NativeMethods.g.cs")
        .unwrap();

    cbindgen::Builder::new()
        .with_crate(".")
        .with_config(cbindgen::Config::from_file("cbindgen.toml").unwrap())
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file("bindings/c/libp2p_stream_poc.h");
    // */
}
