// Лаунчер Altherra: нативний інтерфейс на Slint, логіка на Rust.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod auth;
mod launch;
mod manifest;
mod mojang;
mod net;
mod paths;
mod ping;
mod servers;
mod settings;
mod update;

use auth::{Accounts, Kind};
use manifest::Manifest;
use settings::Settings;
use slint::{ComponentHandle, Image, Model, ModelRc, Rgba8Pixel, SharedPixelBuffer, SharedString, VecModel, Weak};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

slint::include_modules!();

/// Офіційна збірка (з ліцензією Microsoft) чи Altherra Offline.
pub const OFFICIAL: bool = cfg!(feature = "microsoft");
pub const PRODUCT: &str = if OFFICIAL { "Altherra" } else { "Altherra Offline" };

/// В офіційній збірці гра за ніком доступна лише тоді, коли в лаунчері є ліцензійний акаунт.
fn offline_allowed(acc: &Accounts) -> bool {
    !OFFICIAL || acc.list.iter().any(|a| a.kind == Kind::Microsoft)
}

const MAX_LOG_LINES: usize = 3000;

/// Спільний стан між інтерфейсом і фоновими задачами.
#[derive(Default)]
struct State {
    manifest: Option<Manifest>,
    accounts: Accounts,
    settings: Settings,
    faces: HashMap<String, (Vec<u8>, u32)>,
    pending_log: Vec<String>,
    busy: bool,
    game_running: bool,
    kill: Option<tokio::sync::oneshot::Sender<()>>,
    ms_cancel: Option<tokio::sync::watch::Sender<bool>>,
}

type Shared = Arc<Mutex<State>>;

fn rt() -> &'static tokio::runtime::Runtime {
    static RT: std::sync::OnceLock<tokio::runtime::Runtime> = std::sync::OnceLock::new();
    RT.get_or_init(|| tokio::runtime::Builder::new_multi_thread().worker_threads(4).enable_all().build().expect("tokio"))
}

/// Декодування чужої картинки з обмеженням розміру, щоб величезний файл не зʼїв памʼять.
pub fn decode_image(bytes: &[u8], max_side: u32) -> anyhow::Result<image::DynamicImage> {
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(max_side);
    limits.max_image_height = Some(max_side);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader.decode()?)
}

fn image_from(buf: &(Vec<u8>, u32)) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(&buf.0, buf.1, buf.1))
}

fn image_rgba(data: &[u8], w: u32, h: u32) -> Image {
    Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::clone_from_slice(data, w, h))
}

fn initial(name: &str) -> SharedString {
    name.chars().next().map(|c| c.to_uppercase().to_string()).unwrap_or_default().into()
}

fn err_text(e: &anyhow::Error) -> String {
    // Перший рядок — суть, далі причина з ланцюжка, без технічного сміття.
    let mut parts: Vec<String> = e.chain().map(|c| c.to_string()).collect();
    parts.dedup();
    parts.truncate(2);
    parts.join(": ")
}

// ---------- Оновлення інтерфейсу ----------

fn show_toast(ui: &AppWindow, text: &str) {
    ui.set_toast(text.into());
    let weak = ui.as_weak();
    let text = text.to_owned();
    slint::Timer::single_shot(Duration::from_secs(6), move || {
        if let Some(ui) = weak.upgrade() {
            if ui.get_toast() == text.as_str() {
                ui.set_toast("".into());
            }
        }
    });
}

fn push_accounts(ui: &AppWindow, st: &State) {
    let selected = st.accounts.selected().map(|a| a.id.clone());
    let items: Vec<AccountItem> = st
        .accounts
        .list
        .iter()
        .map(|a| {
            let face = st.faces.get(&a.id);
            AccountItem {
                id: a.id.clone().into(),
                name: a.name.clone().into(),
                initial: initial(&a.name),
                licensed: a.kind == Kind::Microsoft,
                face: face.map(image_from).unwrap_or_default(),
                has_face: face.is_some(),
                selected: Some(&a.id) == selected.as_ref(),
            }
        })
        .collect();
    let current = items.iter().find(|a| a.selected).cloned();
    ui.set_offline_allowed(offline_allowed(&st.accounts));
    ui.set_has_account(current.is_some());
    if let Some(c) = current {
        ui.set_account(c);
    }
    ui.set_accounts(ModelRc::new(VecModel::from(items)));
}

