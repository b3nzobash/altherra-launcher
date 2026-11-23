//! Самооновлення з релізів GitHub. Список контрольних сум підписаний ключем Ed25519,
//! публічна частина якого вшита в лаунчер, тож підмінений файл не встановиться.
use crate::net;
use anyhow::{Context, Result, anyhow, bail};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::Deserialize;
use sha2::{Digest, Sha256};

/// Репозиторій із релізами. Можна змінити під час збирання: ALTHERRA_REPO=власник/назва
pub const REPO: &str = match option_env!("ALTHERRA_REPO") {
    Some(r) => r,
    None => "b3nzobash/altherra-launcher",
};

/// Публічний ключ підпису оновлень (Ed25519).
const PUBLIC_KEY: [u8; 32] = hex_key("1c4814f2efc45dfd3931c8a3855356a8f50a617a29137235551ce6c0e99cf11f");

const fn hex_key(s: &str) -> [u8; 32] {
    const fn nib(c: u8) -> u8 {
        match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            _ => panic!("bad hex"),
        }
    }
    let b = s.as_bytes();
    let mut out = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        out[i] = nib(b[2 * i]) << 4 | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

/// Файл релізу для цієї системи. На macOS оновлюємо через сторінку завантаження.
pub fn asset_name() -> Option<&'static str> {
    // Кожна збірка оновлюється лише до своєї: офіційна — до офіційної, Offline — до Offline.
    let official = crate::OFFICIAL;
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some(if official { "Altherra-windows-x64.exe" } else { "Altherra-Offline-windows-x64.exe" })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64", target_env = "musl")) {
        Some(if official { "altherra-launcher-linux-x64-musl" } else { "altherra-offline-linux-x64-musl" })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(if official { "altherra-launcher-linux-x64-gnu" } else { "altherra-offline-linux-x64-gnu" })
    } else {
        None
    }
}

pub struct Available {
    pub version: String,
    pub page: String,
    tag: String,
}

/// Одна версія для сторінки «Що нового».
pub struct Change {
    pub version: String,
    pub date: String,
    pub notes: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    published_at: Option<String>,
    draft: bool,
    prerelease: bool,
}

/// «launcher-v1.2.3» / «v1.2.3» / «1.2.3» → (1, 2, 3)
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let v = s.trim_start_matches("launcher-").trim_start_matches("offline-").trim_start_matches('v');
    let mut it = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().unwrap_or(Some(0))?))
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Префікс тегів релізів цієї версії: основна — «launcher-v…», Offline — «offline-v…» (окрема гілка).
pub fn tag_prefix() -> &'static str {
    if crate::OFFICIAL { "launcher-v" } else { "offline-v" }
}

pub async fn check() -> Result<Option<Available>> {
    Ok(check_with_changes().await?.0)
}

/// Нова версія (якщо є) і список змін своїх релізів — одним запитом до GitHub.
pub async fn check_with_changes() -> Result<(Option<Available>, Vec<Change>)> {
    // Релізи обох версій лежать в одному репозиторії — беремо лише свої.
    let list: Vec<Release> = net::get_json(&format!("https://api.github.com/repos/{REPO}/releases?per_page=30")).await?;
    let mut own: Vec<((u32, u32, u32), Release)> = list
        .into_iter()
        .filter(|r| !r.draft && !r.prerelease && r.tag_name.starts_with(tag_prefix()))
        .filter_map(|r| parse_version(r.tag_name.trim_start_matches(tag_prefix())).map(|v| (v, r)))
        .collect();
    own.sort_by(|a, b| b.0.cmp(&a.0));
    let changes = own
        .iter()
        .map(|(v, r)| Change {
            version: format!("{}.{}.{}", v.0, v.1, v.2),
            date: r.published_at.as_deref().map(format_date).unwrap_or_default(),
            notes: release_notes(r.body.as_deref().unwrap_or("")),
        })
        .collect();
    let Some(cur) = parse_version(current()) else { return Ok((None, changes)) };
    let available = own.into_iter().next().filter(|(v, _)| *v > cur).map(|(latest, r)| {
        let page = if r.html_url.starts_with("https://github.com/") { r.html_url } else { format!("https://github.com/{REPO}/releases") };
        Available { version: format!("{}.{}.{}", latest.0, latest.1, latest.2), page, tag: r.tag_name }
    });
    Ok((available, changes))
}

/// «2026-11-23T15:40:21Z» → «23.11.2026»
fn format_date(s: &str) -> String {
    let d = s.get(..10).unwrap_or("");
    let p: Vec<&str> = d.split('-').collect();
    if p.len() == 3 { format!("{}.{}.{}", p[2], p[1], p[0]) } else { String::new() }
}

