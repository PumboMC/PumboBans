//! Host-independent parts of the Pumpkin layer: names, the config template,
//! the layer's messages, and how permission nodes look on Pumpkin.

use std::collections::HashMap;

use pumbo_bans_core::BansCfg;
use pumbo_bans_core::render::Langs;
use pumbo_common::config::{self, Warning};
use pumbo_common::lang::{Bundle, COMMON, Lang};

/// Name of the plugin in Pumpkin: namespace of its permissions
/// (`pumbobans:ban`), its data folder (`plugins/data/pumbobans`) and the
/// recipient name other plugins send messages to.
pub const PLUGIN_NAME: &str = "pumbobans";

/// Commented `config.yml` written on first start.
pub const CONFIG_TEMPLATE: &str = include_str!("../assets/config.yml");

/// Messages of the Pumpkin layer (prefix and platform notes).
pub const LANG: Bundle = Bundle {
    name: "bans-pumpkin",
    files: &[("en", include_str!("../lang/en.yml")), ("pl", include_str!("../lang/pl.yml"))],
};

/// Every message bundle, in load order.
pub const BUNDLES: [Bundle; 3] = [COMMON, pumbo_bans_core::LANG, LANG];

/// Pumpkin only accepts permission nodes in the plugin's namespace, so the
/// shared node `pumbo.bans.<action>` is registered as `pumbobans:<action>`.
pub fn pumpkin_node(node: &str) -> String {
    let prefix = format!("pumbo.{}.", pumbo_bans_core::ID);
    let action = node.strip_prefix(&prefix).unwrap_or(node);
    format!("{PLUGIN_NAME}:{action}")
}

/// Who gets a node without being given it: an operator level, or nobody.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Default {
    Op(u8),
    Nobody,
}

/// Default and description of a node (`pumbo.bans.<action>`).
pub fn node_info(node: &str) -> (Default, String) {
    let prefix = format!("pumbo.{}.", pumbo_bans_core::ID);
    let action = node.strip_prefix(&prefix).unwrap_or(node);
    let (default, what) = match action {
        a if a.starts_with("admin.") => {
            (Default::Op(4), format!("PumboBans administration: {}", a.trim_start_matches("admin.")))
        }
        "viewips" => (Default::Op(4), "See full IP addresses in /alts".into()),
        "exempt.bypass" => (Default::Op(4), "Punish players who are exempt".into()),
        a if a.starts_with("exempt.") => {
            (Default::Nobody, format!("Cannot be punished with {}", a.trim_start_matches("exempt.")))
        }
        a if a.starts_with("limit.") => {
            (Default::Nobody, format!("Length limits of group {}", a.trim_start_matches("limit.")))
        }
        "notify" => (Default::Op(3), "See punishments placed by staff".into()),
        "notify.silent" => (Default::Op(3), "See silent punishments too".into()),
        "notify.alts" => (Default::Op(3), "See when someone joins from a banned account's address".into()),
        a => (Default::Op(3), format!("Use /pumbobans {a}")),
    };
    (default, what)
}

/// Reads the language files (writing the bundled ones on first start): the
/// configured default plus every other language with a file or bundled text.
pub fn load_langs(dir: &str, cfg: &BansCfg) -> (Langs, Vec<Warning>) {
    let mut warnings = Vec::new();
    let lang_dir = format!("{dir}/lang");
    let mut codes: Vec<String> = Lang::builtin_codes(&BUNDLES).iter().map(|c| (*c).to_string()).collect();
    if let Ok(entries) = std::fs::read_dir(&lang_dir) {
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if let Some(code) = name.strip_suffix(".yml") {
                codes.push(code.to_string());
            }
        }
    }
    if !codes.contains(&cfg.language) {
        codes.push(cfg.language.clone());
    }
    codes.sort();
    codes.dedup();
    let mut all = HashMap::new();
    for code in &codes {
        let path = format!("{lang_dir}/{code}.yml");
        let (text, w) = config::read_or_create(&path, &Lang::template(&BUNDLES, code));
        warnings.extend(w);
        let (lang, w) = Lang::load(&BUNDLES, code, Some(&text));
        warnings.extend(w.into_iter().map(|w| Warning { message: format!("lang/{code}.yml: {}", w.message), ..w }));
        all.insert(code.clone(), lang);
    }
    let default = all.remove(&cfg.language).unwrap_or_else(|| Lang::load(&BUNDLES, "en", None).0);
    (Langs { default, others: all, per_player: cfg.per_player_language }, warnings)
}

/// Reads `config.yml`, writing the template on first start.
pub fn load_config(dir: &str) -> (BansCfg, Vec<Warning>) {
    let (text, w) = config::read_or_create(&format!("{dir}/config.yml"), CONFIG_TEMPLATE);
    let (cfg, mut warnings) = config::load::<BansCfg>(&text);
    warnings.extend(w);
    warnings.extend(config::old_files(dir));
    (cfg, warnings)
}

#[cfg(test)]
mod tests {
    use pumbo_common::lang::check_bundle;

    use super::*;

    #[test]
    fn template_is_the_default() {
        let (cfg, w) = config::load::<BansCfg>(CONFIG_TEMPLATE);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(cfg, BansCfg::default());
    }

    #[test]
    fn messages() {
        assert_eq!(check_bundle(&LANG), Vec::<String>::new());
        let (en, w) = Lang::load(&BUNDLES, "en", None);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(en.get("prefix"), pumbo_common::style::prefix_code("PumboBans"));
        let reply = pumbo_common::style::error(&en, "error-db", &pumbo_common::text::Args::new());
        assert!(reply.plain().starts_with("PumboBans » The punishment database"), "{}", reply.plain());
    }

    #[test]
    fn nodes() {
        assert_eq!(pumpkin_node("pumbo.bans.ban"), "pumbobans:ban");
        assert_eq!(pumpkin_node("pumbo.bans.exempt.ban"), "pumbobans:exempt.ban");
        assert_eq!(node_info("pumbo.bans.exempt.ban").0, Default::Nobody);
        assert_eq!(node_info("pumbo.bans.admin.reload").0, Default::Op(4));
        assert_eq!(node_info("pumbo.bans.ban").0, Default::Op(3));
    }

    #[test]
    fn files_are_written_and_read() {
        let dir = std::env::temp_dir().join(format!("pumbobans-setup-{}", std::process::id()));
        let dir = dir.to_string_lossy().to_string();
        let (cfg, w) = load_config(&dir);
        assert!(w.is_empty(), "{w:?}");
        assert!(std::path::Path::new(&format!("{dir}/config.yml")).exists());
        let mut cfg = cfg;
        cfg.language = "pl".into();
        let (langs, w) = load_langs(&dir, &cfg);
        assert!(w.is_empty(), "{w:?}");
        assert_eq!(langs.default.get("kind-warn"), "ostrzeżenie");
        assert!(langs.others.contains_key("en"));
        let _ = std::fs::remove_file(format!("{dir}/config.yml"));
        let _ = std::fs::remove_file(format!("{dir}/lang/en.yml"));
        let _ = std::fs::remove_file(format!("{dir}/lang/pl.yml"));
        let _ = std::fs::remove_dir(format!("{dir}/lang"));
        let _ = std::fs::remove_dir(&dir);
    }
}
