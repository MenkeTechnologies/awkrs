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
    let lang_major = lang_short
        .split('_')
        .next()
        .unwrap_or("C")
        .to_string();

    let base = Path::new(dirname);
    let mo_name = format!("{domain}.mo");
    let candidates: Vec<PathBuf> = vec![
        base.join(&lang).join("LC_MESSAGES").join(&mo_name),
        base.join("locale").join(&lang).join("LC_MESSAGES").join(&mo_name),
        base.join("locale").join(&lang_short).join("LC_MESSAGES").join(&mo_name),
        base.join("locale").join(&lang_major).join("LC_MESSAGES").join(&mo_name),
        base.join("locale").join("C").join("LC_MESSAGES").join(&mo_name),
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