/// Опис змін з тексту релізу: усе до роздільника «---» (далі — посилання на файли).
/// Розмітку Markdown спрощуємо до звичайного тексту з маркерами «•».
fn release_notes(body: &str) -> String {
    let body = body.replace("\r\n", "\n");
    let part = body.split("\n---").next().unwrap_or("");
    // Старі релізи мають лише список файлів для завантаження — змін там немає.
    if part.trim_start().starts_with("- Windows") {
        return String::new();
    }
    part.lines()
        .map(|l| l.trim_end())
        .filter(|l| !l.starts_with('#'))
        .map(|l| {
            let l = l.replace("**", "").replace('`', "");
            match l.trim_start().strip_prefix("- ").or_else(|| l.trim_start().strip_prefix("* ")) {
                Some(rest) => format!("• {rest}"),
                None => l,
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_owned()
}

/// Скачує нову версію, перевіряє підпис і хеш, підміняє свій файл. Після цього треба перезапуститися.
pub async fn install(a: &Available, progress: impl Fn(f32)) -> Result<std::path::PathBuf> {
    let asset = asset_name().ok_or_else(|| anyhow!("для цієї системи оновлення встановлюється вручну"))?;
    if !a.tag.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_')) {
        bail!("дивна назва релізу");
    }
    let base = format!("https://github.com/{REPO}/releases/download/{}", a.tag);
    progress(0.05);
    let sums = net::get_bytes(&format!("{base}/SHA256SUMS.txt"), 64 * 1024).await?;
    let sig = net::get_bytes(&format!("{base}/SHA256SUMS.txt.sig"), 1024).await?;
    let key = VerifyingKey::from_bytes(&PUBLIC_KEY).context("ключ оновлень")?;
    let sig = Signature::from_slice(&sig).map_err(|_| anyhow!("пошкоджений підпис оновлення"))?;
    key.verify(&sums, &sig).map_err(|_| anyhow!("підпис оновлення не збігся — файл не від Altherra"))?;
    let expected = std::str::from_utf8(&sums)?
        .lines()
        .find_map(|l| {
            let (hash, name) = l.split_once(char::is_whitespace)?;
            (name.trim().trim_start_matches('*') == asset).then(|| hash.to_ascii_lowercase())
        })
        .ok_or_else(|| anyhow!("у релізі немає файлу {asset}"))?;

    progress(0.15);
    let bin = net::get_bytes(&format!("{base}/{asset}"), 200 * 1024 * 1024).await?;
    if hex::encode(Sha256::digest(&bin)) != expected {
        bail!("контрольна сума оновлення не збіглася");
    }
    progress(0.9);
    let exe = std::env::current_exe()?;
    replace_self(&exe, &bin)?;
    progress(1.0);
    Ok(exe)
}

fn replace_self(exe: &std::path::Path, bin: &[u8]) -> Result<()> {
    let dir = exe.parent().ok_or_else(|| anyhow!("невідома тека лаунчера"))?;
    let new = dir.join(".altherra-update.tmp");
    std::fs::write(&new, bin).with_context(|| format!("немає прав на запис у {}", dir.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new, std::fs::Permissions::from_mode(0o755))?;
        // Запущений файл лишається в памʼяті, тож його можна просто замінити.
        std::fs::rename(&new, exe)?;
    }
    #[cfg(windows)]
    {
        // Запущений .exe не можна перезаписати, але можна перейменувати.
        let old = exe.with_extension("old.exe");
        let _ = std::fs::remove_file(&old);
        std::fs::rename(exe, &old)?;
        if let Err(e) = std::fs::rename(&new, exe) {
            let _ = std::fs::rename(&old, exe);
            return Err(e.into());
        }
    }
    Ok(())
}

/// Прибирає старий .exe, що лишився після оновлення на Windows.
pub fn cleanup() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::remove_file(exe.with_extension("old.exe"));
        if let Some(dir) = exe.parent() {
            let _ = std::fs::remove_file(dir.join(".altherra-update.tmp"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("launcher-v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("v1.10"), Some((1, 10, 0)));
        assert!(parse_version("launcher-v1.10.0") > parse_version("1.9.9"));
        assert_eq!(parse_version("nightly"), None);
    }

    #[test]
    fn notes() {
        let body = "## Що нового\r\n- **Ресурспаки** вмикаються самі\r\n- Виправлено `options.txt`\r\n\r\n---\r\n- Windows 10/11 — x.exe";
        assert_eq!(release_notes(body), "• Ресурспаки вмикаються самі\n• Виправлено options.txt");
        assert_eq!(release_notes("- Windows 10/11 — x.exe\n- Linux"), "");
        assert_eq!(format_date("2026-11-23T15:40:21Z"), "23.11.2026");
    }

    #[test]
    fn key_parses() {
        assert!(VerifyingKey::from_bytes(&PUBLIC_KEY).is_ok());
    }
}

#[cfg(test)]
mod sig_tests {
    use super::*;

    /// Перевірка підпису, зробленого так само, як у CI (openssl pkeyutl -rawin).
    /// Запуск: ALTHERRA_SIG_FILE=… ALTHERRA_SIG=… cargo test sig_from_openssl -- --ignored
    #[test]
    #[ignore]
    fn sig_from_openssl() {
        let data = std::fs::read(std::env::var("ALTHERRA_SIG_FILE").unwrap()).unwrap();
        let sig = std::fs::read(std::env::var("ALTHERRA_SIG").unwrap()).unwrap();
        let key = VerifyingKey::from_bytes(&PUBLIC_KEY).unwrap();
        key.verify(&data, &Signature::from_slice(&sig).unwrap()).expect("підпис має збігтися");
        let mut bad = data.clone();
        bad[0] ^= 1;
        assert!(key.verify(&bad, &Signature::from_slice(&sig).unwrap()).is_err());
    }
}
