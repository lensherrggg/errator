use serde::Serialize;

use crate::model::SCHEMA_VERSION;

#[derive(Debug, Clone, Serialize)]
pub struct VersionInfo {
    pub name: &'static str,
    pub version: &'static str,
    pub schema_version: &'static str,
    pub git: &'static str,
    pub os: &'static str,
    pub arch: &'static str,
    pub profile: &'static str,
}

pub fn info() -> VersionInfo {
    VersionInfo {
        name: "errator",
        version: env!("CARGO_PKG_VERSION"),
        schema_version: SCHEMA_VERSION,
        git: env!("ERRATOR_GIT_DESCRIBE"),
        os: std::env::consts::OS,
        arch: std::env::consts::ARCH,
        profile: if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        },
    }
}

pub fn short() -> String {
    format!("errator {}", env!("CARGO_PKG_VERSION"))
}

pub fn long() -> String {
    let info = info();
    format!(
        "Errator {}\nschema: {}\ngit: {}\nplatform: {}/{}\nprofile: {}",
        info.version, info.schema_version, info.git, info.os, info.arch, info.profile
    )
}