fn push_manifest(ui: &AppWindow, m: &Manifest, online: bool) {
    ui.set_server_name(m.name.clone().into());
    ui.set_mc_version(m.minecraft.clone().into());
    let loader = match m.loader.kind.as_str() {
        "fabric" if !m.loader.version.is_empty() => format!("Fabric {}", m.loader.version),
        "fabric" => "Fabric".to_owned(),
        _ => String::new(),
    };
    ui.set_loader_text(loader.clone().into());
    let mods = m.files.iter().filter(|f| f.dir == "mods").count();
    ui.set_mods_count(mods as i32);
    ui.set_manifest_offline(!online);
    ui.set_manifest_error("".into());
    ui.set_ms_available(OFFICIAL && !ms_client_id(m).is_empty());
    ui.set_ram_recommended((m.recommended_ram_mb.max(1024) as f32 / 1024.0).round());
    let mut info = format!("Minecraft {}", m.minecraft);
    if !loader.is_empty() {
        info += &format!(" · {loader}");
    }
    info += &format!(" · ревізія {}", m.revision);
    ui.set_build_info(info.into());
    let files: Vec<FileItem> = m
        .files
        .iter()
        .map(|f| FileItem {
            name: f.name.clone().into(),
            dir: f.dir.clone().into(),
            size: if f.size >= 1048576 { format!("{:.1} МБ", f.size as f64 / 1048576.0) } else { format!("{} КБ", (f.size / 1024).max(1)) }.into(),
        })
        .collect();
    ui.set_files(ModelRc::new(VecModel::from(files)));
}

fn ms_client_id(m: &Manifest) -> String {
    std::env::var("ALTHERRA_MS_CLIENT_ID").unwrap_or_else(|_| m.ms_client_id.clone())
}

fn push_settings(ui: &AppWindow, s: &Settings, recommended_mb: u32) {
    let max_gb = (settings::system_ram_mb() as f32 / 1024.0).floor().clamp(2.0, 32.0);
    ui.set_ram_max(max_gb);
    let mb = if s.ram_mb > 0 { s.ram_mb } else { recommended_mb.max(2048) };
    ui.set_ram_gb((mb as f32 / 1024.0).round().clamp(1.0, max_gb));
    ui.set_java_path(s.java_path.clone().into());
    ui.set_jvm_args(s.jvm_args.clone().into());
    ui.set_auto_connect(s.auto_connect);
    ui.set_after_launch(match s.after_launch.as_str() {
        "keep" => 0,
        "close" => 2,
        _ => 1,
    });
}

// ---------- Фонові задачі ----------

/// Маніфест збірки, потім статус сервера й новини.
fn load_manifest(weak: Weak<AppWindow>, state: Shared) {
    rt().spawn(async move {
        match manifest::fetch().await {
            Ok((m, online)) => {
                state.lock().unwrap().manifest = Some(m.clone());
                let (m2, st2) = (m.clone(), state.clone());
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    push_manifest(&ui, &m2, online);
                    let s = st2.lock().unwrap();
                    push_settings(&ui, &s.settings, m2.recommended_ram_mb);
                });
                if !m.site_url.is_empty() {
                    load_news(weak.clone(), m.site_url.clone());
                }
            }
            Err(e) => {
                let msg = format!("Не вдалося отримати збірку з сервера лаунчера ({})", err_text(&e));
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_manifest_error(msg.into()));
            }
        }
    });
}

/// Статистика з сервера лаунчера (/api/stats).
#[derive(serde::Deserialize)]
struct Stats {
    status: String,
    players: u32,
    max: u32,
    tps: Option<f64>,
    uptime: Option<u64>,
    today: StatsToday,
    history: StatsHistory,
}
#[derive(serde::Deserialize)]
struct StatsToday {
    peak: u32,
    unique: Option<u32>,
}
#[derive(serde::Deserialize)]
struct StatsHistory {
    points: Vec<(u64, Option<u32>)>,
}

