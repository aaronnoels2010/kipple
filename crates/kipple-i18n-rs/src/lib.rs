// SPDX-License-Identifier: GPL-3.0-or-later
//! The tray and notification strings, for when no webview is loaded (ADR-0004). Messages come
//! from `locales/<lang>/tray.json`, compiled in by build.rs. Supported ICU subset: text,
//! `{name}` and `{name, plural, one {…} other {…}}` with `#`; plural rules come from ICU4X.
#![forbid(unsafe_code)]

include!(concat!(env!("OUT_DIR"), "/catalog.rs"));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    En,
    Nl,
}

impl Locale {
    /// From an OS locale such as `nl-BE` or `nl_NL`; English unless it's Dutch.
    pub fn from_tag(tag: &str) -> Self {
        if tag.to_ascii_lowercase().starts_with("nl") {
            Locale::Nl
        } else {
            Locale::En
        }
    }

    fn code(self) -> &'static str {
        match self {
            Locale::En => "en",
            Locale::Nl => "nl",
        }
    }

    fn group_separator(self) -> char {
        match self {
            Locale::En => ',',
            Locale::Nl => '.',
        }
    }

    fn plural_category(self, n: u64) -> &'static str {
        use icu_plurals::{PluralCategory, PluralRules};
        let locale = icu_locale_core::Locale::try_from_str(self.code()).expect("valid locale code");
        let Ok(rules) = PluralRules::try_new(locale.into(), Default::default()) else {
            return "other";
        };
        match rules.category_for(n) {
            PluralCategory::Zero => "zero",
            PluralCategory::One => "one",
            PluralCategory::Two => "two",
            PluralCategory::Few => "few",
            PluralCategory::Many => "many",
            PluralCategory::Other => "other",
        }
    }

    fn number(self, n: u64) -> String {
        let digits = n.to_string();
        let mut out = String::with_capacity(digits.len() + digits.len() / 3);
        for (i, ch) in digits.chars().enumerate() {
            if i > 0 && (digits.len() - i).is_multiple_of(3) {
                out.push(self.group_separator());
            }
            out.push(ch);
        }
        out
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Arg<'a> {
    Text(&'a str),
    Number(u64),
}

/// Formats `key` for `locale`, or `None` if the key doesn't exist or uses unsupported syntax.
pub fn format(locale: Locale, key: &str, args: &[(&str, Arg)]) -> Option<String> {
    let (_, _, message) = MESSAGES
        .iter()
        .find(|(l, k, _)| *l == locale.code() && *k == key)?;
    render(message, locale, args, None)
}

/// Splits `{...}` off the start of `s` (which begins after the opening brace), honouring nesting.
fn split_braced(s: &str) -> Option<(&str, &str)> {
    let mut depth = 1;
    for (i, ch) in s.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((&s[..i], &s[i + 1..]));
                }
            }
            _ => {}
        }
    }
    None
}

fn render(
    message: &str,
    locale: Locale,
    args: &[(&str, Arg)],
    pound: Option<u64>,
) -> Option<String> {
    let mut out = String::new();
    let mut rest = message;
    while let Some(ch) = rest.chars().next() {
        rest = &rest[ch.len_utf8()..];
        match ch {
            '{' => {
                let (inner, after) = split_braced(rest)?;
                rest = after;
                out.push_str(&argument(inner, locale, args)?);
            }
            '#' if let Some(n) = pound => out.push_str(&locale.number(n)),
            // ICU apostrophe quoting ('' or '{) isn't supported; a lone apostrophe is literal.
            '\'' if rest.starts_with(['\'', '{', '}', '#']) => return None,
            '}' => return None,
            _ => out.push(ch),
        }
    }
    Some(out)
}

