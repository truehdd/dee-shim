use std::env;

fn main() {
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let arch = env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
    let enabled = |name| env::var_os(name).is_some();
    let ddp = enabled("CARGO_FEATURE_DDP");
    let joc = enabled("CARGO_FEATURE_DDPJOC");
    let dthd = enabled("CARGO_FEATURE_DTHD");
    let versions = [("CARGO_FEATURE_DEE_6_7", "6.7")]
        .into_iter()
        .filter_map(|(feature, version)| {
            if env::var_os(feature).is_none() || arch != "x86_64" {
                return None;
            }
            let available = match (os.as_str(), version) {
                ("linux" | "macos" | "windows", "6.7") => ddp || joc || dthd,
                _ => false,
            };
            available.then_some(version)
        })
        .collect::<Vec<_>>();
    let label = if versions.is_empty() {
        "none".to_owned()
    } else {
        versions.join(", ")
    };
    println!("cargo:rustc-env=DSHIM_DEE_VERSIONS={label}");
}
