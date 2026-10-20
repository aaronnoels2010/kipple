// SPDX-License-Identifier: GPL-3.0-or-later
//! Keeps file paths out of the logs (ARCHITECTURE §10, Redaction). A path becomes
//! `<path#1a2b3c4d5e6f>`: an HMAC-SHA256 of the path, keyed with a per-install salt that never
//! leaves the machine. The same path always gives the same tag, so a log still shows "the same
//! file failed twice" without saying which file.
//!
//! Two layers ([`crate::logging`]): fields named like a path (`path`, `source_path`, `file`,
//! `dir`…) are hashed whole, spaces and all ([`Redactor::tag_value`]); and every line is scrubbed
//! for path-like text ([`Redactor::scrub`]), which also catches paths inside third-party
//! messages. The scrubber can't know where an unquoted path with spaces ends, so put paths in
//! fields, never in the message text.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

/// Hashes paths with a per-install key.
pub struct Redactor {
    mac: Hmac<Sha256>,
}

impl Redactor {
    pub fn new(salt: [u8; 32]) -> Self {
        Self {
            mac: Hmac::new_from_slice(&salt).expect("HMAC takes any key length"),
        }
    }

    /// `<path#…>` for one path: 6 bytes of its HMAC, enough to tell paths apart in a log
    /// without being reversible.
    pub fn tag(&self, path: &str) -> String {
        let hash = self
            .mac
            .clone()
            .chain_update(path.as_bytes())
            .finalize()
            .into_bytes();
        let hex: String = hash[..6].iter().map(|b| format!("{b:02x}")).collect();
        format!("<path#{hex}>")
    }

    /// The tag for a formatted field value: Debug output (`"C:\\x"`) is unquoted and unescaped
    /// first, so `?path` and `%path.display()` give the same tag.
    pub fn tag_value(&self, formatted: &str) -> String {
        match formatted
            .strip_prefix('"')
            .and_then(|s| s.strip_suffix('"'))
        {
            Some(debug) => self.tag(&unescape(debug)),
            None => self.tag(formatted),
        }
    }

    /// `text` with every path-like substring replaced by its tag.
    pub fn scrub(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        let mut prev: Option<char> = None;
        while let Some(ch) = rest.chars().next() {
            if at_boundary(prev) {
                // http(s) links (the updater) stay readable; other paths become tags.
                let found = match web_url_len(rest) {
                    Some(len) => Some((len, rest[..len].to_string())),
                    None if starts_path(rest) => Some(self.path_at(rest, prev)),
                    None => None,
                };
                if let Some((len, replacement)) = found {
                    out.push_str(&replacement);
                    prev = rest[..len].chars().next_back();
                    rest = &rest[len..];
                    continue;
                }
            }
            out.push(ch);
            prev = Some(ch);
            rest = &rest[ch.len_utf8()..];
        }
        out
    }

    /// The length and tag of the path at the start of `s`. After an opening quote the path runs
    /// to the closing one; after `"` it's Debug output, so it's unescaped before hashing.
    fn path_at(&self, s: &str, prev: Option<char>) -> (usize, String) {
        match prev
            .filter(|q| *q == '"' || *q == '\'')
            .and_then(|q| Some((q, quoted_len(s, q)?)))
        {
            Some(('"', len)) => (len, self.tag(&unescape(&s[..len]))),
            Some((_, len)) => (len, self.tag(&s[..len])),
            None => {
                let len = unquoted_len(s);
                (len, self.tag(&s[..len]))
            }
        }
    }
}

/// Undoes Rust's Debug escapes for quotes and backslashes.
fn unescape(debug: &str) -> String {
    let mut out = String::with_capacity(debug.len());
    let mut chars = debug.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(&next @ ('\\' | '"' | '\''))) => {
                out.push(next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// A path can start at the beginning, after whitespace or after punctuation that typically
/// precedes one (`"…"`, `(…)`, `` `…` ``, `key=…`, `error:…`, `a->…`). Not after a letter or digit,
/// so ratios (`1/2`) and words (`and/or`) stay intact.
fn at_boundary(prev: Option<char>) -> bool {
    prev.is_none_or(|c| c.is_whitespace() || "\"'`([{<>=,:;|@*".contains(c))
}

/// The length of an `http://` or `https://` URL at the start of `s`.
fn web_url_len(s: &str) -> Option<usize> {
    let lower = s.get(..8).unwrap_or(s).to_ascii_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")).then(|| {
        s.find(|c: char| c.is_whitespace() || "\"'`<>".contains(c))
            .unwrap_or(s.len())
    })
}

