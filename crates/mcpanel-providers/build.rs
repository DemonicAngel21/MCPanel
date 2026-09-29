//! Cloud OAuth values are compiled in from the build environment (see
//! docs/development.md, "Cloud storage app registrations"). Rebuild when they change, and
//! say which ones this build embeds (names only, never values) so a personal credential
//! is not baked into a build by accident.

const VARS: &[&str] = &[
    "MCPANEL_GOOGLE_CLIENT_ID",
    "MCPANEL_GOOGLE_CLIENT_SECRET",
    "MCPANEL_MICROSOFT_CLIENT_ID",
    "MCPANEL_DROPBOX_CLIENT_ID",
];

fn main() {
    let mut set = Vec::new();
    for v in VARS {
        println!("cargo:rerun-if-env-changed={v}");
        if std::env::var(v).is_ok_and(|s| !s.trim().is_empty()) {
            set.push(*v);
        }
    }
    if !set.is_empty() {
        println!(
            "cargo:warning=This build embeds cloud OAuth configuration from the environment: {}",
            set.join(", ")
        );
    }
}
