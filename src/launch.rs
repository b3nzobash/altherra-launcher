//! Підготовка (скачування всього потрібного) і запуск гри.
use crate::auth::{Account, Kind};
use crate::manifest::{self, Manifest};
use crate::mojang::{self, Profile};
use crate::net;
use crate::paths;
use crate::settings::Settings;
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};

/// Повідомлення для інтерфейсу під час підготовки.
pub trait Reporter: Send + Sync + 'static {
    fn stage(&self, text: &str, progress: f32);
}

pub struct Prepared {
    profile: Profile,
    java: PathBuf,
    classpath: Vec<PathBuf>,
}

fn fmt_mb(bytes: u64) -> String {
    format!("{:.1} МБ", bytes as f64 / 1048576.0)
}

/// Скачує Java, бібліотеки, клієнт, ассети й моди. `check_hash` — перевірити хеші всіх файлів.
pub async fn prepare(m: &Manifest, settings: &Settings, check_hash: bool, rep: std::sync::Arc<dyn Reporter>) -> Result<Prepared> {
    rep.stage("Отримуємо версію гри…", 0.02);
    let profile = mojang::load_profile(&m.minecraft, &m.loader.kind, &m.loader.version).await?;

    // Java
    let java = if settings.java_path.trim().is_empty() {
        let component = mojang::java_component(&profile);
        let exe = mojang::java_exe(&component);
        if !exe.exists() || check_hash {
            rep.stage("Готуємо Java…", 0.05);
            let files = mojang::java_files(&component).await?;
            let r = rep.clone();
            net::download_all(files, 16, check_hash, move |d, t, b| r.stage(&format!("Java · {d} / {t} · {}", fmt_mb(b)), 0.05 + 0.15 * d as f32 / t as f32)).await?;
        }
        exe
    } else {
        PathBuf::from(settings.java_path.trim())
    };

    // Бібліотеки й клієнт
    let mut libs = mojang::libraries(&profile);
    let client = mojang::client_jar(&profile)?;
    let classpath: Vec<PathBuf> = libs.iter().map(|d| d.path.clone()).chain(std::iter::once(client.path.clone())).collect();
    libs.push(client);
    let r = rep.clone();
    net::download_all(libs, 16, check_hash, move |d, t, b| r.stage(&format!("Бібліотеки · {d} / {t} · {}", fmt_mb(b)), 0.2 + 0.2 * d as f32 / t as f32)).await?;

    // Ассети
    rep.stage("Перевіряємо ресурси гри…", 0.4);
    let assets = mojang::assets(&profile).await?;
    let r = rep.clone();
    net::download_all(assets, 32, check_hash, move |d, t, b| r.stage(&format!("Ресурси · {d} / {t} · {}", fmt_mb(b)), 0.4 + 0.45 * d as f32 / t as f32)).await?;

    // Моди й інші файли збірки
    rep.stage("Синхронізуємо моди…", 0.86);
    let files = manifest::plan_sync(m)?;
    let r = rep.clone();
    net::download_all(files, 8, true, move |d, t, b| r.stage(&format!("Моди · {d} / {t} · {}", fmt_mb(b)), 0.86 + 0.13 * d as f32 / t as f32)).await?;
    manifest::save_sync_state(m);

    rep.stage("Запускаємо…", 1.0);
    Ok(Prepared { profile, java, classpath })
}

