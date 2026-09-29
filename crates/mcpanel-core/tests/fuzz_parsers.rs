//! Property-based "fuzz" tests: parsers that read untrusted text (server files, log
//! output, provider data, user-supplied files) must never panic, and the lossless
//! editors must keep what they do not change.

use mcpanel_core::bedrock::yaml;
use mcpanel_core::config::PropertiesDocument;
use mcpanel_core::console::dialect;
use mcpanel_core::diagnostics;
use mcpanel_core::perf;
use mcpanel_core::software::TpsSource;
use proptest::prelude::*;

/// Text that looks like the files and logs MCPanel reads (keys, separators,
/// escapes, brackets, stack-trace fragments, odd whitespace and Unicode).
fn texty() -> impl Strategy<Value = String> {
    let atoms = prop::sample::select(vec![
        "a",
        "key",
        "=",
        ":",
        " ",
        "\t",
        "\\",
        "\n",
        "\r\n",
        "#",
        "!",
        "[",
        "]",
        "/",
        ".",
        "-",
        "12:00:00",
        "INFO",
        "ERROR",
        "Server thread",
        "at ",
        "(",
        ")",
        "Foo.jar//",
        "java.lang.",
        "Exception",
        "Caused by: ",
        "System chat: ",
        "ms",
        "P95: ",
        "TPS from last 1m, 5m, 15m: ",
        "é",
        "日本",
        "\u{0}",
        "\u{202e}",
        "'",
        "\"",
        "  ",
        "bedrock:",
        "port: ",
        "\u{feff}",
    ]);
    prop::collection::vec(atoms, 0..60).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn properties_round_trip_losslessly(text in texty()) {
        let doc = PropertiesDocument::parse(&text);
        // Content is kept byte for byte; the document writes one line-ending style.
        let norm = |s: &str| s.replace("\r\n", "\n");
        prop_assert_eq!(norm(&doc.to_text()), norm(&text));
    }

    #[test]
    fn properties_set_changes_only_that_key(text in texty(), value in "[a-zA-Z0-9 ]{0,20}") {
        let mut doc = PropertiesDocument::parse(&text);
        let before: Vec<(String, String)> = doc
            .entries()
            .into_iter()
            .filter(|(k, _)| k != "motd")
            .collect();
        if doc.set("motd", &value).is_ok() {
            let again = PropertiesDocument::parse(&doc.to_text());
            prop_assert_eq!(again.get("motd").map(str::trim_end), Some(value.trim_end()));
            let after: Vec<(String, String)> = again
                .entries()
                .into_iter()
                .filter(|(k, _)| k != "motd")
                .collect();
            prop_assert_eq!(before, after);
        }
    }

    #[test]
    fn yaml_patch_never_panics_and_sets_the_value(text in texty(), port in 1024u16..65535) {
        let out = yaml::set(&text, "bedrock", "port", &port.to_string());
        prop_assert_eq!(yaml::get(&out, "bedrock", "port"), Some(port.to_string()));
        let _ = yaml::get(&text, "java", "auth-type");
    }

    #[test]
    fn log_lines_never_panic(line in texty()) {
        let _ = dialect::parse_line(&line);
        let _ = perf::message(&line);
        for s in [TpsSource::VanillaTickQuery, TpsSource::PaperCommands] {
            let _ = perf::is_reply(s, &line);
            let _ = perf::parse(s, std::slice::from_ref(&line), mcpanel_core::time::Timestamp(0));
        }
    }

    #[test]
    fn crash_analysis_never_panics(lines in prop::collection::vec(texty(), 0..40)) {
        let _ = diagnostics::analyze(&lines, &[]);
    }

    #[test]
    fn decoding_arbitrary_bytes_never_panics(bytes in prop::collection::vec(any::<u8>(), 0..300)) {
        let _ = PropertiesDocument::parse(&mcpanel_core::config::properties::decode_bytes(&bytes));
    }
}
