//! Per-domain log files with secret redaction (defence in depth: secrets should never
//! reach a log call in the first place).

use regex::Regex;
use std::io::Write;
use std::path::Path;
use std::sync::LazyLock;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer, filter};

static SECRET_PATTERNS: LazyLock<Vec<(Regex, &'static str)>> = LazyLock::new(|| {
    [
        (r"(?i)(authorization:\s*bearer\s+)[A-Za-z0-9._~+/=-]+", "${1}[REDACTED]"),
        (r"(?i)(bearer\s+)[A-Za-z0-9._~+/=-]{16,}", "${1}[REDACTED]"),
        (r"AGE-SECRET-KEY-1[0-9A-Z]+", "[REDACTED-AGE-KEY]"),
        (r"GOCSPX-[A-Za-z0-9_-]+", "[REDACTED-CLIENT-SECRET]"),
        (
            r"(?i)(\b(?:access_token|refresh_token|id_token|client_secret|password|secret|token|code)=)[^&\s]+",
            "${1}[REDACTED]",
        ),
        (r#"(?i)("(?:access_token|refresh_token|client_secret|password|secret)"\s*:\s*")[^"]*"#, "${1}[REDACTED]"),
        (r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]*?-----END [A-Z ]*PRIVATE KEY-----", "[REDACTED-PRIVATE-KEY]"),
    ]
    .into_iter()
    .filter_map(|(p, r)| Regex::new(p).ok().map(|re| (re, r)))
    .collect()
});

pub fn redact(input: &str) -> std::borrow::Cow<'_, str> {
    let mut out = std::borrow::Cow::Borrowed(input);
    for (re, rep) in SECRET_PATTERNS.iter() {
        if re.is_match(&out) {
            out = std::borrow::Cow::Owned(re.replace_all(&out, *rep).into_owned());
        }
    }
    out
}

/// Writer wrapper that scrubs each formatted record before it reaches the file.
#[derive(Clone)]
struct Redacting<M>(M);

struct RedactingWriter<W>(W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        let clean = redact(&text);
        self.0.write_all(clean.as_bytes())?;
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for Redacting<M> {
    type Writer = RedactingWriter<M::Writer>;
    fn make_writer(&'a self) -> Self::Writer {
        RedactingWriter(self.0.make_writer())
    }
}

fn appender(dir: &Path, name: &str) -> Option<RollingFileAppender> {
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(name)
        .filename_suffix("log")
        .max_log_files(14)
        .build(dir)
        .ok()
}

/// Initialise logging. Returns guards that must live as long as the app.
pub fn init(logs_dir: &Path) -> Vec<WorkerGuard> {
    let _ = std::fs::create_dir_all(logs_dir);
    let mut guards = Vec::new();
    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();

    // (file, targets) — `app` receives everything from MCPanel.
    let domains: &[(&str, &[&str])] = &[
        ("app", &["mcpanel", "mcpanel_desktop"]),
        ("server", &["mcpanel::server"]),
        ("java", &["mcpanel::java"]),
        (
            "providers",
            &[
                "mcpanel::catalog",
                "mcpanel::providers",
                "mcpanel::platform",
            ],
        ),
        ("db", &["mcpanel::db", "mcpanel::jobs", "mcpanel::audit"]),
    ];
    for (name, targets) in domains {
        let Some(app) = appender(logs_dir, name) else {
            continue;
        };
        let (nb, guard) = tracing_appender::non_blocking(app);
        guards.push(guard);
        let targets: Vec<String> = targets.iter().map(|s| s.to_string()).collect();
        let is_app = *name == "app";
        let f = filter::filter_fn(move |meta| {
            let t = meta.target();
            if is_app {
                return targets.iter().any(|p| t.starts_with(p.as_str()))
                    || *meta.level() <= tracing::Level::WARN;
            }
            targets.iter().any(|p| t.starts_with(p.as_str()))
        });
        layers.push(
            tracing_subscriber::fmt::layer()
                .with_writer(Redacting(nb))
                .with_ansi(false)
                .with_target(true)
                .with_filter(f)
                .boxed(),
        );
    }
    let env = EnvFilter::try_from_env("MCPANEL_LOG").unwrap_or_else(|_| EnvFilter::new("info"));
    let registry = tracing_subscriber::registry().with(env).with(layers);
    #[cfg(debug_assertions)]
    let registry =
        registry.with(tracing_subscriber::fmt::layer().with_writer(Redacting(std::io::stderr)));
    let _ = registry.try_init();
    guards
}

#[cfg(test)]
mod tests {
    use super::redact;

    #[test]
    fn redacts_common_secret_shapes() {
        assert_eq!(
            redact("Authorization: Bearer abc.def-ghi"),
            "Authorization: Bearer [REDACTED]"
        );
        assert!(!redact("url?code=XYZ&state=1").contains("XYZ"));
        assert!(!redact("refresh_token=r3fr35h").contains("r3fr35h"));
        assert!(!redact(r#"{"access_token":"tok123"}"#).contains("tok123"));
        assert!(!redact("AGE-SECRET-KEY-1QQQQQQQQQQQQ").contains("QQQQ"));
        // Google Desktop client secret (embedded in releases, never logged).
        assert!(!redact("client_id=x&client_secret=GOCSPX-abcDEF_123").contains("abcDEF_123"));
        assert!(!redact("value GOCSPX-abcDEF_123 seen").contains("abcDEF_123"));
        assert!(!redact(r#"{"client_secret":"GOCSPX-abcDEF_123"}"#).contains("abcDEF_123"));
        assert!(
            !redact("-----BEGIN PRIVATE KEY-----\nMIIE\n-----END PRIVATE KEY-----")
                .contains("MIIE")
        );
        assert_eq!(
            redact("Server started on port 25565"),
            "Server started on port 25565"
        );
    }
}
