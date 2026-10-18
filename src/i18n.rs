use std::collections::HashMap;

pub const LOCALES: [&str; 2] = ["en", "zh-TW"];

/// Embedded string catalogs, one flat TOML table per locale.
pub struct I18n {
    catalogs: HashMap<&'static str, HashMap<String, String>>,
}

#[derive(Clone, Copy)]
pub struct T<'a> {
    pub locale: &'static str,
    strings: &'a HashMap<String, String>,
}

impl<'a> T<'a> {
    pub fn s(&self, key: &str) -> &'a str {
        match self.strings.get(key) {
            Some(v) => v,
            None => {
                tracing::error!("missing string {key}");
                ""
            }
        }
    }
}

impl I18n {
    pub fn load() -> Self {
        let parse = |src: &str| toml::from_str::<HashMap<String, String>>(src).expect("valid catalog");
        let catalogs = HashMap::from([
            ("en", parse(include_str!("../i18n/en.toml"))),
            ("zh-TW", parse(include_str!("../i18n/zh-TW.toml"))),
        ]);
        I18n { catalogs }
    }

    pub fn t(&self, locale: &str) -> T<'_> {
        let locale = LOCALES.into_iter().find(|l| *l == locale).unwrap_or("en");
        T { locale, strings: &self.catalogs[locale] }
    }
}

/// Picks the best supported locale from an `Accept-Language` header.
pub fn negotiate(accept_language: Option<&str>) -> &'static str {
    let mut best = ("en", 0.0_f32);
    for part in accept_language.unwrap_or("").split(',') {
        let mut it = part.split(';');
        let tag = it.next().unwrap_or("").trim().to_ascii_lowercase();
        let q = it.find_map(|p| p.trim().strip_prefix("q=").and_then(|v| v.parse().ok())).unwrap_or(1.0);
        let locale = match tag.as_str() {
            t if t == "zh" || t.starts_with("zh-") => "zh-TW",
            t if t == "en" || t.starts_with("en-") => "en",
            _ => continue,
        };
        if q > best.1 {
            best = (locale, q);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalogs_have_the_same_keys() {
        let i = I18n::load();
        let mut en: Vec<_> = i.catalogs["en"].keys().collect();
        let mut zh: Vec<_> = i.catalogs["zh-TW"].keys().collect();
        en.sort();
        zh.sort();
        assert_eq!(en, zh);
    }

    #[test]
    fn negotiation() {
        assert_eq!(negotiate(Some("zh-TW,zh;q=0.9,en;q=0.8")), "zh-TW");
        assert_eq!(negotiate(Some("en-US,zh;q=0.5")), "en");
        assert_eq!(negotiate(Some("fr-FR")), "en");
        assert_eq!(negotiate(None), "en");
        assert_eq!(negotiate(Some("fr;q=1, zh-Hant;q=0.7")), "zh-TW");
    }
}
