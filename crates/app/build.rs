//! Stamps the build with its number (`AUDRA_BUILD_NUMBER`, commits so far)
//! and the commit it was made from (`AUDRA_BUILD`), so
//! About and the top bar say exactly which Audra is running (the owner,
//! 2026-11-10: "start including the version number somewhere so we know
//! which one we are dealing with"). A "+" marks uncommitted changes to
//! the app's sources (crates/, Cargo.toml, Cargo.lock);
//! "unknown" where there's no git (a source package).

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let hash = git(&["rev-parse", "--short", "HEAD"]).filter(|h| !h.is_empty()).unwrap_or_else(|| "unknown".into());
    // Only the app's own sources count (critique #74 A 6: a changed tools
    // script marked the build "+" and lengthened the top bar's label).
    let dirty = git(&["status", "--porcelain", "--untracked-files=no", "--", ":/crates", ":/Cargo.toml", ":/Cargo.lock"]).is_some_and(|s| !s.is_empty());
    let mark = if dirty { "+" } else { "" };
    // The build number: commits so far, so it grows with every change and
    // a screenshot or a test says which build (the owner: "it needs to be
    // build numbers cos it will be useful for us to test").
    let number = git(&["rev-list", "--count", "HEAD"]).filter(|n| !n.is_empty()).unwrap_or_else(|| "0".into());
    println!("cargo:rustc-env=AUDRA_BUILD={hash}{mark}");
    println!("cargo:rustc-env=AUDRA_BUILD_NUMBER={number}{mark}");
    // A new commit, or files staged or changed, makes a new stamp.
    for path in ["../../.git/HEAD", "../../.git/logs/HEAD", "../../.git/index"] {
        println!("cargo:rerun-if-changed={path}");
    }
}
