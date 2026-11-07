mod version;

fn main() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    println!("cargo:rerun-if-env-changed=DSHIM_BUILD_VERSION");
    println!("cargo:rerun-if-changed=build");
    println!("cargo:rerun-if-changed=../../crates");
    println!("cargo:rerun-if-changed=../../Cargo.toml");
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    version::watch(&root);
    let version = std::env::var("DSHIM_BUILD_VERSION")
        .unwrap_or_else(|_| version::describe(&root, env!("CARGO_PKG_VERSION")));
    println!("cargo:rustc-env=DSHIM_VERSION={version}");
}
