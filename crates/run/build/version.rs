use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn describe(root: &Path, fallback: &str) -> String {
    let unknown = || format!("v{fallback}+unknown");
    let Some(top) = git(root, &["rev-parse", "--show-toplevel"]) else {
        return unknown();
    };
    if root.canonicalize().ok() != Path::new(&top).canonicalize().ok() {
        return unknown();
    }
    let Some(hash) = git(root, &["rev-parse", "--short=7", "HEAD"]) else {
        return unknown();
    };
    let release =
        git(root, &["describe", "--long", "--match", "v[0-9]*", "HEAD"]).and_then(|label| {
            let (prefix, _) = label.rsplit_once("-g")?;
            let (tag, count) = prefix.rsplit_once('-')?;
            let parsed = semver::Version::parse(tag.strip_prefix('v')?).ok()?;
            if tag != format!("v{parsed}") || !parsed.build.is_empty() {
                return None;
            }
            Some((tag.to_owned(), count.parse::<u64>().ok()?))
        });
    let (tag, count) = release.unwrap_or_else(|| {
        let count = git(root, &["rev-list", "--count", "HEAD"])
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        (format!("v{fallback}"), count)
    });
    // The hash and any count are build metadata (after `+`), so the label keeps the tag's
    // precedence: `v1.0.0+gHASH` is 1.0.0, where `v1.0.0-gHASH` would be a pre-release of it.
    let mut label = if count == 0 {
        format!("{tag}+g{hash}")
    } else {
        format!("{tag}+{count}-g{hash}")
    };
    if git(root, &["status", "--porcelain", "--untracked-files=no"])
        .is_some_and(|status| !status.is_empty())
    {
        label.push_str("-dirty");
    }
    label
}

pub fn watch(root: &Path) {
    for name in ["HEAD", "index", "packed-refs", "refs"] {
        if let Some(path) = git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-path", name],
        ) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(files) = git(root, &["ls-files", "-z"]) {
        for file in files.split('\0').filter(|file| !file.is_empty()) {
            println!("cargo:rerun-if-changed={}", root.join(file).display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(root: &Path, args: &[&str]) -> String {
        let result = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", root.join("absent-config"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        String::from_utf8(result.stdout).unwrap().trim().to_owned()
    }

    #[test]
    fn release_ahead_dirty_and_archive() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        assert_eq!(describe(root, "0.1.0"), "v0.1.0+unknown");
        command(root, &["init", "--quiet"]);
        command(root, &["config", "user.name", "Version test"]);
        command(root, &["config", "user.email", "test@example.invalid"]);
        std::fs::write(root.join("source"), "original").unwrap();
        command(root, &["add", "source"]);
        command(root, &["commit", "--quiet", "-m", "initial"]);
        let hash = command(root, &["rev-parse", "--short=7", "HEAD"]);
        assert_eq!(describe(root, "0.1.0"), format!("v0.1.0+1-g{hash}"));
        command(root, &["tag", "-a", "v1.2.3-rc.1", "-m", "release"]);
        let at_tag = format!("v1.2.3-rc.1+g{hash}");
        assert_eq!(describe(root, "0.1.0"), at_tag);
        let parsed = semver::Version::parse(&at_tag[1..]).unwrap();
        assert_eq!(parsed.pre.as_str(), "rc.1");
        command(root, &["commit", "--quiet", "--allow-empty", "-m", "next"]);
        let hash = command(root, &["rev-parse", "--short=7", "HEAD"]);
        command(root, &["tag", "v9.9.9"]);
        let clean = format!("v1.2.3-rc.1+1-g{hash}");
        assert_eq!(describe(root, "0.1.0"), clean);
        std::fs::write(root.join("untracked"), "output").unwrap();
        assert_eq!(describe(root, "0.1.0"), clean);
        std::fs::write(root.join("source"), "changed").unwrap();
        assert_eq!(describe(root, "0.1.0"), format!("{clean}-dirty"));
        let nested = root.join("nested");
        std::fs::create_dir(&nested).unwrap();
        assert_eq!(describe(&nested, "0.1.0"), "v0.1.0+unknown");
        watch(root);
    }
}