/// SVG-шлях графіка гравців у полі 100×30.
fn spark_path(points: &[(u64, Option<u32>)], max: u32) -> String {
    let n = points.len().max(2) as f32 - 1.0;
    let top = max.max(1) as f32;
    let mut out = String::new();
    let mut pen_down = false;
    for (i, (_, v)) in points.iter().enumerate() {
        let Some(v) = v else {
            pen_down = false;
            continue;
        };
        let x = i as f32 / n * 100.0;
        let y = 30.0 - (*v as f32 / top).min(1.0) * 28.0 - 1.0;
        out += &format!("{}{x:.1} {y:.1} ", if pen_down { "L" } else { "M" });
        pen_down = true;
    }
    out
}

fn fmt_uptime(secs: u64) -> String {
    let (d, h, m) = (secs / 86400, secs % 86400 / 3600, secs % 3600 / 60);
    if d > 0 { format!("{d} д {h} год") } else if h > 0 { format!("{h} год {m} хв") } else { format!("{m} хв") }
}

fn ping_loop(weak: Weak<AppWindow>, state: Shared) {
    rt().spawn(async move {
        loop {
            let address = state.lock().unwrap().manifest.as_ref().map(|m| m.server_address.clone()).unwrap_or_default();
            // Спершу статистика демона (з графіком), а якщо він недоступний — пряме опитування сервера.
            match net::get_json::<Stats>(&format!("{}/api/stats", manifest::api_url())).await {
                Ok(s) if s.status != "unknown" => {
                    let peak_top = s.history.points.iter().filter_map(|p| p.1).max().unwrap_or(0).max(s.today.peak);
                    let spark = spark_path(&s.history.points, peak_top.max(4));
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        let online = s.status == "online";
                        ui.set_server_state(if online { 1 } else { 2 });
                        ui.set_server_status(if online { format!("онлайн · {} / {}", s.players, s.max) } else { "сервер вимкнений".to_owned() }.into());
                        ui.set_stats_visible(true);
                        ui.set_stats_spark(spark.into());
                        ui.set_stats_peak(s.today.peak.to_string().into());
                        ui.set_stats_unique(s.today.unique.map(|u| u.to_string()).unwrap_or_else(|| "—".into()).into());
                        ui.set_stats_tps(s.tps.map(|t| format!("{t:.1}")).unwrap_or_else(|| "—".into()).into());
                        ui.set_stats_uptime(s.uptime.filter(|_| online).map(fmt_uptime).unwrap_or_else(|| "—".into()).into());
                    });
                }
                _ if !address.is_empty() => {
                    let res = ping::status(&address).await;
                    let _ = weak.upgrade_in_event_loop(move |ui| match res {
                        Ok(s) => {
                            ui.set_server_state(1);
                            ui.set_server_status(format!("онлайн · {} / {}", s.online, s.max).into());
                        }
                        Err(_) => {
                            ui.set_server_state(2);
                            ui.set_server_status("сервер вимкнений".into());
                        }
                    });
                }
                _ => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    continue;
                }
            }
            tokio::time::sleep(Duration::from_secs(30)).await;
        }
    });
}

