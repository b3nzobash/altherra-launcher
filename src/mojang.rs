//! Версії Minecraft, бібліотеки, ассети, Java від Mojang і профіль Fabric.
use crate::net::{self, Download};
use crate::paths;
use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

const VERSION_MANIFEST: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const JAVA_MANIFEST: &str = "https://launchermeta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const RESOURCES: &str = "https://resources.download.minecraft.net";

// ---------- Платформа ----------

pub fn os_name() -> &'static str {
    if cfg!(windows) { "windows" } else if cfg!(target_os = "macos") { "osx" } else { "linux" }
}

fn java_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "aarch64") => "windows-arm64",
        ("windows", "x86") => "windows-x86",
        ("windows", _) => "windows-x64",
        ("macos", "aarch64") => "mac-os-arm64",
        ("macos", _) => "mac-os",
        (_, "x86") => "linux-i386",
        _ => "linux",
    }
}

/// Правила з version.json: чи стосується елемент цієї системи й увімкнених функцій.
pub fn rules_allow(rules: Option<&Value>, features: &HashMap<&str, bool>) -> bool {
    let Some(rules) = rules.and_then(Value::as_array) else { return true };
    let mut allowed = false;
    for rule in rules {
        let mut matches = true;
        if let Some(os) = rule.get("os") {
            if let Some(name) = os.get("name").and_then(Value::as_str) {
                matches &= name == os_name();
            }
            if let Some(arch) = os.get("arch").and_then(Value::as_str) {
                matches &= match arch {
                    "x86" => cfg!(target_arch = "x86"),
                    "arm64" => cfg!(target_arch = "aarch64"),
                    _ => true,
                };
            }
        }
        if let Some(feats) = rule.get("features").and_then(Value::as_object) {
            for (k, v) in feats {
                matches &= features.get(k.as_str()).copied().unwrap_or(false) == v.as_bool().unwrap_or(false);
            }
        }
        if matches {
            allowed = rule.get("action").and_then(Value::as_str) == Some("allow");
        }
    }
    allowed
}

// ---------- version.json ----------

/// Ідентифікатор версії стає частиною шляху й адреси, тож пускаємо лише безпечні символи.
pub fn valid_id(s: &str) -> bool {
    (1..=64).contains(&s.len()) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-' | '+')) && !s.starts_with('.')
}

/// Готовий до запуску профіль: ванільний version.json, за потреби злитий із профілем Fabric.
pub struct Profile {
    pub id: String,
    pub vanilla_id: String,
    pub main_class: String,
    pub json: Value,           // ванільний
    pub extra: Option<Value>,  // профіль лоадера
}

#[derive(Deserialize)]
struct ManifestV2 {
    versions: Vec<ManifestEntry>,
}
#[derive(Deserialize)]
struct ManifestEntry {
    id: String,
    url: String,
    sha1: String,
}

async fn cached_json(path: PathBuf, url: &str, sha1: Option<&str>) -> Result<Value> {
    if path.exists() {
        let ok = sha1.map(|s| net::sha1_file(&path).map(|h| h == s).unwrap_or(false)).unwrap_or(true);
        if ok {
            if let Ok(v) = serde_json::from_slice(&tokio::fs::read(&path).await?) {
                return Ok(v);
            }
        }
    }
    let d = Download { urls: vec![url.to_owned()], path: path.clone(), sha1: sha1.map(str::to_owned), size: None, executable: false };
    net::download(&d, &Default::default()).await?;
    Ok(serde_json::from_slice(&tokio::fs::read(&path).await?)?)
}

pub async fn load_profile(mc: &str, loader: &str, loader_version: &str) -> Result<Profile> {
    if !valid_id(mc) {
        bail!("сервер вказав неправильну версію гри");
    }
    if !loader_version.is_empty() && !valid_id(loader_version) {
        bail!("сервер вказав неправильну версію Fabric");
    }
    let vanilla_path = paths::versions().join(mc).join(format!("{mc}.json"));
    // Без інтернету працюємо з тим, що вже скачано.
    let json = match net::get_json::<ManifestV2>(VERSION_MANIFEST).await {
        Ok(m) => {
            let e = m.versions.into_iter().find(|v| v.id == mc).ok_or_else(|| anyhow!("Mojang не знає версію {mc}"))?;
            cached_json(vanilla_path, &e.url, Some(&e.sha1)).await?
        }
        Err(err) if vanilla_path.exists() => serde_json::from_slice(&tokio::fs::read(&vanilla_path).await?).context(err)?,
        Err(err) => return Err(err),
    };

    let (id, extra) = if loader == "fabric" {
        let lv = if loader_version.is_empty() { latest_fabric(mc).await? } else { loader_version.to_owned() };
        if !valid_id(&lv) {
            bail!("неправильна версія Fabric");
        }
        let id = format!("fabric-loader-{lv}-{mc}");
        let path = paths::versions().join(&id).join(format!("{id}.json"));
        let url = format!("https://meta.fabricmc.net/v2/versions/loader/{mc}/{lv}/profile/json");
        let profile = match cached_json(path.clone(), &url, None).await {
            Ok(p) => p,
            Err(e) => return Err(e.context("не вдалося отримати профіль Fabric")),
        };
        (id, Some(profile))
    } else {
        (mc.to_owned(), None)
    };

    let main_class = extra
        .as_ref()
        .and_then(|e| e["mainClass"].as_str())
        .or_else(|| json["mainClass"].as_str())
        .ok_or_else(|| anyhow!("у version.json немає mainClass"))?
        .to_owned();
    Ok(Profile { id, vanilla_id: mc.to_owned(), main_class, json, extra })
}

