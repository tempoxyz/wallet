use std::process::Command;

fn main() {
    // Git SHA: prefer CI-provided env, then invoke git, fallback to "unknown"
    let git_sha = std::env::var("PRESTO_GIT_SHA").unwrap_or_else(|_| {
        Command::new("git")
            .args(["rev-parse", "--short=7", "HEAD"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    });

    // Build date: prefer CI-provided env, fallback to current UTC via `date`
    let build_date = std::env::var("PRESTO_BUILD_DATE").unwrap_or_else(|_| {
        Command::new("date")
            .args(["-u", "+%Y-%m-%dT%H:%M:%SZ"])
            .output()
            .ok()
            .and_then(|o| {
                if o.status.success() {
                    String::from_utf8(o.stdout)
                        .ok()
                        .map(|s| s.trim().to_string())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    });

    // Build profile: prefer CI-provided env, then Cargo's PROFILE
    let profile = std::env::var("PRESTO_BUILD_PROFILE")
        .or_else(|_| std::env::var("PROFILE"))
        .unwrap_or_else(|_| "unknown".to_string());

    println!("cargo:rustc-env=PRESTO_GIT_SHA={git_sha}");
    println!("cargo:rustc-env=PRESTO_BUILD_DATE={build_date}");
    println!("cargo:rustc-env=PRESTO_BUILD_PROFILE={profile}");

    // Re-run when build script, git HEAD, or CI env vars change
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    if let Ok(head) = std::fs::read_to_string(".git/HEAD") {
        if let Some(ref_path) = head.strip_prefix("ref: ") {
            println!("cargo:rerun-if-changed=.git/{}", ref_path.trim());
        }
    }
    println!("cargo:rerun-if-env-changed=PRESTO_GIT_SHA");
    println!("cargo:rerun-if-env-changed=PRESTO_BUILD_DATE");
    println!("cargo:rerun-if-env-changed=PRESTO_BUILD_PROFILE");
}