fn load_news(weak: Weak<AppWindow>, site: String) {
    rt().spawn(async move {
        #[derive(serde::Deserialize)]
        struct Resp {
            news: Vec<Item>,
        }
        #[derive(serde::Deserialize)]
        struct Item {
            title: String,
            summary: String,
            url: String,
            cover: Option<String>,
            published_at: String,
        }
        let Ok(resp) = net::get_json::<Resp>(&format!("{}/api/news", site.trim_end_matches('/'))).await else { return };
        let mut items = Vec::new();
        // Відкриватимемо лише звичайні веб-посилання.
        for n in resp.news.into_iter().filter(|n| n.url.starts_with("https://") || n.url.starts_with("http://")).take(3) {
            // Обкладинку зменшуємо одразу: у картці вона невелика.
            let cover = match &n.cover {
                Some(url) => async {
                    let bytes = net::get_bytes(url, 10 * 1024 * 1024).await.ok()?;
                    let img = decode_image(&bytes, 6000).ok()?.thumbnail(640, 360).to_rgba8();
                    Some((img.width(), img.height(), img.into_raw()))
                }
                .await,
                None => None,
            };
            let date = n.published_at.get(..10).map(|d| {
                let p: Vec<&str> = d.split('-').collect();
                if p.len() == 3 { format!("{}.{}.{}", p[2], p[1], p[0]) } else { d.to_owned() }
            });
            items.push((n.title, n.summary, n.url, cover, date.unwrap_or_default()));
        }
        let _ = weak.upgrade_in_event_loop(move |ui| {
            let model: Vec<NewsItem> = items
                .into_iter()
                .map(|(title, summary, url, cover, date)| NewsItem {
                    title: title.into(),
                    summary: summary.into(),
                    url: url.into(),
                    has_cover: cover.is_some(),
                    cover: cover.map(|(w, h, d)| image_rgba(&d, w, h)).unwrap_or_default(),
                    date: date.into(),
                })
                .collect();
            ui.set_news(ModelRc::new(VecModel::from(model)));
        });
    });
}

fn load_face(weak: Weak<AppWindow>, state: Shared, id: String, url: String) {
    if url.is_empty() {
        return;
    }
    rt().spawn(async move {
        if let Ok(face) = auth::skin_face(&url, 8).await {
            state.lock().unwrap().faces.insert(id, face);
            let st = state.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| push_accounts(&ui, &st.lock().unwrap()));
        }
    });
}

struct UiReporter(Weak<AppWindow>, Mutex<std::time::Instant>);

impl launch::Reporter for UiReporter {
    fn stage(&self, text: &str, progress: f32) {
        // Не частіше ніж 20 разів на секунду, але фінальні кроки показуємо завжди.
        let mut last = self.1.lock().unwrap();
        if last.elapsed() < Duration::from_millis(50) && progress < 0.999 {
            return;
        }
        *last = std::time::Instant::now();
        let text = text.to_owned();
        let _ = self.0.upgrade_in_event_loop(move |ui| {
            ui.set_status_text(text.into());
            ui.set_progress(progress);
        });
    }
}

fn set_busy(weak: &Weak<AppWindow>, state: &Shared, busy: bool, text: &str) {
    state.lock().unwrap().busy = busy;
    let text = text.to_owned();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        ui.set_busy(busy);
        ui.set_status_text(text.into());
        if busy {
            ui.set_progress(0.0);
        }
    });
}

