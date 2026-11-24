//! Ресурспаки зі збірки вмикаються в грі самі (options.txt → resourcePacks).
//! Новий пак у збірці вмикається один раз і стає найвищим за пріоритетом; якщо гравець потім
//! вимкне його в грі, лаунчер це поважає. Пак, який прибрали зі збірки, зникає зі списку.
use crate::manifest::Manifest;
use crate::paths;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct State {
    /// «selected», а не «enabled» з 1.4.4: та версія вмикала паки неправильно, тож вони ввімкнуться ще раз.
    selected: BTreeSet<String>,
}

pub fn apply(m: &Manifest) {
    let state: State = std::fs::read(paths::packs_state()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let packs: Vec<String> = m.files.iter().filter(|f| f.dir == "resourcepacks").map(|f| f.filename.clone()).collect();
    let options = paths::game_dir().join("options.txt");
    let text = std::fs::read_to_string(&options).unwrap_or_default();
    let (out, enabled) = update(&text, &packs, &state.selected);
    if out != text && write_atomic(&options, &out).is_err() {
        return; // наступного запуску спробуємо знову
    }
    if let Ok(b) = serde_json::to_vec_pretty(&State { selected: enabled }) {
        let _ = std::fs::write(paths::packs_state(), b);
    }
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("txt.altherra-tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

fn read_list(text: &str, key: &str) -> Option<Vec<String>> {
    let prefix = format!("{key}:");
    let line = text.lines().find(|l| l.starts_with(&prefix))?;
    serde_json::from_str(&line[prefix.len()..]).ok()
}

fn set_list(text: &str, key: &str, list: &[String]) -> String {
    let prefix = format!("{key}:");
    let value = format!("{prefix}{}", serde_json::to_string(list).unwrap_or_else(|_| "[]".into()));
    let mut found = false;
    let mut lines: Vec<String> = text
        .lines()
        .map(|l| {
            if l.starts_with(&prefix) {
                found = true;
                value.clone()
            } else {
                l.to_owned()
            }
        })
        .collect();
    if !found {
        lines.push(value);
    }
    let mut s = lines.join("\n");
    s.push('\n');
    s
}

/// Повертає новий options.txt і новий список «уже ввімкнених» паків.
fn update(text: &str, packs: &[String], done: &BTreeSet<String>) -> (String, BTreeSet<String>) {
    let mut selected = read_list(text, "resourcePacks").unwrap_or_else(|| vec!["vanilla".into()]);
    // Порожній список (навіть без "vanilla") лишає сама гра, коли пак не завантажився:
    // «Caught error loading resourcepacks, removing all selected». Гравець так зробити не може —
    // тоді вмикаємо паки збірки заново, ніби вперше.
    let reset = selected.is_empty();
    let empty = BTreeSet::new();
    let done = if reset {
        selected.push("vanilla".into());
        &empty
    } else {
        done
    };
    let mut incompatible = read_list(text, "incompatibleResourcePacks").unwrap_or_default();
    let before = (selected.clone(), incompatible.clone());
    let current: BTreeSet<&String> = packs.iter().collect();

    // Прибрані зі збірки — геть зі списків.
    for gone in done.iter().filter(|p| !current.contains(p)) {
        let id = format!("file/{gone}");
        selected.retain(|x| *x != id);
        incompatible.retain(|x| *x != id);
    }
    // Нові — вмикаємо поверх усього. У incompatibleResourcePacks не додаємо: якщо пак сумісний,
    // гра прибирає його звідти й через свою помилку заразом не вмикає.
    // Розширення (напр. «Fresh Animations: Extensions») мають стояти вище базового пака.
    let mut fresh: Vec<&String> = packs.iter().filter(|p| !done.contains(*p)).collect();
    fresh.sort_by_key(|p| p.to_lowercase().contains("extension"));
    for p in fresh {
        let id = format!("file/{p}");
        selected.retain(|x| *x != id);
        incompatible.retain(|x| *x != id);
        selected.push(id);
    }
    let enabled = packs.iter().cloned().collect();
    if (selected.clone(), incompatible.clone()) == before {
        return (text.to_owned(), enabled);
    }
    let out = set_list(text, "resourcePacks", &selected);
    (set_list(&out, "incompatibleResourcePacks", &incompatible), enabled)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(v: &[&str]) -> BTreeSet<String> {
        v.iter().map(|s| s.to_string()).collect()
    }
    fn packs(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn enables_new_pack_on_top_and_keeps_other_options() {
        let text = "version:4000\nresourcePacks:[\"vanilla\",\"fabric\",\"file/Mine.zip\"]\nfov:0.5\n";
        let (out, en) = update(text, &packs(&["Altherra.zip"]), &set(&[]));
        assert!(out.contains("resourcePacks:[\"vanilla\",\"fabric\",\"file/Mine.zip\",\"file/Altherra.zip\"]"));
        assert!(out.contains("incompatibleResourcePacks:[]"));
        assert!(out.starts_with("version:4000\n") && out.contains("fov:0.5\n"));
        assert_eq!(en, set(&["Altherra.zip"]));
    }

    #[test]
    fn respects_player_disabling_and_removes_dropped() {
        // Гравець вимкнув Altherra.zip — більше не вмикаємо.
        let text = "resourcePacks:[\"vanilla\"]\n";
        let (out, _) = update(text, &packs(&["Altherra.zip"]), &set(&["Altherra.zip"]));
        assert_eq!(out, text);
        // Пак прибрали зі збірки — зникає зі списку.
        let text = "resourcePacks:[\"vanilla\",\"file/Old.zip\"]\nincompatibleResourcePacks:[\"file/Old.zip\"]\n";
        let (out, en) = update(text, &packs(&[]), &set(&["Old.zip"]));
        assert_eq!(out, "resourcePacks:[\"vanilla\"]\nincompatibleResourcePacks:[]\n");
        assert!(en.is_empty());
    }

    #[test]
    fn old_state_from_1_4_4_enables_again() {
        let st: State = serde_json::from_str("{\"enabled\":[\"A.zip\"]}").unwrap();
        assert!(st.selected.is_empty());
    }

    #[test]
    fn reenables_after_game_reset_and_puts_extensions_on_top() {
        let text = "resourcePacks:[]\nincompatibleResourcePacks:[]\n";
        let (out, _) = update(text, &packs(&["FA_Extensions.zip", "FreshAnimations.zip"]), &set(&["FA_Extensions.zip"]));
        assert_eq!(out, "resourcePacks:[\"vanilla\",\"file/FreshAnimations.zip\",\"file/FA_Extensions.zip\"]\nincompatibleResourcePacks:[]\n");
    }

    #[test]
    fn creates_options_on_first_launch() {
        let (out, _) = update("", &packs(&["A.zip"]), &set(&[]));
        assert_eq!(out, "resourcePacks:[\"vanilla\",\"file/A.zip\"]\nincompatibleResourcePacks:[]\n");
    }
}
