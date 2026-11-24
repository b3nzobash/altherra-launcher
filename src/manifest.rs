//! Маніфест збірки з демона сервера і синхронізація модів, ресурспаків та конфігів.
use crate::net::{self, Download};
use crate::paths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Адреса демона. Можна перевизначити під час збирання: ALTHERRA_API=https://… cargo build
pub const API_URL: &str = match option_env!("ALTHERRA_API") {
    Some(u) => u,
    None => "https://launcher.altherra.com.ua:8443",
};

pub fn api_url() -> String {
    std::env::var("ALTHERRA_API").unwrap_or_else(|_| API_URL.to_owned()).trim_end_matches('/').to_owned()
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Manifest {
    pub revision: u64,
    pub name: String,
    pub server_address: String,
    pub site_url: String,
    pub minecraft: String,
    pub loader: Loader,
    pub ms_client_id: String,
    pub recommended_ram_mb: u32,
    pub jvm_args: String,
    pub strict_mods: bool,
    pub files: Vec<FileEntry>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Loader {
    #[serde(rename = "type")]
    pub kind: String,
    pub version: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileEntry {
    pub name: String,
    pub dir: String,
    pub filename: String,
    pub sha1: String,
    pub size: u64,
    pub url: String,
    pub fallback: Option<String>,
    /// Обовʼязковий ресурспак: вмикаємо при кожному запуску, навіть якщо гравець вимкнув.
    pub required: bool,
}

const DIRS: [&str; 4] = ["mods", "resourcepacks", "shaderpacks", "config"];

/// Відбиток збірки: SHA-1 від відсортованих рядків «тека/файл:sha1» усіх файлів маніфесту.
/// Гра повідомляє його серверу під час входу, а сервер рахує так само зі свого маніфесту:
/// якщо відбитки різні — гравець грає зі старою збіркою (моди, текстури, шейдери чи конфіги).
pub fn fingerprint(files: &[FileEntry]) -> String {
    use sha1::{Digest, Sha1};
    let mut lines: Vec<String> = files.iter().map(|f| format!("{}/{}:{}", f.dir, f.filename, f.sha1.to_ascii_lowercase())).collect();
    lines.sort();
    hex::encode(Sha1::digest(lines.join("\n").as_bytes()))
}

#[cfg(test)]
mod fingerprint_tests {
    use super::*;
    /// sha1("mods/a.jar:aa") — те саме значення має порахувати сервер (Java).
    const FP_A: &str = "546ae3f6237f7d2814bcd9f8e8968ac6b5972e4d";
    fn f(dir: &str, name: &str, sha: &str) -> FileEntry {
        FileEntry { name: name.into(), dir: dir.into(), filename: name.into(), sha1: sha.into(), size: 1, url: String::new(), fallback: None, required: false }
    }
    #[test]
    fn order_does_not_matter_but_content_does() {
        let a = fingerprint(&[f("mods", "a.jar", "AA"), f("config", "b.json", "bb")]);
        let b = fingerprint(&[f("config", "b.json", "bb"), f("mods", "a.jar", "aa")]);
        assert_eq!(a, b);
        assert_ne!(a, fingerprint(&[f("mods", "a.jar", "aa"), f("config", "b.json", "cc")]));
        assert_eq!(fingerprint(&[f("mods", "a.jar", "aa")]), FP_A);
    }
}

/// Свіжий маніфест; якщо сервер недоступний — останній збережений.
pub async fn fetch() -> Result<(Manifest, bool)> {
    let api = api_url();
    if !net::allowed_url(&api) {
        anyhow::bail!("адреса сервера лаунчера має бути https://");
    }
    match net::get_json::<Manifest>(&format!("{api}/api/manifest")).await {
        Ok(m) => {
            if let Ok(bytes) = serde_json::to_vec_pretty(&m) {
                let _ = std::fs::create_dir_all(paths::root());
                let _ = std::fs::write(paths::manifest_cache(), bytes);
            }
            Ok((m, true))
        }
        Err(err) => {
            let cached = std::fs::read(paths::manifest_cache()).map_err(|_| err)?;
            Ok((serde_json::from_slice(&cached).context("пошкоджений збережений маніфест")?, false))
        }
    }
}

fn resolve(url: &str) -> String {
    if url.starts_with('/') { format!("{}{}", api_url(), url) } else { url.to_owned() }
}

/// Назва файлу з маніфесту: без шляхів і символів, які Windows трактує особливо.
fn safe_name(s: &str) -> bool {
    const RESERVED: [&str; 22] = ["CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9"];
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    !s.is_empty()
        && s.len() <= 180
        && !s.starts_with('.')
        && !s.ends_with(['.', ' '])
        && !s.chars().any(|c| c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'))
        && !RESERVED.contains(&stem.as_str())
}

/// Відносний шлях виду «теки/файл» лише в дозволених теках.
fn safe_managed(rel: &str) -> bool {
    rel.split_once('/').is_some_and(|(dir, name)| DIRS.contains(&dir) && safe_name(name))
}

/// Що лаунчер сам поставив минулого разу: шлях → sha1.
#[derive(Default, Serialize, Deserialize)]
struct SyncState {
    files: HashMap<String, String>,
}

/// Список файлів для скачування + видалення тих, яких більше немає в збірці.
pub fn plan_sync(m: &Manifest) -> Result<Vec<Download>> {
    let game = paths::game_dir();
    let state: SyncState = std::fs::read(paths::sync_state()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    let mut wanted: HashMap<String, &FileEntry> = HashMap::new();
    let mut downloads = Vec::new();

    for f in &m.files {
        if !DIRS.contains(&f.dir.as_str()) || !safe_name(&f.filename) {
            continue;
        }
        let rel = format!("{}/{}", f.dir, f.filename);
        let path = game.join(&f.dir).join(&f.filename);
        // Конфіги гравець може міняти — перезаписуємо їх лише тоді, коли змінилися на сервері.
        if f.dir == "config" && path.exists() && state.files.get(&rel) == Some(&f.sha1) {
            wanted.insert(rel, f);
            continue;
        }
        let mut urls = vec![resolve(&f.url)];
        if let Some(fb) = &f.fallback {
            urls.push(resolve(fb));
        }
        downloads.push(Download { urls, path, sha1: Some(f.sha1.clone()), size: Some(f.size), executable: false });
        wanted.insert(rel, f);
    }

    // Прибираємо файли, які лаунчер ставив раніше, але їх прибрали зі збірки.
    for rel in state.files.keys() {
        if !wanted.contains_key(rel) && safe_managed(rel) {
            let _ = std::fs::remove_file(game.join(rel));
        }
    }
    // Суворий режим: у mods лишається тільки збірка.
    if m.strict_mods {
        if let Ok(rd) = std::fs::read_dir(game.join("mods")) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if e.path().is_file() && !wanted.contains_key(&format!("mods/{name}")) {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
    }
    Ok(downloads)
}

pub fn save_sync_state(m: &Manifest) {
    let files = m
        .files
        .iter()
        .filter(|f| DIRS.contains(&f.dir.as_str()) && safe_name(&f.filename))
        .map(|f| (format!("{}/{}", f.dir, f.filename), f.sha1.clone()))
        .collect();
    if let Ok(b) = serde_json::to_vec_pretty(&SyncState { files }) {
        let _ = std::fs::write(paths::sync_state(), b);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(safe_name("sodium-fabric-0.9.2+mc26.3.jar"));
        assert!(!safe_name("../x.jar"));
        assert!(!safe_name("a/b.jar"));
        assert!(!safe_name("a\\b.jar"));
        assert!(!safe_name("CON.jar"));
        assert!(!safe_name("x.jar."));
        assert!(!safe_name(".hidden"));
        assert!(safe_managed("mods/a.jar"));
        assert!(!safe_managed("saves/a.jar"));
        assert!(!safe_managed("mods/../../a"));
    }
}
