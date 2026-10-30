// Copyright 2026 Tsukumoe Contributors
// SPDX-License-Identifier: Apache-2.0 OR MIT

use std::sync::Arc;

use egui::{Context, Id};
use tsukumoe_i18n::{Arg, I18n, Language};

fn key() -> Id {
    Id::NULL.with("tsukumoe-ui-i18n")
}

fn messages(ctx: &Context) -> Arc<I18n> {
    if let Some(i18n) = ctx.data(|d| d.get_temp::<Arc<I18n>>(key())) {
        return i18n;
    }
    let i18n = Arc::new(I18n::new(Language::default()));
    ctx.data_mut(|d| d.insert_temp(key(), i18n.clone()));
    i18n
}

/// Switches the language of every [`tr`] call on `ctx`; English until set.
pub fn set_language(ctx: &Context, language: Language) {
    ctx.data_mut(|d| d.insert_temp(key(), Arc::new(I18n::new(language))));
}

/// The language set by [`set_language`].
pub fn language(ctx: &Context) -> Language {
    messages(ctx).language()
}

/// The message for `key` in the current language.
pub fn tr(ctx: &Context, key: &str) -> String {
    messages(ctx).tr(key)
}

/// The message for `key` with `args` filled in.
pub fn tr_args(ctx: &Context, key: &str, args: &[(&str, Arg)]) -> String {
    messages(ctx).tr_args(key, args)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_language_set_on_the_context_decides_the_message() {
        let ctx = Context::default();
        assert_eq!(tr(&ctx, "dialog-close"), "Close");
        set_language(&ctx, Language::Ja);
        assert_eq!(language(&ctx), Language::Ja);
        assert_eq!(tr(&ctx, "dialog-close"), "閉じる");
    }
}
