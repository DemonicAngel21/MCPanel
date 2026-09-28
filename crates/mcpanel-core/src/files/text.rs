//! Text encoding detection and round-tripping for the built-in editor.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LineEnding {
    Lf,
    CrLf,
    Mixed,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextEncoding {
    /// WHATWG encoding label, e.g. `UTF-8`, `UTF-16LE`, `windows-1252`.
    pub name: String,
    pub bom: bool,
}

#[derive(Debug, Clone)]
pub struct DecodedText {
    pub text: String,
    pub encoding: TextEncoding,
    pub line_ending: LineEnding,
    /// True when decoding replaced invalid sequences (saving could alter the file).
    pub lossy: bool,
}

/// Heuristic: binary if it contains NUL bytes in the first 8 KiB (and is not UTF-16).
pub fn looks_binary(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
        return false;
    }
    head.contains(&0)
}

pub fn decode(bytes: &[u8]) -> DecodedText {
    let (encoding, bom_len) = match encoding_rs::Encoding::for_bom(bytes) {
        Some((enc, len)) => (enc, len),
        None => {
            if std::str::from_utf8(bytes).is_ok() {
                (encoding_rs::UTF_8, 0)
            } else {
                let mut det = chardetng::EncodingDetector::new(chardetng::Iso2022JpDetection::Deny);
                det.feed(bytes, true);
                (det.guess(None, chardetng::Utf8Detection::Allow), 0)
            }
        }
    };
    let (text, had_errors) = encoding.decode_without_bom_handling(&bytes[bom_len..]);
    let text = text.into_owned();
    let line_ending = detect_line_ending(&text);
    DecodedText {
        text,
        encoding: TextEncoding {
            name: encoding.name().to_string(),
            bom: bom_len > 0,
        },
        line_ending,
        lossy: had_errors,
    }
}

pub fn detect_line_ending(text: &str) -> LineEnding {
    let crlf = text.matches("\r\n").count();
    let lf = text.matches('\n').count().saturating_sub(crlf);
    match (crlf, lf) {
        (0, 0) => LineEnding::None,
        (0, _) => LineEnding::Lf,
        (_, 0) => LineEnding::CrLf,
        _ => LineEnding::Mixed,
    }
}

/// Encode `text` back into `encoding` (BOM preserved). Returns `None` if the text contains
/// characters the encoding cannot represent.
pub fn encode(text: &str, encoding: &TextEncoding) -> Option<Vec<u8>> {
    let enc =
        encoding_rs::Encoding::for_label(encoding.name.as_bytes()).unwrap_or(encoding_rs::UTF_8);
    let mut out = Vec::with_capacity(text.len() + 3);
    if enc == encoding_rs::UTF_16LE || enc == encoding_rs::UTF_16BE {
        let le = enc == encoding_rs::UTF_16LE;
        if encoding.bom {
            out.extend_from_slice(if le { &[0xFF, 0xFE] } else { &[0xFE, 0xFF] });
        }
        for unit in text.encode_utf16() {
            out.extend_from_slice(&if le {
                unit.to_le_bytes()
            } else {
                unit.to_be_bytes()
            });
        }
        return Some(out);
    }
    if enc == encoding_rs::UTF_8 {
        if encoding.bom {
            out.extend_from_slice(&[0xEF, 0xBB, 0xBF]);
        }
        out.extend_from_slice(text.as_bytes());
        return Some(out);
    }
    let (bytes, _, unmappable) = enc.encode(text);
    if unmappable {
        return None;
    }
    out.extend_from_slice(&bytes);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_roundtrip_with_bom() {
        let bytes = b"\xEF\xBB\xBFmotd=Hello\r\n";
        let d = decode(bytes);
        assert_eq!(d.text, "motd=Hello\r\n");
        assert!(d.encoding.bom);
        assert_eq!(d.line_ending, LineEnding::CrLf);
        assert_eq!(encode(&d.text, &d.encoding).unwrap(), bytes);
    }

    #[test]
    fn utf16le_roundtrip() {
        let mut bytes = vec![0xFF, 0xFE];
        for u in "héllo\n".encode_utf16() {
            bytes.extend_from_slice(&u.to_le_bytes());
        }
        let d = decode(&bytes);
        assert_eq!(d.text, "héllo\n");
        assert_eq!(d.encoding.name, "UTF-16LE");
        assert_eq!(encode(&d.text, &d.encoding).unwrap(), bytes);
    }

    #[test]
    fn legacy_encoding_detected() {
        // "Café" in windows-1252 is not valid UTF-8.
        let bytes = b"Caf\xE9 au lait, tr\xE8s bien\n";
        let d = decode(bytes);
        assert!(d.text.starts_with("Café"));
        assert_ne!(d.encoding.name, "UTF-8");
        assert_eq!(encode(&d.text, &d.encoding).unwrap(), bytes);
    }

    #[test]
    fn binary_detection() {
        assert!(looks_binary(b"PK\x03\x04\x00\x00"));
        assert!(!looks_binary(b"plain text"));
    }
}
