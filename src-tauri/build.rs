fn main() {
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    build_time(release);
    git_commit(release);
    build_version(release);
    tauri_build::build()
}

/// The output of `git args`, trimmed; `None` without git or when it fails.
fn git(args: &[&str]) -> Option<String> {
    std::process::Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// The version the build carries (D-90; Settings › About, the manifest of exports and backups) as
/// `CAIRN_BUILD_VERSION`: the one the release scripts pass (`packaging/version.sh`, which names the
/// files too), else the same worked out here: `git describe` from the newest `v` tag, without the
/// `v` (`1.0.0` on the tagged commit, `1.0.0-47-g3bb7870` after it), `-dirty` added in a release
/// build with uncommitted changes. Without git or a tag, the crate's version.
fn build_version(release: bool) {
    println!("cargo:rerun-if-env-changed=CAIRN_BUILD_VERSION");
    let given = std::env::var("CAIRN_BUILD_VERSION")
        .ok()
        .filter(|v| !v.is_empty());
    let version = given
        .or_else(|| {
            let mut args = vec!["describe", "--tags", "--match", "v[0-9]*"];
            if release {
                args.push("--dirty");
            }
            git(&args).map(|d| d.strip_prefix('v').unwrap_or(&d).to_string())
        })
        .unwrap_or_else(|| std::env::var("CARGO_PKG_VERSION").unwrap_or_default());
    println!("cargo:rustc-env=CAIRN_BUILD_VERSION={version}");
}

/// Settings › About (UI-SET-5): a release build records when its Rust part was compiled, as Unix
/// seconds (`CAIRN_BUILD_EPOCH`). The rerun line names a file that never exists, so Cargo runs this
/// script on every release build and the time is never stale. Development builds skip it and keep
/// their quick rebuilds; About shows their program file's time instead.
fn build_time(release: bool) {
    if !release {
        return;
    }
    println!("cargo:rerun-if-changed=build-time-never-exists");
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=CAIRN_BUILD_EPOCH={secs}");
}

/// Settings › About (UI-SET-5): the commit the build was made from (`CAIRN_GIT_COMMIT`, the full
/// SHA). A release build also sets `CAIRN_GIT_DIRTY=1` when tracked files differ from that commit
/// (untracked files don't count). A development build runs this again only when HEAD moves (the
/// files watched below, and the tags for [`build_version`]), not on every edit, so it records no
/// local-changes mark. Without git (a source zip) nothing is recorded and About says "Unknown".
fn git_commit(release: bool) {
    let Some(sha) = git(&["rev-parse", "HEAD"]) else {
        return;
    };
    println!("cargo:rustc-env=CAIRN_GIT_COMMIT={sha}");
    if release {
        // `git diff --quiet HEAD` exits with 1 when tracked files differ from HEAD.
        let dirty = std::process::Command::new("git")
            .args(["diff", "--quiet", "HEAD"])
            .status()
            .is_ok_and(|s| s.code() == Some(1));
        if dirty {
            println!("cargo:rustc-env=CAIRN_GIT_DIRTY=1");
        }
    } else if let Some(dir) = git(&["rev-parse", "--absolute-git-dir"]) {
        // Only files that exist: a missing one would make Cargo run this on every build.
        let branch = git(&["symbolic-ref", "-q", "HEAD"]);
        let watched = ["HEAD", "packed-refs", "refs/tags"]
            .map(String::from)
            .into_iter()
            .chain(branch);
        for f in watched {
            let path = std::path::Path::new(&dir).join(f);
            if path.exists() {
                println!("cargo:rerun-if-changed={}", path.display());
            }
        }
    }
}
