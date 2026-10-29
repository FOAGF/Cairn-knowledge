fn main() {
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    build_time(release);
    git_commit(release);
    tauri_build::build()
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
/// files watched below), not on every edit, so it records no local-changes mark. Without git (a
/// source zip) nothing is recorded and About says "Unknown".
fn git_commit(release: bool) {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
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
        let watched = ["HEAD".to_string(), "packed-refs".to_string()]
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
