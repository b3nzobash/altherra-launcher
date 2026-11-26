//! Завантаження з перевіркою SHA-1, повторами й паралельністю.
use anyhow::{Context, Result, anyhow, bail};
use futures_util::{StreamExt, stream};
use sha1::{Digest, Sha1};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

pub const USER_AGENT: &str = concat!("AltherraLauncher/", env!("CARGO_PKG_VERSION"));

/// Найбільша відповідь API, яку ми готові читати в памʼять (JSON, картинки).
pub const MAX_BODY: usize = 32 * 1024 * 1024;

fn is_local_host(host: &str) -> bool {
    matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
}

/// Дозволені адреси: лише HTTPS. Звичайний HTTP — тільки до цього компʼютера (для розробки).
pub fn allowed_url(url: &str) -> bool {
    match reqwest::Url::parse(url) {
        Ok(u) => match u.scheme() {
            "https" => u.host_str().is_some(),
            "http" => u.host_str().is_some_and(is_local_host),
            _ => false,
        },
        Err(_) => false,
    }
}

pub fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        // Перенаправлення не можуть знизити HTTPS до HTTP.
        let policy = reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                attempt.error("забагато перенаправлень")
            } else if !allowed_url(attempt.url().as_str()) {
                attempt.error("небезпечне перенаправлення")
            } else {
                attempt.follow()
            }
        });
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .https_only(false) // перевіряємо самі через allowed_url, щоб лишити localhost для розробки
            .redirect(policy)
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("http client")
    })
}

/// GET з перевіркою адреси й обмеженням розміру відповіді.
pub async fn get_bytes(url: &str, limit: usize) -> Result<Vec<u8>> {
    if !allowed_url(url) {
        bail!("небезпечна адреса: {}", host(url));
    }
    let res = client().get(url).send().await.with_context(|| format!("немає зʼєднання з {}", host(url)))?;
    if !res.status().is_success() {
        bail!("{} відповів {}", host(url), res.status());
    }
    if res.content_length().is_some_and(|n| n as usize > limit) {
        bail!("{} повернув завелику відповідь", host(url));
    }
    let mut out = Vec::new();
    let mut body = res.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        if out.len() + chunk.len() > limit {
            bail!("{} повернув завелику відповідь", host(url));
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}

pub async fn get_json<T: serde::de::DeserializeOwned>(url: &str) -> Result<T> {
    let bytes = get_bytes(url, MAX_BODY).await?;
    serde_json::from_slice(&bytes).with_context(|| format!("{} повернув незрозумілу відповідь", host(url)))
}

/// Відносний шлях із чужих даних: без «..», абсолютних шляхів, дисків і дивних символів.
pub fn safe_rel(rel: &str) -> Option<PathBuf> {
    use std::path::Component;
    if rel.is_empty() || rel.len() > 400 || rel.contains([':', '\0', '\\']) || rel.starts_with('/') {
        return None;
    }
    let p = PathBuf::from(rel);
    p.components().all(|c| matches!(c, Component::Normal(_))).then_some(p)
}

pub fn host(url: &str) -> String {
    reqwest::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_else(|| url.to_owned())
}

/// Один файл, який треба мати на диску.
#[derive(Clone, Debug)]
pub struct Download {
    pub urls: Vec<String>,
    pub path: PathBuf,
    pub sha1: Option<String>,
    pub size: Option<u64>,
    pub executable: bool,
}

pub fn sha1_file(path: &Path) -> Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h = Sha1::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex::encode(h.finalize()))
}

/// Чи вже лежить правильний файл. Розмір перевіряємо завжди, хеш — коли він відомий.
pub fn is_valid(d: &Download, check_hash: bool) -> bool {
    let Ok(meta) = std::fs::metadata(&d.path) else { return false };
    if let Some(size) = d.size {
        if meta.len() != size {
            return false;
        }
    }
    match (&d.sha1, check_hash) {
        (Some(expected), true) => sha1_file(&d.path).map(|h| h.eq_ignore_ascii_case(expected)).unwrap_or(false),
        _ => true,
    }
}

