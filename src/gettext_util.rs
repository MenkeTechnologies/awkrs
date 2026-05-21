//! Load GNU `.mo` catalogs (pure Rust) for `dcgettext` / `dcngettext` after `bindtextdomain`.

use gettext::Catalog;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Try to open a `.mo` for `domain` under `dirname` using common locale directory layouts.
pub fn try_load_gettext_catalog(domain: &str, dirname: &str) -> Option<Arc<Catalog>> {
    let lang = std::env::var("LANGUAGE")
        .ok()
        .and_then(|s| s.split(':').next().map(|x| x.to_string()))
        .or_else(|| std::env::var("LC_ALL").ok())
        .or_else(|| std::env::var("LC_MESSAGES").ok())
        .or_else(|| std::env::var("LANG").ok())
        .unwrap_or_else(|| "C".into());
    let lang_short = lang.split('.').next().unwrap_or("C").to_string();
    let lang_major = lang_short.split('_').next().unwrap_or("C").to_string();

    let base = Path::new(dirname);
    let mo_name = format!("{domain}.mo");
    let candidates: Vec<PathBuf> = vec![
        base.join(&lang).join("LC_MESSAGES").join(&mo_name),
        base.join("locale")
            .join(&lang)
            .join("LC_MESSAGES")
            .join(&mo_name),
        base.join("locale")
            .join(&lang_short)
            .join("LC_MESSAGES")
            .join(&mo_name),
        base.join("locale")
            .join(&lang_major)
            .join("LC_MESSAGES")
            .join(&mo_name),
        base.join("locale")
            .join("C")
            .join("LC_MESSAGES")
            .join(&mo_name),
    ];

    for p in candidates {
        if let Ok(f) = std::fs::File::open(&p) {
            if let Ok(cat) = Catalog::parse(f) {
                return Some(Arc::new(cat));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_load_missing_catalog_returns_none() {
        let tmp = std::env::temp_dir().join(format!(
            "awkrs_no_mo_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        assert!(
            try_load_gettext_catalog("nonexistent_domain", tmp.to_string_lossy().as_ref())
                .is_none()
        );
    }

    #[test]
    fn try_load_with_env_vars_does_not_panic() {
        // We can't easily test actual loading without a real .mo file,
        // but we can test that the fallback logic handles env vars.
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        let old_lang = std::env::var("LANG").ok();
        std::env::set_var("LANG", "en_US.UTF-8");
        let res = try_load_gettext_catalog("domain", "/tmp");
        assert!(res.is_none());
        if let Some(l) = old_lang {
            std::env::set_var("LANG", l);
        } else {
            std::env::remove_var("LANG");
        }
    }

    #[test]
    fn try_load_language_priority_v2() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        // Set LANGUAGE which should take priority over LANG
        std::env::set_var("LANGUAGE", "fr_FR:de_DE");
        std::env::set_var("LANG", "en_US.UTF-8");
        let res = try_load_gettext_catalog("domain", "/tmp");
        assert!(res.is_none());
        std::env::remove_var("LANGUAGE");
    }

    #[test]
    fn try_load_lc_messages_priority_v2() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        // Set LC_MESSAGES which should take priority over LANG
        std::env::set_var("LC_MESSAGES", "es_ES.UTF-8");
        std::env::set_var("LANG", "en_US.UTF-8");
        let res = try_load_gettext_catalog("domain", "/tmp");
        assert!(res.is_none());
        std::env::remove_var("LC_MESSAGES");
    }

    #[test]
    fn try_load_fallback_to_c_v2() {
        let _g = crate::test_sync::ENV_LOCK.lock().unwrap();
        // Clear all env vars
        std::env::remove_var("LANGUAGE");
        std::env::remove_var("LC_ALL");
        std::env::remove_var("LC_MESSAGES");
        std::env::remove_var("LANG");
        let res = try_load_gettext_catalog("domain", "/tmp");
        assert!(res.is_none());
    }

    #[test]
    fn gettext_empty_domain_v25() {
        assert!(try_load_gettext_catalog("", "/tmp").is_none());
    }
    #[test]
    fn gettext_invalid_path_v25() {
        assert!(try_load_gettext_catalog("d", "/invalid/path").is_none());
    }
}
