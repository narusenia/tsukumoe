// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

//! UI translations. Messages are Fluent (`locales/<code>/main.ftl`), embedded
//! in the binary and looked up by key. A key missing in the chosen language
//! falls back to English, then to the key itself, so a gap shows on screen.
//!
//! Keys are Fluent identifiers: lowercase words joined by `-`, area first
//! (`dialog-close`, `host-reset`).

use fluent_bundle::concurrent::FluentBundle;
use fluent_bundle::{FluentArgs, FluentResource, FluentValue};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use unic_langid::LanguageIdentifier;

/// A language the UI is translated into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    Ja,
}

impl Language {
    pub const ALL: [Self; 2] = [Self::En, Self::Ja];

    /// The BCP 47 code, also the name of its `locales` folder.
    pub fn code(self) -> &'static str {
        match self {
            Self::En => "en",
            Self::Ja => "ja",
        }
    }

    /// The language's name in itself, for a language picker.
    pub fn native_name(self) -> &'static str {
        match self {
            Self::En => "English",
            Self::Ja => "日本語",
        }
    }

    /// The language a tag such as `ja`, `ja-JP` or `en_US` belongs to.
    pub fn from_tag(tag: &str) -> Option<Self> {
        let primary = tag.split(['-', '_']).next()?.to_ascii_lowercase();
        Self::ALL.into_iter().find(|l| l.code() == primary)
    }

    /// The first supported language among the OS's preferred ones; English
    /// when none is supported or the OS gives none.
    pub fn detect() -> Self {
        sys_locale::get_locales()
            .find_map(|tag| Self::from_tag(&tag))
            .unwrap_or_default()
    }

    fn source(self) -> &'static str {
        match self {
            Self::En => include_str!("../locales/en/main.ftl"),
            Self::Ja => include_str!("../locales/ja/main.ftl"),
        }
    }
}

/// A value to put into a message.
#[derive(Clone, Debug, PartialEq)]
pub enum Arg {
    Text(String),
    Number(f64),
}

impl From<&str> for Arg {
    fn from(s: &str) -> Self {
        Self::Text(s.to_owned())
    }
}

impl From<f64> for Arg {
    fn from(n: f64) -> Self {
        Self::Number(n)
    }
}

impl From<usize> for Arg {
    fn from(n: usize) -> Self {
        Self::Number(n as f64)
    }
}

type Bundle = FluentBundle<FluentResource>;

fn bundle(language: Language) -> Bundle {
    let id: LanguageIdentifier = language.code().parse().expect("language codes are valid");
    let mut bundle = FluentBundle::new_concurrent(vec![id]);
    // Isolation marks around arguments would show up as stray glyphs in egui.
    bundle.set_use_isolating(false);
    let resource = FluentResource::try_new(language.source().to_owned())
        .unwrap_or_else(|(resource, _)| resource);
    // A duplicate id keeps the first message; the tests reject duplicates.
    let _ = bundle.add_resource(resource);
    bundle
}

/// The messages of one language, with English behind it.
pub struct I18n {
    language: Language,
    chosen: Bundle,
    english: Bundle,
}

impl I18n {
    pub fn new(language: Language) -> Self {
        Self {
            language,
            chosen: bundle(language),
            english: bundle(Language::En),
        }
    }

    pub fn language(&self) -> Language {
        self.language
    }

    /// The message for `key`.
    pub fn tr(&self, key: &str) -> String {
        self.tr_args(key, &[])
    }

    /// The message for `key` with `args` filled in.
    pub fn tr_args(&self, key: &str, args: &[(&str, Arg)]) -> String {
        let mut fluent = FluentArgs::new();
        for (name, arg) in args {
            match arg {
                Arg::Text(s) => fluent.set(*name, FluentValue::from(s.as_str())),
                Arg::Number(n) => fluent.set(*name, FluentValue::from(*n)),
            }
        }
        [&self.chosen, &self.english]
            .into_iter()
            .find_map(|bundle| {
                let pattern = bundle.get_message(key)?.value()?;
                let mut errors = Vec::new();
                Some(
                    bundle
                        .format_pattern(pattern, Some(&fluent), &mut errors)
                        .into_owned(),
                )
            })
            .unwrap_or_else(|| key.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use fluent_syntax::ast::{
        Entry, Expression, InlineExpression, Pattern, PatternElement, Variant,
    };
    use fluent_syntax::parser;

    use super::*;

    /// Message id → the variables its pattern (and every variant) uses.
    fn messages(language: Language) -> BTreeMap<String, BTreeSet<String>> {
        let resource = parser::parse(language.source())
            .unwrap_or_else(|(_, e)| panic!("{}: {e:?}", language.code()));
        let mut out = BTreeMap::new();
        for entry in &resource.body {
            if let Entry::Message(m) = entry {
                let mut vars = BTreeSet::new();
                if let Some(p) = &m.value {
                    collect(p, &mut vars);
                }
                assert!(
                    out.insert(m.id.name.to_owned(), vars).is_none(),
                    "{}",
                    m.id.name
                );
            }
        }
        out
    }

    fn collect(pattern: &Pattern<&str>, vars: &mut BTreeSet<String>) {
        for element in &pattern.elements {
            if let PatternElement::Placeable { expression } = element {
                expression_vars(expression, vars);
            }
        }
    }

    fn expression_vars(expression: &Expression<&str>, vars: &mut BTreeSet<String>) {
        match expression {
            Expression::Inline(i) => inline_vars(i, vars),
            Expression::Select { selector, variants } => {
                inline_vars(selector, vars);
                for Variant { value, .. } in variants {
                    collect(value, vars);
                }
            }
        }
    }

    fn inline_vars(inline: &InlineExpression<&str>, vars: &mut BTreeSet<String>) {
        if let InlineExpression::VariableReference { id } = inline {
            vars.insert(id.name.to_owned());
        }
    }

    #[test]
    fn every_language_has_the_same_keys_and_arguments() {
        let english = messages(Language::En);
        for language in Language::ALL {
            let other = messages(language);
            assert_eq!(
                english.keys().collect::<Vec<_>>(),
                other.keys().collect::<Vec<_>>(),
                "keys of {}",
                language.code()
            );
            assert_eq!(english, other, "arguments of {}", language.code());
        }
    }

    #[test]
    fn a_message_is_looked_up_in_the_chosen_language() {
        assert_eq!(I18n::new(Language::En).tr("dialog-close"), "Close");
        assert_eq!(I18n::new(Language::Ja).tr("dialog-close"), "閉じる");
    }

    #[test]
    fn arguments_and_plurals_are_filled_in() {
        let en = I18n::new(Language::En);
        assert_eq!(
            en.tr_args("host-parameters", &[("count", 1usize.into())]),
            "1 parameter"
        );
        assert_eq!(
            en.tr_args("host-parameters", &[("count", 3usize.into())]),
            "3 parameters"
        );
        let ja = I18n::new(Language::Ja);
        assert_eq!(
            ja.tr_args("host-parameters", &[("count", 3usize.into())]),
            "パラメータ 3 個"
        );
    }

    #[test]
    fn a_missing_key_falls_back_to_its_name() {
        assert_eq!(I18n::new(Language::Ja).tr("no-such-key"), "no-such-key");
    }

    #[test]
    fn tags_map_to_languages() {
        assert_eq!(Language::from_tag("ja-JP"), Some(Language::Ja));
        assert_eq!(Language::from_tag("en_US"), Some(Language::En));
        assert_eq!(Language::from_tag("fr"), None);
        assert_eq!(Language::from_tag(""), None);
    }
}
