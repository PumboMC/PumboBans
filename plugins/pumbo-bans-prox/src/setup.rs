//! Host-independent parts of the PumboProx layer: names, config and message
//! files, text as JSON components, what other plugins hear about punishments.

use std::collections::HashMap;

use pumbo_bans_core::render::Langs;
use pumbo_bans_core::{BansCfg, Kind, Punishment, Target, net};
use pumbo_common::config::{self, Warning};
use pumbo_common::lang::{Bundle, COMMON, Lang};
use pumbo_common::rich::Text;

/// Plugin id in the manifest (`pumbo-bans.yml`); the `pumbo:` topics of
/// PumboBans belong to it (`pumbo-contracts`).
pub const PLUGIN_ID: &str = "pumbo-bans";

/// Config written for the admin to copy into `plugins/pumbo-bans/` (the same
/// options as on Pumpkin).
pub const CONFIG_TEMPLATE: &str = include_str!("../../pumbo-bans-pumpkin/assets/config.yml");

/// Messages of the PumboProx layer (prefix).
pub const LANG: Bundle = Bundle {
    name: "bans-prox",
    files: &[("en", include_str!("../lang/en.yml")), ("pl", include_str!("../lang/pl.yml"))],
};

/// Every message bundle, in load order.
pub const BUNDLES: [Bundle; 3] = [COMMON, pumbo_bans_core::LANG, LANG];

/// Reads `config.yml` from the read-only config folder (`/config` in the
/// plugin); a missing file means the defaults.
pub fn load_config(dir: &str) -> (BansCfg, Vec<Warning>, bool) {
    let (cfg, mut w, found) = match std::fs::read_to_string(format!("{dir}/config.yml")) {
        Ok(text) => {
            let (cfg, w) = config::load::<BansCfg>(&text);
            (cfg, w, true)
        }
        Err(_) => (BansCfg::default(), Vec::new(), false),
    };
    w.extend(config::old_files(dir));
    (cfg, w, found)
}

/// The configured language and every other one with a bundled text or a file
/// in `lang/` (overrides of the bundled messages).
pub fn load_langs(dir: &str, cfg: &BansCfg) -> (Langs, Vec<Warning>) {
    let lang_dir = format!("{dir}/lang");
    let mut codes: Vec<String> = Lang::builtin_codes(&BUNDLES).iter().map(|c| (*c).to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(&lang_dir) {
        for e in entries.flatten() {
            if let Some(code) = e.file_name().to_string_lossy().strip_suffix(".yml") {
                codes.push(code.to_string());
            }
        }
    }
    codes.push(cfg.language.clone());
    codes.sort();
    codes.dedup();
    let mut warnings = Vec::new();
    let mut all = HashMap::new();
    for code in &codes {
        let user = std::fs::read_to_string(format!("{lang_dir}/{code}.yml")).ok();
        let (lang, w) = Lang::load(&BUNDLES, code, user.as_deref());
        warnings.extend(w.into_iter().map(|w| Warning { message: format!("lang/{code}.yml: {}", w.message), ..w }));
        all.insert(code.clone(), lang);
    }
    let default = all.remove(&cfg.language).unwrap_or_else(|| Lang::load(&BUNDLES, "en", None).0);
    (Langs { default, others: all, per_player: cfg.per_player_language }, warnings)
}

/// Rich text as a JSON text component (the proxy encodes it for each client
/// version).
pub fn json(t: &Text) -> String {
    pumbo_common::rich::json(t)
}

/// A punishment as other plugins hear about it (`pumbo:player-punished@1.0`,
/// `pumbo:punishment-revoked@1.0`).
pub fn contract(p: &Punishment) -> pumbo_sdk::contracts::Punishment {
    use pumbo_sdk::contracts::PunishmentKind as K;
    let kind = match (p.kind, p.target.is_address()) {
        (Kind::Ban, true) => K::IpBan,
        (Kind::Ban, false) => K::Ban,
        (Kind::Mute, _) => K::Mute,
        (Kind::Warn, _) => K::Warn,
        (Kind::Kick, _) => K::Kick,
    };
    let target = match &p.target {
        Target::Player { uuid } => uuid.simple(),
        Target::Address { net } => net::label(net),
        // A name punishment points at the account it was linked to, when known.
        Target::Name { name } => p.victim_uuid.map(|u| u.simple()).unwrap_or_else(|| name.clone()),
    };
    pumbo_sdk::contracts::Punishment {
        id: p.id,
        kind,
        target,
        until: p.expires.map(|e| e / 1000),
        author: p.operator.name.clone(),
        reason: Some(p.reason.clone()).filter(|r| !r.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use pumbo_common::lang::check_bundle;
    use pumbo_common::style;
    use pumbo_common::text::Args;

    use super::*;

    #[test]
    fn built_in_language_files_are_current() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/lang");
        let written = Lang::write_templates(&BUNDLES, &dir);
        assert!(written.is_empty(), "rewritten from the message bundles, commit them: {written:?}");
    }

    #[test]
    fn template_is_the_default() {
        let (cfg, w) = config::load::<BansCfg>(CONFIG_TEMPLATE);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg, BansCfg::default());
    }

    #[test]
    fn messages_and_files() {
        assert_eq!(check_bundle(&LANG), Vec::<String>::new());
        let dir = std::env::temp_dir().join(format!("pumbo-bans-prox-{}", std::process::id()));
        let _ = std::fs::create_dir_all(dir.join("lang"));
        let d = dir.to_string_lossy().to_string();
        let (cfg, w, found) = load_config(&d);
        assert!(w.is_empty() && !found && cfg == BansCfg::default());
        std::fs::write(dir.join("config.yml"), "language: pl\n").unwrap();
        std::fs::write(dir.join("lang/pl.yml"), "kind-warn: upomnienie\n").unwrap();
        let (cfg, w, found) = load_config(&d);
        assert!(w.is_empty() && found);
        let (langs, w) = load_langs(&d, &cfg);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(langs.default.get("kind-warn"), "upomnienie");
        assert_eq!(langs.for_locale(Some("en_us")).get("kind-warn"), "warning");
        let reply = style::error(&langs.default, "error-db", &Args::new());
        assert!(reply.plain().starts_with("PumboBans » "), "{}", reply.plain());
        let _ = std::fs::remove_file(dir.join("lang/pl.yml"));
        let _ = std::fs::remove_file(dir.join("config.yml"));
        let _ = std::fs::remove_dir(dir.join("lang"));
        let _ = std::fs::remove_dir(&dir);
    }
}
