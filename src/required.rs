//! Обовʼязкові файли гри, які лаунчер ставить завжди, незалежно від збірки на сервері:
//!  - мод Altherra Client (перепідключення після кіку, «Вийти з гри») — вшитий у лаунчер;
//!  - Fabric API, без якого мод не працює, — якщо його немає в збірці, береться з Modrinth.
use crate::manifest::{FileEntry, Manifest};
use crate::net;
use crate::paths;
use anyhow::{Context, Result, anyhow};
use serde_json::Value;
use sha1::{Digest, Sha1};

const CLIENT_JAR: &[u8] = include_bytes!("../assets/mods/altherra-client.jar");
const CLIENT_NAME: &str = "altherra-client.jar";
const FABRIC_API_PROJECT: &str = "P7dR8mSH";

/// Збірка сервера + обовʼязкові файли. Мод ставиться лише для Fabric.
pub async fn with_required(m: &Manifest) -> Result<Manifest> {
    let mut m = m.clone();
    if m.loader.kind != "fabric" {
        return Ok(m);
    }
    // Свою копію мода з панелі прибираємо — лаунчер ставить вшиту.
    m.files.retain(|f| !(f.dir == "mods" && f.filename.to_ascii_lowercase().starts_with("altherra-client")));

    let has_fabric_api = m.files.iter().any(|f| f.dir == "mods" && f.filename.to_ascii_lowercase().starts_with("fabric-api"));
    if !has_fabric_api {
        match fabric_api(&m.minecraft).await {
            Ok(f) => m.files.push(f),
            // Без Fabric API мод не завантажиться й не дасть запустити гру, тож тоді не ставимо й мод.
            Err(_) => return Ok(m),
        }
    }

    let sha1 = hex::encode(Sha1::digest(CLIENT_JAR));
    let path = paths::game_dir().join("mods").join(CLIENT_NAME);
    let up_to_date = std::fs::read(&path).map(|b| hex::encode(Sha1::digest(&b)) == sha1).unwrap_or(false);
    if !up_to_date {
        std::fs::create_dir_all(path.parent().unwrap())?;
        let tmp = path.with_extension("jar.part");
        std::fs::write(&tmp, CLIENT_JAR)?;
        std::fs::rename(tmp, &path).context("не вдалося записати мод Altherra Client")?;
    }
    m.files.push(FileEntry {
        name: "Altherra Client".into(),
        dir: "mods".into(),
        filename: CLIENT_NAME.into(),
        sha1,
        size: CLIENT_JAR.len() as u64,
        // Файл уже записано вище; посилання потрібне лише формально.
        url: "https://github.com/b3nzobash/altherra-launcher".into(),
        fallback: None,
        required: false,
        optional: false,
    });
    Ok(m)
}

/// Остання стабільна версія Fabric API для цієї версії гри.
async fn fabric_api(mc: &str) -> Result<FileEntry> {
    let url = format!(
        "https://api.modrinth.com/v2/project/{FABRIC_API_PROJECT}/version?loaders=%5B%22fabric%22%5D&game_versions=%5B%22{}%22%5D",
        mc.replace('"', "")
    );
    let versions: Vec<Value> = net::get_json(&url).await?;
    let v = versions
        .iter()
        .find(|v| v["version_type"] == "release")
        .or_else(|| versions.first())
        .ok_or_else(|| anyhow!("Fabric API ще не вийшов для {mc}"))?;
    let files = v["files"].as_array().ok_or_else(|| anyhow!("у Fabric API немає файлів"))?;
    let f = files.iter().find(|f| f["primary"] == true).or_else(|| files.first()).ok_or_else(|| anyhow!("у Fabric API немає файлів"))?;
    let file_url = f["url"].as_str().unwrap_or_default();
    if !file_url.starts_with("https://cdn.modrinth.com/") {
        return Err(anyhow!("підозріле посилання на Fabric API"));
    }
    Ok(FileEntry {
        name: "Fabric API".into(),
        dir: "mods".into(),
        filename: f["filename"].as_str().unwrap_or("fabric-api.jar").into(),
        sha1: f["hashes"]["sha1"].as_str().unwrap_or_default().into(),
        size: f["size"].as_u64().unwrap_or(0),
        url: file_url.into(),
        fallback: None,
        required: false,
        optional: false,
    })
}
