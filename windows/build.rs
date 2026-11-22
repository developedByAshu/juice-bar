//! Builds the app icon, name and version into the program.

fn main() {
    println!("cargo:rerun-if-changed=assets/juicebar.rc");
    println!("cargo:rerun-if-changed=assets/juicebar.ico");
    println!("cargo:rerun-if-changed=assets/juicebar.manifest");
    #[cfg(windows)]
    {
        let mut macros: Vec<String> = ["MAJOR", "MINOR", "PATCH"]
            .iter()
            .map(|part| {
                let value = std::env::var(format!("CARGO_PKG_VERSION_{part}")).expect("Cargo always sets the version");
                format!("VERSION_{part}={value}")
            })
            .collect();
        // The icon is drawn by `cargo run --example make_icon`; a fresh checkout without it still builds.
        if std::path::Path::new("assets/juicebar.ico").exists() {
            macros.push("HAS_ICON=1".to_string());
        }
        embed_resource::compile("assets/juicebar.rc", macros)
            .manifest_required()
            .expect("the app icon and details could not be built into the program");
    }
}