/// `/x`, `//host`, `./x`, `../x`, `~/x`, `~user/x`, `C:\`, `C:/`, `\\server`, or a URL with
/// another scheme (`file://`, `smb://`…).
fn starts_path(s: &str) -> bool {
    let b = s.as_bytes();
    let path_char = |i: usize| b.get(i).is_some_and(|c| !c.is_ascii_whitespace());
    let sep = |c: u8| c == b'/' || c == b'\\';
    match b {
        [b'/', b'/', ..] => path_char(2) && b.get(2) != Some(&b'/'),
        [b'/', ..] => path_char(1),
        [b'.', b'.', c, ..] | [b'.', c, ..] if sep(*c) => true,
        [b'~', ..] => {
            let name = s[1..].find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '-'));
            name.is_some_and(|n| sep(b[1 + n]))
        }
        [b'\\', b'\\', ..] => path_char(2),
        [drive, b':', c, ..] if sep(*c) => drive.is_ascii_alphabetic(),
        _ => scheme_len(s).is_some(),
    }
}

/// `smb://…` → 3. At least two letters, so `C://` stays a drive.
fn scheme_len(s: &str) -> Option<usize> {
    let end = s.find("://")?;
    let scheme = &s[..end];
    (scheme.len() >= 2
        && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c)))
    .then_some(end)
}

/// Up to the closing quote (Debug escapes like `\\` and `\"` are skipped over).
fn quoted_len(s: &str, quote: char) -> Option<usize> {
    let mut chars = s.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            c if c == quote => return Some(i),
            _ => {}
        }
    }
    None
}

