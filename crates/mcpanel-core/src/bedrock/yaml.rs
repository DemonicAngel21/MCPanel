//! Targeted, comment-preserving edits of two-level YAML scalars (`section.key`) such as
//! Geyser's `bedrock.port`. Only the value of the addressed line changes; everything
//! else (comments, order, unknown keys, line endings) is kept byte for byte.

/// Lines of `text` with their original terminators.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn indent(line: &str) -> usize {
    line.len() - line.trim_start_matches(' ').len()
}

fn is_blank_or_comment(line: &str) -> bool {
    let t = line.trim();
    t.is_empty() || t.starts_with('#')
}

/// `key: value` on this line (`None` for other lines).
fn key_value(line: &str) -> Option<(&str, &str)> {
    let t = line.trim_start_matches(' ');
    let (k, rest) = t.split_once(':')?;
    if k.is_empty() || k.contains(' ') || k.starts_with('#') || k.starts_with('-') {
        return None;
    }
    Some((k, rest))
}

/// Index of the top-level `section:` line and the range of its body lines.
fn section(ls: &[&str], name: &str) -> Option<(usize, usize)> {
    let start = ls.iter().position(|l| {
        indent(l) == 0 && key_value(l).is_some_and(|(k, v)| k == name && strip(v).is_empty())
    })?;
    let mut end = start + 1;
    while end < ls.len() && (is_blank_or_comment(ls[end]) || indent(ls[end]) > 0) {
        end += 1;
    }
    Some((start, end))
}

/// The direct child line of `section` holding `key`, with its indentation.
fn child(ls: &[&str], section_name: &str, key: &str) -> Option<(usize, usize)> {
    let (start, end) = section(ls, section_name)?;
    let child_indent = ls[start + 1..end]
        .iter()
        .find(|l| !is_blank_or_comment(l))
        .map(|l| indent(l))?;
    (start + 1..end)
        .find(|&i| indent(ls[i]) == child_indent && key_value(ls[i]).is_some_and(|(k, _)| k == key))
        .map(|i| (i, child_indent))
}

/// Value text without an inline comment, quotes or surrounding whitespace.
fn strip(raw: &str) -> String {
    let v = raw.trim();
    let v = if v.starts_with('\'') || v.starts_with('"') {
        v
    } else {
        v.split(" #").next().unwrap_or(v).trim()
    };
    let unq = |q: char| v.strip_prefix(q).and_then(|s| s.strip_suffix(q));
    unq('\'').or_else(|| unq('"')).unwrap_or(v).to_string()
}

/// The scalar at `section.key`, if present.
pub fn get(text: &str, section_name: &str, key: &str) -> Option<String> {
    let ls = lines(text);
    let (i, _) = child(&ls, section_name, key)?;
    key_value(ls[i]).map(|(_, v)| strip(v))
}

/// Set `section.key` to a plain scalar (caller validates it needs no quoting).
pub fn set(text: &str, section_name: &str, key: &str, value: &str) -> String {
    let eol = if text.contains("\r\n") { "\r\n" } else { "\n" };
    let ls = lines(text);
    if let Some((i, ind)) = child(&ls, section_name, key) {
        let old = ls[i];
        let term = &old[old.trim_end_matches(['\r', '\n']).len()..];
        let comment = key_value(old)
            .and_then(|(_, v)| {
                let v = v.trim();
                (!v.starts_with('\'') && !v.starts_with('"'))
                    .then(|| v.find(" #").map(|p| &v[p..]))
                    .flatten()
            })
            .unwrap_or("");
        let line = format!("{}{key}: {value}{comment}{term}", " ".repeat(ind));
        return ls[..i]
            .iter()
            .copied()
            .chain(std::iter::once(line.as_str()))
            .chain(ls[i + 1..].iter().copied())
            .collect();
    }
    if let Some((start, end)) = section(&ls, section_name) {
        let ind = ls[start + 1..end]
            .iter()
            .find(|l| !is_blank_or_comment(l))
            .map_or(2, |l| indent(l));
        let mut out: String = ls[..=start].concat();
        if !out.ends_with('\n') {
            out.push_str(eol);
        }
        out.push_str(&format!("{}{key}: {value}{eol}", " ".repeat(ind)));
        out.push_str(&ls[start + 1..].concat());
        return out;
    }
    let mut out = text.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(eol);
    }
    out.push_str(&format!("{section_name}:{eol}  {key}: {value}{eol}"));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const GEYSER: &str = "# Geyser Configuration File\n\n# Network settings\nbedrock:\n  # The IP address\n  address: 0.0.0.0\n\n  # The port\n  port: 19132\n  signaling:\n    port: 1\n\njava:\n  # auth\n  auth-type: online\n\nmotd:\n  primary-motd: 'Geyser: x'\nconfig-version: 8\n";

    #[test]
    fn reads_direct_children_only() {
        assert_eq!(get(GEYSER, "bedrock", "port").as_deref(), Some("19132"));
        assert_eq!(get(GEYSER, "java", "auth-type").as_deref(), Some("online"));
        assert_eq!(
            get(GEYSER, "motd", "primary-motd").as_deref(),
            Some("Geyser: x")
        );
        assert_eq!(get(GEYSER, "bedrock", "missing"), None);
        assert_eq!(get(GEYSER, "nope", "port"), None);
    }

    #[test]
    fn set_changes_only_the_addressed_value() {
        let out = set(GEYSER, "bedrock", "port", "19140");
        assert_eq!(out, GEYSER.replace("  port: 19132", "  port: 19140"));
        assert_eq!(get(&out, "bedrock", "signaling").as_deref(), Some(""));
        let out = set(&out, "java", "auth-type", "floodgate");
        assert!(out.contains("  # auth\n  auth-type: floodgate\n"));
        assert_eq!(out.lines().count(), GEYSER.lines().count());
    }

    #[test]
    fn set_keeps_crlf_and_inline_comments() {
        let text = "bedrock:\r\n  port: 19132 # udp\r\n";
        assert_eq!(
            set(text, "bedrock", "port", "1"),
            "bedrock:\r\n  port: 1 # udp\r\n"
        );
    }

    #[test]
    fn set_adds_missing_keys_and_sections() {
        let out = set("bedrock:\n  address: 0.0.0.0\n", "bedrock", "port", "19133");
        assert_eq!(out, "bedrock:\n  port: 19133\n  address: 0.0.0.0\n");
        let out = set("", "bedrock", "port", "19133");
        assert_eq!(out, "bedrock:\n  port: 19133\n");
        let out = set(&out, "java", "auth-type", "floodgate");
        assert_eq!(
            out,
            "bedrock:\n  port: 19133\njava:\n  auth-type: floodgate\n"
        );
    }
}