async fn fetch_once(url: &str, d: &Download, done_bytes: &AtomicU64) -> Result<()> {
    // Тимчасове ім'я — від повного імені файлу й унікальне: раніше jvm.cfg і jvm.lib качалися
    // в один і той самий jvm.part і перезаписували одне одного (Java падала з «No leading -»).
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let mut name = d.path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".{}.part", SEQ.fetch_add(1, Ordering::Relaxed)));
    let tmp = d.path.with_file_name(name);
    let res = client().get(url).send().await?;
    if !res.status().is_success() {
        bail!("{} відповів {}", host(url), res.status());
    }
    // Без відомого розміру більше 1 ГБ не качаємо.
    let limit = d.size.unwrap_or(1 << 30);
    let mut file = tokio::fs::File::create(&tmp).await?;
    let mut hasher = Sha1::new();
    let mut got: u64 = 0;
    let mut body = res.bytes_stream();
    while let Some(chunk) = body.next().await {
        let chunk = chunk?;
        got += chunk.len() as u64;
        if got > limit {
            drop(file);
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("файл більший, ніж заявлено");
        }
        hasher.update(&chunk);
        done_bytes.fetch_add(chunk.len() as u64, Ordering::Relaxed);
        file.write_all(&chunk).await?;
    }
    file.flush().await?;
    drop(file);
    if let Some(size) = d.size {
        if got != size {
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("файл обрізаний ({got} з {size} байт)");
        }
    }
    if let Some(expected) = &d.sha1 {
        let actual = hex::encode(hasher.finalize());
        if !actual.eq_ignore_ascii_case(expected) {
            let _ = tokio::fs::remove_file(&tmp).await;
            bail!("контрольна сума не збіглася");
        }
    }
    tokio::fs::rename(&tmp, &d.path).await?;
    #[cfg(unix)]
    if d.executable {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&d.path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub async fn download(d: &Download, done_bytes: &AtomicU64) -> Result<()> {
    if let Some(parent) = d.path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let mut last_err = anyhow!("немає безпечного посилання");
    for url in d.urls.iter().filter(|u| allowed_url(u)) {
        for attempt in 0..3 {
            match fetch_once(url, d, done_bytes).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    last_err = e;
                    tokio::time::sleep(Duration::from_millis(400 * (attempt + 1))).await;
                }
            }
        }
    }
    let name = d.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Err(last_err.context(format!("не вдалося скачати {name}")))
}

/// Скачує все, чого бракує. `progress(готово_файлів, усього_файлів, байти)`.
pub async fn download_all(
    items: Vec<Download>,
    parallel: usize,
    check_hash: bool,
    progress: impl Fn(usize, usize, u64) + Send + Sync + 'static,
) -> Result<()> {
    // Перевірка наявних файлів теж займає час, тож робимо її в окремих потоках.
    let missing: Vec<Download> = tokio::task::spawn_blocking(move || items.into_iter().filter(|d| !is_valid(d, check_hash)).collect())
        .await?;
    let total = missing.len();
    if total == 0 {
        return Ok(());
    }
    let done = Arc::new(AtomicUsize::new(0));
    let bytes = Arc::new(AtomicU64::new(0));
    let progress = Arc::new(progress);
    progress(0, total, 0);
    let results: Vec<Result<()>> = stream::iter(missing)
        .map(|d| {
            let (done, bytes, progress) = (done.clone(), bytes.clone(), progress.clone());
            async move {
                let r = download(&d, &bytes).await;
                let n = done.fetch_add(1, Ordering::Relaxed) + 1;
                progress(n, total, bytes.load(Ordering::Relaxed));
                r
            }
        })
        .buffer_unordered(parallel)
        .collect()
        .await;
    results.into_iter().collect::<Result<Vec<_>>>()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        assert!(allowed_url("https://cdn.modrinth.com/a.jar"));
        assert!(allowed_url("http://localhost:8791/api/manifest"));
        assert!(!allowed_url("http://evil.example/a.jar"));
        assert!(!allowed_url("file:///etc/passwd"));
        assert!(!allowed_url("javascript:alert(1)"));
    }

    #[test]
    fn rel_paths() {
        assert!(safe_rel("org/lwjgl/lwjgl/3.4.3/lwjgl-3.4.3.jar").is_some());
        assert!(safe_rel("../../.bashrc").is_none());
        assert!(safe_rel("a/../../b").is_none());
        assert!(safe_rel("/etc/passwd").is_none());
        assert!(safe_rel("C:/Windows/x.dll").is_none());
        assert!(safe_rel("a\\..\\b").is_none());
        assert!(safe_rel("").is_none());
    }
}
