use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-env-changed=ERRATOR_GIT_COMMIT");

    let describe = std::env::var("ERRATOR_GIT_COMMIT").ok().or_else(|| {
        Command::new("git")
            .args(["describe", "--tags", "--always", "--dirty"])
            .output()
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty())
    });

    println!(
        "cargo:rustc-env=ERRATOR_GIT_DESCRIBE={}",
        describe.unwrap_or_else(|| "unknown".to_owned())
    );
}
