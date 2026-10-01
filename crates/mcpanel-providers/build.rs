//! Cloud OAuth values are compiled in from the build environment (see
//! docs/development.md, "Cloud storage app registrations"). Rebuild when they change, and
//! say which ones this build embeds (names only, never values) so a personal credential
//! is not baked into a build by accident.

const VARS: &[&str] = &[
    "MCPANEL_GOOGLE_CLIENT_ID",
    "MCPANEL_GOOGLE_CLIENT_SECRET",
    "MCPANEL_DROPBOX_CLIENT_ID",
    "MCPANEL_MICROSOFT_CLIENT_ID",
    "MCPANEL_MICROSOFT_CLIENT_SECRET",
    "MCPANEL_FIREBASE_API_KEY",
];

fn main() {
    let mut set = Vec::new();
    let mut firebase_configured = false;
    for v in VARS {
        println!("cargo:rerun-if-env-changed={v}");
        if std::env::var(v).is_ok_and(|s| !s.trim().is_empty()) {
            set.push(*v);
            firebase_configured |= *v == "MCPANEL_FIREBASE_API_KEY";
        }
    }
    if !set.is_empty() {
        println!(
            "cargo:warning=This build embeds cloud OAuth configuration from the environment: {}",
            set.join(", ")
        );
    }
    if !firebase_configured {
        println!(
            "cargo:warning=Firebase accounts will be unavailable in this build: MCPANEL_FIREBASE_API_KEY is unset"
        );
    }
}