/// Розгортає аргументи з version.json (рядки й умовні блоки) з підстановкою змінних.
fn expand(args: Option<&Value>, vars: &HashMap<&str, String>, features: &HashMap<&str, bool>, out: &mut Vec<String>) {
    let sub = |s: &str| {
        let mut r = s.to_owned();
        for (k, v) in vars {
            r = r.replace(&format!("${{{k}}}"), v);
        }
        r
    };
    for a in args.and_then(Value::as_array).into_iter().flatten() {
        match a {
            Value::String(s) => out.push(sub(s)),
            Value::Object(o) => {
                if !mojang::rules_allow(o.get("rules"), features) {
                    continue;
                }
                match o.get("value") {
                    Some(Value::String(s)) => out.push(sub(s)),
                    Some(Value::Array(vs)) => out.extend(vs.iter().filter_map(Value::as_str).map(sub)),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

/// Аргументи JVM, які дозволено задавати серверу. Решта (агенти, запуск команд, шляхи бібліотек)
/// дала б зламаному серверу виконати код на компʼютері гравця, тому відкидаємо.
fn server_arg_allowed(a: &str) -> bool {
    let lower = a.to_ascii_lowercase();
    if lower.starts_with("-xx:") {
        let forbidden = ["onerror", "onoutofmemoryerror", "flags=", "vmoptionsfile", "errorfile", "heapdumppath", "logfile", "compilecommandfile"];
        return !forbidden.iter().any(|f| lower.contains(f));
    }
    if lower.starts_with("-xss") || lower.starts_with("-xmn") {
        return true;
    }
    if let Some(prop) = lower.strip_prefix("-d") {
        let forbidden = ["java.library.path", "java.class.path", "java.security", "java.system.class.loader", "jdk.attach", "log4j", "fabric.addmods", "fabric.loadmods", "fabric.classpathgroups", "org.lwjgl.librarypath", "jna.", "user.home", "user.dir", "java.home", "sun."];
        return !prop.is_empty() && !forbidden.iter().any(|f| prop.starts_with(f));
    }
    false
}

/// Рядок аргументів від користувача з урахуванням лапок.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for c in s.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, c) if c.is_whitespace() => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            (_, c) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Запускає гру й повертає процес. Рядки журналу йдуть у `on_line`.
pub fn spawn(p: &Prepared, m: &Manifest, s: &Settings, acc: &Account, on_line: impl Fn(String) + Send + Sync + 'static) -> Result<tokio::process::Child> {
    if !p.java.exists() {
        bail!("Не знайдено Java: {}", p.java.display());
    }
    let game = paths::game_dir();
    std::fs::create_dir_all(&game)?;
    let natives = paths::natives(&p.profile.id);
    std::fs::create_dir_all(&natives)?;
    let sep = if cfg!(windows) { ";" } else { ":" };
    let cp = p.classpath.iter().map(|x| x.to_string_lossy().into_owned()).collect::<Vec<_>>().join(sep);

    let ram = if s.ram_mb > 0 { s.ram_mb } else { m.recommended_ram_mb.max(2048) };
    let server = m.server_address.trim();
    let connect = s.auto_connect && !server.is_empty();
    let server_arg = if server.contains(':') { server.to_owned() } else { format!("{server}:25565") };
    let (token, user_type) = match acc.kind {
        Kind::Microsoft => (acc.access_token.clone(), "msa"),
        Kind::Offline => ("0".to_owned(), "legacy"),
    };

    let vars: HashMap<&str, String> = HashMap::from([
        ("auth_player_name", acc.name.clone()),
        ("version_name", p.profile.id.clone()),
        ("game_directory", game.to_string_lossy().into_owned()),
        ("assets_root", paths::assets().to_string_lossy().into_owned()),
        ("assets_index_name", p.profile.json["assetIndex"]["id"].as_str().unwrap_or_default().to_owned()),
        ("auth_uuid", acc.id.clone()),
        ("auth_access_token", token),
        ("auth_session", "0".to_owned()),
        ("clientid", String::new()),
        ("auth_xuid", "0".to_owned()),
        ("user_type", user_type.to_owned()),
        ("user_properties", "{}".to_owned()),
        ("version_type", "release".to_owned()),
        ("natives_directory", natives.to_string_lossy().into_owned()),
        ("launcher_name", "altherra".to_owned()),
        ("launcher_version", env!("CARGO_PKG_VERSION").to_owned()),
        ("classpath", cp),
        ("classpath_separator", sep.to_owned()),
        ("library_directory", paths::libraries().to_string_lossy().into_owned()),
        ("quickPlayMultiplayer", server_arg),
        ("resolution_width", s.width.to_string()),
        ("resolution_height", s.height.to_string()),
    ]);
    let features: HashMap<&str, bool> = HashMap::from([
        ("is_demo_user", false),
        ("has_custom_resolution", s.width > 0 && s.height > 0),
        ("is_quick_play_multiplayer", connect),
    ]);

    let mut args = vec![format!("-Xmx{ram}M"), format!("-Xms{}M", (ram / 2).min(2048))];
    args.extend(split_args(&m.jvm_args).into_iter().filter(|a| server_arg_allowed(a)));
    args.extend(split_args(&s.jvm_args));
    expand(p.profile.json.get("arguments").and_then(|a| a.get("jvm")), &vars, &features, &mut args);
    if let Some(extra) = &p.profile.extra {
        expand(extra.get("arguments").and_then(|a| a.get("jvm")), &vars, &features, &mut args);
    }
    args.push(p.profile.main_class.clone());
    expand(p.profile.json.get("arguments").and_then(|a| a.get("game")), &vars, &features, &mut args);
    if let Some(extra) = &p.profile.extra {
        expand(extra.get("arguments").and_then(|a| a.get("game")), &vars, &features, &mut args);
    }

    let mut cmd = tokio::process::Command::new(&p.java);
    cmd.args(&args).current_dir(&game).stdout(Stdio::piped()).stderr(Stdio::piped()).stdin(Stdio::null());
    #[cfg(windows)]
    {
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = cmd.spawn().with_context(|| format!("не вдалося запустити {}", p.java.display()))?;

    // Журнал гри: токен доступу ніколи не показуємо.
    let secret = acc.access_token.clone();
    let on_line = std::sync::Arc::new(on_line);
    let pipes: [Option<Box<dyn tokio::io::AsyncRead + Unpin + Send>>; 2] =
        [child.stdout.take().map(|x| Box::new(x) as _), child.stderr.take().map(|x| Box::new(x) as _)];
    for pipe in pipes.into_iter().flatten() {
        let (on_line, secret) = (on_line.clone(), secret.clone());
        tokio::spawn(async move {
            let mut lines = BufReader::new(pipe).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = if !secret.is_empty() { line.replace(&secret, "•••") } else { line };
                on_line(line);
            }
        });
    }
    on_line(format!("[лаунчер] {} {}", p.java.display(), args.iter().map(|a| if a == &acc.access_token && !a.is_empty() { "•••" } else { a.as_str() }).collect::<Vec<_>>().join(" ")));
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::server_arg_allowed as ok;

    #[test]
    fn server_jvm_args() {
        assert!(ok("-XX:+UseZGC"));
        assert!(ok("-XX:MaxGCPauseMillis=50"));
        assert!(ok("-Dsodium.checks.issue2561=false"));
        assert!(ok("-Xss2M"));
        assert!(!ok("-XX:OnOutOfMemoryError=calc.exe"));
        assert!(!ok("-XX:OnError=rm -rf ~"));
        assert!(!ok("-javaagent:x.jar"));
        assert!(!ok("-agentpath:/tmp/x.so"));
        assert!(!ok("-Djava.library.path=/tmp"));
        assert!(!ok("-Dfabric.addMods=/tmp/evil.jar"));
        assert!(!ok("-cp"));
        assert!(!ok("--add-opens"));
    }
}