async fn latest_fabric(mc: &str) -> Result<String> {
    let list: Vec<Value> = net::get_json(&format!("https://meta.fabricmc.net/v2/versions/loader/{mc}")).await?;
    list.iter()
        .find(|e| e["loader"]["stable"].as_bool() == Some(true))
        .or_else(|| list.first())
        .and_then(|e| e["loader"]["version"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("Fabric не підтримує Minecraft {mc}"))
}

// ---------- Бібліотеки ----------

/// «group:artifact:version[:classifier]» → шлях у репозиторії Maven.
pub fn maven_path(coord: &str) -> Option<String> {
    let (coord, ext) = coord.split_once('@').unwrap_or((coord, "jar"));
    let p: Vec<&str> = coord.split(':').collect();
    if p.len() < 3 {
        return None;
    }
    let (g, a, v) = (p[0].replace('.', "/"), p[1], p[2]);
    let classifier = p.get(3).map(|c| format!("-{c}")).unwrap_or_default();
    Some(format!("{g}/{a}/{v}/{a}-{v}{classifier}.{ext}"))
}

/// Ключ для злиття: та сама бібліотека без версії.
fn lib_key(name: &str) -> String {
    let p: Vec<&str> = name.split(':').collect();
    match p.len() {
        0..=2 => name.to_owned(),
        3 => format!("{}:{}", p[0], p[1]),
        _ => format!("{}:{}:{}", p[0], p[1], p[3]),
    }
}

/// Бібліотеки для запуску: спершу лоадер, потім ваніль (дублікати перемагає лоадер).
pub fn libraries(profile: &Profile) -> Vec<Download> {
    let features = HashMap::new();
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let sources = profile.extra.iter().chain(std::iter::once(&profile.json));
    for src in sources {
        for lib in src["libraries"].as_array().into_iter().flatten() {
            if !rules_allow(lib.get("rules"), &features) {
                continue;
            }
            let Some(name) = lib["name"].as_str() else { continue };
            if !seen.insert(lib_key(name)) {
                continue;
            }
            let path_rel;
            let url;
            let (sha1, size);
            if let Some(art) = lib.get("downloads").and_then(|d| d.get("artifact")) {
                path_rel = art["path"].as_str().map(str::to_owned).or_else(|| maven_path(name));
                url = art["url"].as_str().map(str::to_owned);
                sha1 = art["sha1"].as_str().map(str::to_owned);
                size = art["size"].as_u64();
            } else {
                // Формат Fabric: базова адреса Maven + шлях за координатами.
                path_rel = maven_path(name);
                let base = lib["url"].as_str().unwrap_or("https://libraries.minecraft.net/");
                url = path_rel.as_ref().map(|p| format!("{}/{}", base.trim_end_matches('/'), p));
                sha1 = lib["sha1"].as_str().map(str::to_owned);
                size = lib["size"].as_u64();
            }
            let (Some(rel), Some(url)) = (path_rel.as_deref().and_then(net::safe_rel), url) else { continue };
            if url.is_empty() {
                continue;
            }
            out.push(Download { urls: vec![url], path: paths::libraries().join(rel), sha1, size, executable: false });
        }
    }
    out
}

pub fn client_jar(profile: &Profile) -> Result<Download> {
    let c = &profile.json["downloads"]["client"];
    Ok(Download {
        urls: vec![c["url"].as_str().ok_or_else(|| anyhow!("у version.json немає клієнта"))?.to_owned()],
        path: paths::versions().join(&profile.vanilla_id).join(format!("{}.jar", profile.vanilla_id)),
        sha1: c["sha1"].as_str().map(str::to_owned),
        size: c["size"].as_u64(),
        executable: false,
    })
}

// ---------- Ассети ----------

pub async fn assets(profile: &Profile) -> Result<Vec<Download>> {
    let idx = &profile.json["assetIndex"];
    let id = idx["id"].as_str().filter(|i| valid_id(i)).ok_or_else(|| anyhow!("немає assetIndex"))?;
    let path = paths::assets().join("indexes").join(format!("{id}.json"));
    let index = cached_json(path, idx["url"].as_str().unwrap_or_default(), idx["sha1"].as_str()).await?;
    let mut out = Vec::new();
    for obj in index["objects"].as_object().into_iter().flat_map(|o| o.values()) {
        let Some(hash) = obj["hash"].as_str().filter(|h| h.len() == 40 && h.bytes().all(|b| b.is_ascii_hexdigit())) else { continue };
        let prefix = &hash[..2];
        out.push(Download {
            urls: vec![format!("{RESOURCES}/{prefix}/{hash}")],
            path: paths::assets().join("objects").join(prefix).join(hash),
            sha1: Some(hash.to_owned()),
            size: obj["size"].as_u64(),
            executable: false,
        });
    }
    Ok(out)
}

// ---------- Java ----------

/// Потрібний компонент Java і шлях до виконуваного файлу після встановлення.
pub fn java_component(profile: &Profile) -> String {
    profile.json["javaVersion"]["component"].as_str().unwrap_or("java-runtime-delta").to_owned()
}

pub fn java_home(component: &str) -> PathBuf {
    paths::runtime().join(component).join(java_platform())
}

pub fn java_exe(component: &str) -> PathBuf {
    let home = java_home(component);
    if cfg!(windows) {
        home.join("bin").join("javaw.exe")
    } else if cfg!(target_os = "macos") {
        home.join("jre.bundle/Contents/Home/bin/java")
    } else {
        home.join("bin").join("java")
    }
}

/// Чи веде відносне посилання `target`, розміщене в `link`, кудись усередині `home`.
fn link_stays_inside(home: &std::path::Path, link: &std::path::Path, target: &str) -> bool {
    use std::path::Component;
    if target.is_empty() || target.starts_with('/') || target.contains(':') {
        return false;
    }
    let Ok(rel) = link.parent().unwrap_or(home).strip_prefix(home) else { return false };
    let mut depth: Vec<_> = rel.components().collect();
    for c in std::path::Path::new(target).components() {
        match c {
            Component::ParentDir => {
                if depth.pop().is_none() {
                    return false;
                }
            }
            Component::Normal(_) => depth.push(c),
            Component::CurDir => {}
            _ => return false,
        }
    }
    true
}

/// Файли Java від Mojang для цієї системи. Посилання (symlink) створюються одразу.
pub async fn java_files(component: &str) -> Result<Vec<Download>> {
    let all: Value = net::get_json(JAVA_MANIFEST).await?;
    let entry = all[java_platform()][component]
        .as_array()
        .and_then(|a| a.first())
        .ok_or_else(|| anyhow!("Mojang не має Java {component} для цієї системи"))?;
    let manifest: Value = net::get_json(entry["manifest"]["url"].as_str().unwrap_or_default()).await?;
    let home = java_home(component);
    let mut out = Vec::new();
    for (rel, f) in manifest["files"].as_object().into_iter().flatten() {
        let Some(rel_path) = net::safe_rel(rel) else { bail!("підозрілий шлях у маніфесті Java: {rel}") };
        let path = home.join(rel_path);
        match f["type"].as_str() {
            Some("directory") => std::fs::create_dir_all(&path)?,
            Some("file") => {
                let raw = &f["downloads"]["raw"];
                out.push(Download {
                    urls: vec![raw["url"].as_str().unwrap_or_default().to_owned()],
                    path,
                    sha1: raw["sha1"].as_str().map(str::to_owned),
                    size: raw["size"].as_u64(),
                    executable: f["executable"].as_bool().unwrap_or(false),
                });
            }
            #[cfg(unix)]
            Some("link") => {
                if let (Some(target), false) = (f["target"].as_str(), path.symlink_metadata().is_ok()) {
                    // Посилання мусить лишатися всередині теки Java.
                    if !link_stays_inside(&home, &path, target) {
                        bail!("підозріле посилання в маніфесті Java: {rel}");
                    }
                    if let Some(p) = path.parent() {
                        std::fs::create_dir_all(p)?;
                    }
                    std::os::unix::fs::symlink(target, &path)?;
                }
            }
            _ => {}
        }
    }
    if out.is_empty() {
        bail!("порожній маніфест Java");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn ids() {
        assert!(valid_id("26.3"));
        assert!(valid_id("0.20.1"));
        assert!(valid_id("26.4-snapshot-10"));
        assert!(!valid_id("../26.3"));
        assert!(!valid_id("a/b"));
        assert!(!valid_id(""));
    }

    #[test]
    fn links() {
        let home = Path::new("/rt");
        assert!(link_stays_inside(home, Path::new("/rt/bin/java"), "../lib/java"));
        assert!(!link_stays_inside(home, Path::new("/rt/bin/java"), "../../../etc/passwd"));
        assert!(!link_stays_inside(home, Path::new("/rt/bin/java"), "/usr/bin/sh"));
    }
}