/// Готує файли (і, якщо `launch`, запускає гру).
fn run(weak: Weak<AppWindow>, state: Shared, launch_game: bool, check_hash: bool) {
    {
        let mut st = state.lock().unwrap();
        if st.busy || st.game_running {
            return;
        }
        st.busy = true;
    }
    set_busy(&weak, &state, true, "Готуємося…");
    rt().spawn(async move {
        let result: anyhow::Result<()> = async {
            // Свіжий маніфест перед кожним запуском: адмін міг оновити збірку.
            let (m, online) = manifest::fetch().await?;
            state.lock().unwrap().manifest = Some(m.clone());
            {
                let m2 = m.clone();
                let _ = weak.upgrade_in_event_loop(move |ui| push_manifest(&ui, &m2, online));
            }
            let (settings, account) = {
                let st = state.lock().unwrap();
                (st.settings.clone(), st.accounts.selected().cloned())
            };
            let account = match (launch_game, account) {
                (true, None) => anyhow::bail!("Спершу додай акаунт"),
                (true, Some(a)) => {
                    if a.kind == Kind::Offline && !offline_allowed(&state.lock().unwrap().accounts) {
                        anyhow::bail!("Гра без ліцензії доступна після входу з ліцензійним акаунтом. Або завантаж Altherra Offline.");
                    }
                    #[cfg(feature = "microsoft")]
                    let fresh = auth::ensure_fresh(&a, &ms_client_id(&m)).await?;
                    #[cfg(not(feature = "microsoft"))]
                    let fresh = a.clone();
                    if fresh.access_token != a.access_token {
                        let mut st = state.lock().unwrap();
                        st.accounts.upsert(fresh.clone());
                        st.accounts.save()?;
                    }
                    Some(fresh)
                }
                (false, a) => a,
            };
            let reporter = Arc::new(UiReporter(weak.clone(), Mutex::new(std::time::Instant::now())));
            let prepared = launch::prepare(&m, &settings, check_hash, reporter).await?;
            if !launch_game {
                set_busy(&weak, &state, false, "Файли перевірено — усе на місці.");
                return Ok(());
            }
            let account = account.expect("account");
            let st_log = state.clone();
            let mut child = launch::spawn(&prepared, &m, &settings, &account, move |line| st_log.lock().unwrap().pending_log.push(line))?;

            let (kill_tx, kill_rx) = tokio::sync::oneshot::channel();
            {
                let mut st = state.lock().unwrap();
                st.busy = false;
                st.game_running = true;
                st.kill = Some(kill_tx);
            }
            let after = settings.after_launch.clone();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                ui.set_game_running(true);
                ui.set_status_text(format!("Гра запущена · {}", account.name).into());
                match after.as_str() {
                    "minimize" => ui.window().set_minimized(true),
                    "close" => {
                        let _ = ui.hide();
                    }
                    _ => {}
                }
            });

            let status = tokio::select! {
                s = child.wait() => s?,
                _ = kill_rx => { let _ = child.kill().await; child.wait().await? }
            };
            state.lock().unwrap().game_running = false;
            let crashed = !status.success();
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_game_running(false);
                if settings.after_launch == "close" {
                    slint::quit_event_loop().ok();
                    return;
                }
                ui.window().set_minimized(false);
                if crashed {
                    ui.set_status_text(format!("Гра завершилася з помилкою (код {})", status.code().unwrap_or(-1)).into());
                    ui.set_page(3);
                    let _ = ui.show();
                } else {
                    ui.set_status_text("".into());
                }
            });
            Ok(())
        }
        .await;
        if let Err(e) = result {
            let msg = err_text(&e);
            {
                let mut st = state.lock().unwrap();
                st.busy = false;
                st.game_running = false;
            }
            let _ = weak.upgrade_in_event_loop(move |ui| {
                ui.set_busy(false);
                ui.set_game_running(false);
                ui.set_status_text("".into());
                let _ = ui.show();
                show_toast(&ui, &msg);
            });
        }
    });
}

// ---------- Старт ----------

/// Службовий режим без вікна: `altherra-launcher --cli [verify] [--offline Нік] [--no-connect]`.
/// Корисний для діагностики: усе, що робить кнопка «Грати», з виводом у консоль.
fn cli(args: &[String]) -> anyhow::Result<()> {
    struct Print;
    impl launch::Reporter for Print {
        fn stage(&self, text: &str, p: f32) {
            eprint!("\r[{:>3.0}%] {:<70}", p * 100.0, text);
        }
    }
    if args.iter().any(|a| a == "update") {
        return rt().block_on(async {
            eprintln!("Поточна версія: {}", update::current());
            match update::check().await? {
                None => eprintln!("Оновлень немає."),
                Some(a) => {
                    eprintln!("Доступна {} — встановлюємо…", a.version);
                    let exe = update::install(&a, |p| eprint!("\r{:>3.0}%", p * 100.0)).await?;
                    eprintln!("\nОновлено: {}", exe.display());
                }
            }
            Ok(())
        });
    }
    let verify = args.iter().any(|a| a == "verify");
    let nick = args.iter().position(|a| a == "--offline").and_then(|i| args.get(i + 1));
    if nick.is_some() && !offline_allowed(&Accounts::load()) {
        anyhow::bail!("в офіційній збірці гра без ліцензії доступна після входу з ліцензійним акаунтом");
    }
    rt().block_on(async {
        let (m, online) = manifest::fetch().await?;
        eprintln!("Збірка: Minecraft {} · {} {} · {} файлів · ревізія {}{}", m.minecraft, m.loader.kind, m.loader.version, m.files.len(), m.revision, if online { "" } else { " (з кешу)" });
        let mut settings = Settings::load();
        if args.iter().any(|a| a == "--no-connect") {
            settings.auto_connect = false;
        }
        let prepared = launch::prepare(&m, &settings, verify, Arc::new(Print)).await?;
        eprintln!();
        let Some(nick) = nick else {
            eprintln!("Файли готові.");
            return Ok(());
        };
        let acc = auth::offline(nick)?;
        let mut child = launch::spawn(&prepared, &m, &settings, &acc, |l| println!("{l}"))?;
        let status = child.wait().await?;
        eprintln!("Гра завершилася: {status}");
        Ok(())
    })
}