/// Whitespace ends the path, unless the next word has a separator too ("My Documents/x.pdf").
/// Trailing punctuation ("a.txt:", "(…/x)", "`…`") stays outside.
fn unquoted_len(s: &str) -> usize {
    let word_end = |from: usize| {
        s[from..]
            .find(|c: char| c.is_whitespace() || "\"'`".contains(c))
            .map_or(s.len(), |n| from + n)
    };
    let mut end = word_end(0);
    loop {
        let next = end + s[end..].len() - s[end..].trim_start_matches(' ').len();
        if next == end || next == s.len() {
            break;
        }
        let next_end = word_end(next);
        if !s[next..next_end].contains(['/', '\\']) || starts_path(&s[next..]) {
            break;
        }
        end = next_end;
    }
    s[..end]
        .trim_end_matches(['.', ',', ';', ':', ')', ']', '}', '>', '!', '?'])
        .len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redactor() -> Redactor {
        Redactor::new([7; 32])
    }

    /// Scrubs `text` and checks none of `secrets` survived.
    fn scrubbed(text: &str, secrets: &[&str]) -> String {
        let out = redactor().scrub(text);
        for secret in secrets {
            assert!(!out.contains(secret), "{secret:?} leaked: {out}");
        }
        out
    }

    #[test]
    fn tags_are_stable_per_install_and_differ_between_installs() {
        let a = redactor().tag("/Users/zoë/Desktop");
        assert_eq!(a, redactor().tag("/Users/zoë/Desktop"));
        assert_ne!(a, redactor().tag("/Users/zoë/Desktop2"));
        assert_ne!(a, Redactor::new([8; 32]).tag("/Users/zoë/Desktop"));
        assert!(
            a.starts_with("<path#") && a.ends_with('>') && a.len() == "<path#123456789abc>".len(),
            "{a}"
        );
    }

    #[test]
    fn quoted_paths_are_replaced_whole_even_with_spaces() {
        let out = scrubbed(
            r#"opening "/Users/zoë/My Documents/tax 2025.pdf" failed"#,
            &["zoë", "Documents", "tax", "2025"],
        );
        let tag = redactor().tag("/Users/zoë/My Documents/tax 2025.pdf");
        assert_eq!(out, format!(r#"opening "{tag}" failed"#));
    }

    #[test]
    fn unquoted_paths_end_at_the_last_segment_with_a_separator() {
        let out = scrubbed(
            "moved /Users/zoë/My Documents/old/setup.dmg to Trash",
            &["zoë", "Documents", "setup"],
        );
        assert!(
            out.starts_with("moved <path#") && out.ends_with("> to Trash"),
            "{out}"
        );
    }

    #[test]
    fn trailing_punctuation_stays_outside_the_tag() {
        let out = scrubbed(
            "/home/zoë/a.txt: Permission denied (os error 13)",
            &["zoë", "a.txt"],
        );
        let tag = redactor().tag("/home/zoë/a.txt");
        assert_eq!(out, format!("{tag}: Permission denied (os error 13)"));
    }

    #[test]
    fn windows_unc_home_and_file_url_forms() {
        // Debug output doubles backslashes, so cover both spellings.
        scrubbed(
            r#"path: "C:\\Users\\Zoë\\Desktop\\x.txt""#,
            &["Zoë", "Desktop", "x.txt"],
        );
        scrubbed(
            r"C:\Users\Zoë\Desktop\x.txt not found",
            &["Zoë", "Desktop", "x.txt"],
        );
        scrubbed(r"D:/Photos/2024/beach.jpg", &["Photos", "beach"]);
        scrubbed(
            r#"share "\\\\nas\\family\\photos""#,
            &["nas", "family", "photos"],
        );
        scrubbed(r"\\nas\family\photos", &["nas", "family", "photos"]);
        scrubbed("~/Pictures/me.png", &["Pictures", "me.png"]);
        scrubbed("file:///Users/zoë/x.pdf", &["zoë", "x.pdf"]);
        scrubbed("in '/Users/zoë/a b' now", &["zoë", "a b"]);
        scrubbed("path=/Users/zoë/x", &["zoë"]);
        scrubbed("(/Users/zoë/x)", &["zoë"]);
    }

    #[test]
    fn paths_after_punctuation_are_caught() {
        for text in [
            "failed to create `/Users/zoë/secret.pdf`",
            "error:/Users/zoë/secret.pdf",
            "a.txt->/Users/zoë/secret.pdf",
            "x|/Users/zoë/secret.pdf",
            "a;/Users/zoë/secret.pdf",
            "user@/Users/zoë/secret.pdf",
            "glob */Users/zoë/secret.pdf",
        ] {
            scrubbed(text, &["zoë", "secret"]);
        }
    }

    #[test]
    fn relative_network_and_other_url_forms_are_caught() {
        scrubbed("./Belastingen/aangifte.pdf", &["Belastingen", "aangifte"]);
        scrubbed("from ../zoë/x.pdf", &["zoë", "x.pdf"]);
        scrubbed("//nas/family/photos", &["nas", "family"]);
        scrubbed("smb://nas/family/photos", &["nas", "family"]);
        scrubbed("~zoë/Documents", &["zoë", "Documents"]);
    }

    #[test]
    fn debug_and_display_spellings_give_the_same_tag() {
        let r = redactor();
        let tag = r.tag(r"C:\Users\Zoë\x.txt");
        assert_eq!(
            r.scrub(r#"path="C:\\Users\\Zoë\\x.txt""#),
            format!(r#"path="{tag}""#)
        );
        assert_eq!(r.scrub(r"C:\Users\Zoë\x.txt"), tag);
        assert_eq!(r.tag_value(r#""C:\\Users\\Zoë\\x.txt""#), tag);
        assert_eq!(r.tag_value(r"C:\Users\Zoë\x.txt"), tag);
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for text in [
            "scanned 1/2 locations in 3.4s",
            "kipple_app::appearance: theme changed and/or reloaded",
            "update check https://github.com/kipple/kipple/releases/latest.json",
            "v1.2.3 on macos, 50% done",
            "a / b",
            "",
        ] {
            assert_eq!(redactor().scrub(text), text);
        }
    }
}