/// `name` or `name, plural, one {…} other {…}`.
fn argument(inner: &str, locale: Locale, args: &[(&str, Arg)]) -> Option<String> {
    let mut parts = inner.splitn(3, ',');
    let name = parts.next()?.trim();
    let value = args.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)?;
    match (parts.next().map(str::trim), value) {
        (None, Arg::Text(text)) => Some(text.to_string()),
        (None, Arg::Number(n)) => Some(locale.number(n)),
        (Some("plural"), Arg::Number(n)) => {
            let options = plural_options(parts.next()?)?;
            let exact = format!("={n}");
            let category = locale.plural_category(n);
            let chosen = options
                .iter()
                .find(|(sel, _)| *sel == exact)
                .or_else(|| options.iter().find(|(sel, _)| *sel == category))
                .or_else(|| options.iter().find(|(sel, _)| *sel == "other"))?;
            render(chosen.1, locale, args, Some(n))
        }
        _ => None,
    }
}

/// `one {…} other {…}` → [("one", "…"), ("other", "…")].
fn plural_options(mut s: &str) -> Option<Vec<(&str, &str)>> {
    let mut options = Vec::new();
    loop {
        s = s.trim_start();
        if s.is_empty() {
            return Some(options);
        }
        let open = s.find('{')?;
        let selector = s[..open].trim();
        let (body, after) = split_braced(&s[open + 1..])?;
        options.push((selector, body));
        s = after;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locale_from_os_tags() {
        assert_eq!(Locale::from_tag("nl-BE"), Locale::Nl);
        assert_eq!(Locale::from_tag("nl_NL"), Locale::Nl);
        assert_eq!(Locale::from_tag("fr-BE"), Locale::En);
        assert_eq!(Locale::from_tag(""), Locale::En);
    }

    #[test]
    fn plain_messages() {
        assert_eq!(
            format(Locale::En, key::TRAY_OPEN, &[]).as_deref(),
            Some("Open Kipple")
        );
        assert_eq!(
            format(Locale::Nl, key::TRAY_OPEN, &[]).as_deref(),
            Some("Kipple openen")
        );
    }

    #[test]
    fn plurals_follow_the_language_rules_and_group_digits() {
        let n = |count| [("count", Arg::Number(count))];
        assert_eq!(
            format(Locale::En, key::TRAY_FILES_TIDIED, &n(1)).as_deref(),
            Some("1 file tidied")
        );
        assert_eq!(
            format(Locale::En, key::TRAY_FILES_TIDIED, &n(1284)).as_deref(),
            Some("1,284 files tidied")
        );
        assert_eq!(
            format(Locale::Nl, key::TRAY_FILES_TIDIED, &n(1)).as_deref(),
            Some("1 bestand opgeruimd")
        );
        assert_eq!(
            format(Locale::Nl, key::TRAY_FILES_TIDIED, &n(1284)).as_deref(),
            Some("1.284 bestanden opgeruimd")
        );
    }

    #[test]
    fn apostrophes_are_literal_but_quoting_is_rejected() {
        let none: &[(&str, Arg)] = &[];
        assert_eq!(
            render("foto's", Locale::Nl, none, None).as_deref(),
            Some("foto's")
        );
        assert_eq!(render("it''s", Locale::En, none, None), None);
        assert_eq!(render("'{x}'", Locale::En, none, None), None);
    }

    #[test]
    fn unknown_keys_give_none() {
        assert_eq!(format(Locale::En, "tray.nope", &[]), None);
    }

    /// Renders every message with every argument set to 0, 1, 2 and 1284, which reaches each
    /// plural branch the catalog uses (`=0`, `one`, `other`).
    #[test]
    fn every_message_uses_only_the_supported_syntax() {
        for (lang, key, message) in MESSAGES {
            let locale = match *lang {
                "en" => Locale::En,
                "nl" => Locale::Nl,
                _ => panic!("locales/{lang}/tray.json: add {lang} to Locale"),
            };
            let names: Vec<&str> = message
                .split('{')
                .skip(1)
                .filter_map(|s| s.split([',', '}']).next())
                .map(str::trim)
                .filter(|name| !name.is_empty())
                .collect();
            for n in [0, 1, 2, 1284] {
                let args: Vec<_> = names.iter().map(|name| (*name, Arg::Number(n))).collect();
                assert!(
                    render(message, locale, &args, None).is_some(),
                    "{lang}/{key}: unsupported ICU in {message:?} for {n}"
                );
            }
        }
    }
}