fn main() -> Result<(), slint::PlatformError> {
    let _ = std::fs::create_dir_all(paths::root());
    update::cleanup();
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--cli") {
        if let Err(e) = cli(&args) {
            eprintln!("\nПомилка: {}", err_text(&e));
            std::process::exit(1);
        }
        return Ok(());
    }
    let ui = AppWindow::new()?;
    // На Linux панель задач шукає ярлик (і його іконку) за цим ідентифікатором — у кожної версії свій.
    // Задається до показу вікна.
    if let Err(e) = slint::set_xdg_app_id(if OFFICIAL { "altherra-launcher" } else { "altherra-offline" }) {
        eprintln!("app_id: {e}");
    }
    let state: Shared = Arc::new(Mutex::new(State { accounts: Accounts::load(), settings: Settings::load(), ..Default::default() }));

    ui.set_version(env!("CARGO_PKG_VERSION").into());
    ui.set_edition_official(OFFICIAL);
    ui.global::<Theme>().set_offline(!OFFICIAL);
    ui.set_game_dir(paths::game_dir().to_string_lossy().into_owned().into());
    {
        let st = state.lock().unwrap();
        push_accounts(&ui, &st);
        push_settings(&ui, &st.settings, 4096);
        // Збережений маніфест показуємо одразу, поки йде запит до сервера.
        if let Some(m) = std::fs::read(paths::manifest_cache()).ok().and_then(|b| serde_json::from_slice::<Manifest>(&b).ok()) {
            push_manifest(&ui, &m, true);
            push_settings(&ui, &st.settings, m.recommended_ram_mb);
        }
        for a in &st.accounts.list {
            if a.kind == Kind::Microsoft {
                load_face(ui.as_weak(), state.clone(), a.id.clone(), a.skin_url.clone());
            }
        }
    }
    load_manifest(ui.as_weak(), state.clone());
    ping_loop(ui.as_weak(), state.clone());

    // Оновлення лаунчера: перевіряємо при старті, ставимо за кнопкою.
    let pending_update: Arc<Mutex<Option<update::Available>>> = Arc::default();
    {
        let (weak, pending) = (ui.as_weak(), pending_update.clone());
        rt().spawn(async move {
            if let Ok(Some(a)) = update::check().await {
                let v = a.version.clone();
                *pending.lock().unwrap() = Some(a);
                let _ = weak.upgrade_in_event_loop(move |ui| ui.set_update_version(v.into()));
            }
        });
    }
    {
        let (weak, state, pending) = (ui.as_weak(), state.clone(), pending_update.clone());
        ui.on_do_update(move || {
            let Some(a) = pending.lock().unwrap().take() else { return };
            if update::asset_name().is_none() {
                let _ = open::that_detached(&a.page);
                *pending.lock().unwrap() = Some(a);
                return;
            }
            {
                let mut st = state.lock().unwrap();
                if st.busy || st.game_running {
                    *pending.lock().unwrap() = Some(a);
                    return;
                }
                st.busy = true;
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_update_busy(true);
                ui.set_busy(true);
                ui.set_status_text(format!("Оновлюємо лаунчер до {}…", a.version).into());
            }
            let (weak, state, pending) = (weak.clone(), state.clone(), pending.clone());
            rt().spawn(async move {
                let w = weak.clone();
                let res = update::install(&a, move |p| {
                    let _ = w.upgrade_in_event_loop(move |ui| ui.set_progress(p));
                })
                .await;
                match res {
                    Ok(exe) => {
                        // Запускаємо нову версію й закриваємося.
                        let started = std::process::Command::new(&exe).spawn().is_ok();
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            if started {
                                let _ = slint::quit_event_loop();
                            } else {
                                ui.set_busy(false);
                                ui.set_update_busy(false);
                                ui.set_update_version("".into());
                                ui.set_status_text("Оновлено. Перезапусти лаунчер.".into());
                            }
                        });
                    }
                    Err(e) => {
                        let msg = format!("Не вдалося оновити: {}", err_text(&e));
                        state.lock().unwrap().busy = false;
                        *pending.lock().unwrap() = Some(a);
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.set_busy(false);
                            ui.set_update_busy(false);
                            ui.set_status_text("".into());
                            show_toast(&ui, &msg);
                        });
                    }
                }
            });
        });
    }

    // Журнал гри: зливаємо накопичене в модель кілька разів на секунду.
    let log_model: Rc<VecModel<SharedString>> = Rc::new(VecModel::default());
    ui.set_log_lines(ModelRc::from(log_model.clone()));
    let log_timer = slint::Timer::default();
    {
        let state = state.clone();
        let log_model = log_model.clone();
        log_timer.start(slint::TimerMode::Repeated, Duration::from_millis(250), move || {
            let lines = std::mem::take(&mut state.lock().unwrap().pending_log);
            if lines.is_empty() {
                return;
            }
            for l in lines {
                log_model.push(l.into());
            }
            let extra = log_model.row_count().saturating_sub(MAX_LOG_LINES);
            if extra > 0 {
                let keep: Vec<SharedString> = (extra..log_model.row_count()).filter_map(|i| log_model.row_data(i)).collect();
                log_model.set_vec(keep);
            }
        });
    }

    // Кнопки
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_play(move || run(weak.clone(), state.clone(), true, false));
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_verify_files(move || run(weak.clone(), state.clone(), false, true));
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_refresh(move || load_manifest(weak.clone(), state.clone()));
    }
    ui.on_open_url(|url| {
        let url = url.to_string();
        if url.starts_with("https://") || url.starts_with("http://") {
            let _ = open::that_detached(url);
        }
    });
    ui.on_open_game_dir(|| {
        let _ = std::fs::create_dir_all(paths::game_dir());
        let _ = open::that_detached(paths::game_dir());
    });
    ui.on_open_logs_dir(|| {
        let dir = paths::game_dir().join("logs");
        let _ = std::fs::create_dir_all(&dir);
        let _ = open::that_detached(dir);
    });
    {
        let state = state.clone();
        ui.on_kill_game(move || {
            if let Some(k) = state.lock().unwrap().kill.take() {
                let _ = k.send(());
            }
        });
    }

    // Налаштування
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_save_settings(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            let recommended = st.manifest.as_ref().map(|m| m.recommended_ram_mb).unwrap_or(4096);
            let mb = (ui.get_ram_gb().round() as u32) * 1024;
            st.settings.ram_mb = if mb == recommended { 0 } else { mb };
            st.settings.java_path = ui.get_java_path().to_string();
            st.settings.jvm_args = ui.get_jvm_args().to_string();
            st.settings.auto_connect = ui.get_auto_connect();
            st.settings.after_launch = ["keep", "minimize", "close"][ui.get_after_launch().clamp(0, 2) as usize].to_owned();
            st.settings.save();
        });
    }

    // Акаунти
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_select_account(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            st.accounts.selected = Some(id.to_string());
            let _ = st.accounts.save();
            push_accounts(&ui, &st);
            ui.set_accounts_open(false);
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_remove_account(move |id| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            st.accounts.list.retain(|a| a.id != id.as_str());
            if st.accounts.selected.as_deref() == Some(id.as_str()) {
                st.accounts.selected = st.accounts.list.first().map(|a| a.id.clone());
            }
            let _ = st.accounts.save();
            push_accounts(&ui, &st);
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_add_offline(move |nick| {
            let Some(ui) = weak.upgrade() else { return };
            if !offline_allowed(&state.lock().unwrap().accounts) {
                ui.set_acc_error("Спершу увійди з ліцензією Microsoft — після цього можна додавати й ніки без ліцензії.".into());
                return;
            }
            match auth::offline(nick.trim()) {
                Ok(acc) => {
                    let mut st = state.lock().unwrap();
                    st.accounts.upsert(acc);
                    if let Err(e) = st.accounts.save() {
                        ui.set_acc_error(format!("Не вдалося зберегти: {e}").into());
                        return;
                    }
                    push_accounts(&ui, &st);
                    ui.set_offline_nick("".into());
                    ui.set_acc_error("".into());
                    ui.set_accounts_open(false);
                }
                Err(e) => ui.set_acc_error(e.to_string().into()),
            }
        });
    }
    // Вхід через Microsoft є лише в офіційній збірці; в Altherra Offline цього коду немає зовсім.
    #[cfg(feature = "microsoft")]
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_ms_login(move || {
            let Some(ui) = weak.upgrade() else { return };
            let client_id = state.lock().unwrap().manifest.as_ref().map(ms_client_id).unwrap_or_default();
            if client_id.is_empty() {
                ui.set_acc_error("Вхід через Microsoft ще не налаштований на сервері.".into());
                return;
            }
            let (tx, rx) = tokio::sync::watch::channel(false);
            state.lock().unwrap().ms_cancel = Some(tx);
            ui.set_acc_error("".into());
            ui.set_ms_code("".into());
            ui.set_ms_waiting(true);
            let (weak, state) = (weak.clone(), state.clone());
            rt().spawn(async move {
                let res = async {
                    let dc = auth::start_device_login(&client_id).await?;
                    let (code, url) = (dc.user_code.clone(), dc.verification_uri.clone());
                    let _ = open::that_detached(&url);
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.set_ms_code(code.into());
                        ui.set_ms_url(url.into());
                    });
                    auth::finish_device_login(&client_id, &dc, &rx).await
                }
                .await;
                let cancelled = *rx.borrow();
                match res {
                    Ok(acc) => {
                        let (id, skin) = (acc.id.clone(), acc.skin_url.clone());
                        let save_err = {
                            let mut st = state.lock().unwrap();
                            st.accounts.upsert(acc);
                            st.accounts.save().err()
                        };
                        load_face(weak.clone(), state.clone(), id, skin);
                        let st = state.clone();
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.set_ms_waiting(false);
                            push_accounts(&ui, &st.lock().unwrap());
                            match save_err {
                                Some(e) => ui.set_acc_error(format!("Не вдалося зберегти акаунт: {e}").into()),
                                None => ui.set_accounts_open(false),
                            }
                        });
                    }
                    Err(e) => {
                        let msg = if cancelled { String::new() } else { err_text(&e) };
                        let _ = weak.upgrade_in_event_loop(move |ui| {
                            ui.set_ms_waiting(false);
                            ui.set_acc_error(msg.into());
                        });
                    }
                }
            });
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_ms_cancel(move || {
            if let Some(tx) = state.lock().unwrap().ms_cancel.take() {
                let _ = tx.send(true);
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_ms_waiting(false);
            }
        });
    }

    // Закриття вікна завершує лаунчер (гра, якщо запущена, працює далі).
    ui.window().on_close_requested(|| {
        let _ = slint::quit_event_loop();
        slint::CloseRequestResponse::HideWindow
    });
    ui.show()?;
    slint::run_event_loop_until_quit()?;
    drop(log_timer);
    Ok(())
}
