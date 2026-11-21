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
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("Altherra-windows-x64.exe")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64", target_env = "musl")) {
        Some("altherra-launcher-linux-x64-musl")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("altherra-launcher-linux-x64-gnu")
    } else {
        None
    }
}

pub struct Available {
    pub version: String,
    pub page: String,
    tag: String,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
}

/// «launcher-v1.2.3» / «v1.2.3» / «1.2.3» → (1, 2, 3)
fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let v = s.trim_start_matches("launcher-").trim_start_matches('v');
    let mut it = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next()??, it.next().unwrap_or(Some(0))?))
}

pub fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

pub async fn check() -> Result<Option<Available>> {
    let r: Release = net::get_json(&format!("https://api.github.com/repos/{REPO}/releases/latest")).await?;
    if r.draft || r.prerelease {
        return Ok(None);
    }
    let (Some(latest), Some(cur)) = (parse_version(&r.tag_name), parse_version(current())) else { return Ok(None) };
    if latest <= cur {
        return Ok(None);
    }
    let page = if r.html_url.starts_with("https://github.com/") { r.html_url } else { format!("https://github.com/{REPO}/releases/latest") };
    Ok(Some(Available { version: format!("{}.{}.{}", latest.0, latest.1, latest.2), page, tag: r.tag_name }))
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
