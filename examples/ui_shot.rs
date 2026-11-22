//! Знімки інтерфейсу без вікна (програмний рендерер): cargo run --release --example ui_shot -- <тека>
use slint::platform::software_renderer::{MinimalSoftwareWindow, PremultipliedRgbaColor, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, ModelRc, VecModel};
use std::rc::Rc;

slint::include_modules!();

struct Headless(Rc<MinimalSoftwareWindow>);
impl Platform for Headless {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        Ok(self.0.clone())
    }
}

fn shot(win: &MinimalSoftwareWindow, w: u32, h: u32, path: &str) {
    win.request_redraw();
    let mut buf = vec![PremultipliedRgbaColor::default(); (w * h) as usize];
    win.draw_if_needed(|r| {
        r.render(&mut buf, w as usize);
    });
    let raw: Vec<u8> = buf.iter().flat_map(|p| [p.red, p.green, p.blue, 255]).collect();
    image::save_buffer(path, &raw, w, h, image::ColorType::Rgba8).unwrap();
    println!("{path}");
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let (w, h) = (1120u32, 700u32);
    let win = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    slint::platform::set_platform(Box::new(Headless(win.clone()))).unwrap();
    win.set_size(slint::PhysicalSize::new(w, h));
    let ui = AppWindow::new().unwrap();
    ui.set_server_state(1);
    ui.set_server_status("онлайн · 7 / 50".into());
    ui.set_mc_version("26.3".into());
    ui.set_loader_text("Fabric 0.20.1".into());
    ui.set_mods_count(4);
    ui.set_build_info("Minecraft 26.3 · Fabric 0.20.1 · ревізія 9".into());
    ui.set_ms_available(true);
    let acc = AccountItem { id: "1".into(), name: "b3nzobash".into(), initial: "B".into(), licensed: false, selected: true, ..Default::default() };
    let acc2 = AccountItem { id: "2".into(), name: "Steve_UA".into(), initial: "S".into(), licensed: true, ..Default::default() };
    ui.set_accounts(ModelRc::new(VecModel::from(vec![acc.clone(), acc2])));
    ui.set_files(ModelRc::new(VecModel::from(vec![
        FileItem { name: "Sodium".into(), dir: "mods".into(), size: "1.8 МБ".into() },
        FileItem { name: "Fabric API".into(), dir: "mods".into(), size: "2.4 МБ".into() },
        FileItem { name: "Altherra UI".into(), dir: "resourcepacks".into(), size: "640 КБ".into() },
    ])));
    ui.set_log_lines(ModelRc::new(VecModel::from(vec!["[19:52:01] [Render thread/INFO]: Setting user: b3nzobash".into(), "[19:52:03] [Render thread/INFO]: Backend library: LWJGL version 3.4.3".into()])));
    ui.set_stats_visible(true);
    ui.set_stats_spark("M0 25 L10 22 L20 26 L30 18 L40 12 L50 14 L60 8 L70 10 L80 5 L90 9 L100 7".into());
    ui.set_stats_peak("12".into());
    ui.set_stats_unique("27".into());
    ui.set_stats_tps("19.9".into());
    ui.set_stats_uptime("3 д 4 год".into());
    ui.show().unwrap();

    shot(&win, w, h, &format!("{out}/ui-home-empty.png"));
    ui.set_has_account(true);
    ui.set_account(acc);
    ui.set_busy(true);
    ui.set_progress(0.42);
    ui.set_status_text("Ресурси · 2140 / 5149 · 188.4 МБ".into());
    shot(&win, w, h, &format!("{out}/ui-home-busy.png"));
    ui.set_busy(false);
    ui.set_status_text("".into());
    ui.set_accounts_open(true);
    ui.set_acc_error("Нік: 3–16 символів, лише латиниця, цифри й _".into());
    shot(&win, w, h, &format!("{out}/ui-accounts.png"));
    ui.set_ms_waiting(true);
    ui.set_ms_code("A1B2C3D4".into());
    ui.set_acc_error("".into());
    shot(&win, w, h, &format!("{out}/ui-ms-code.png"));
    ui.set_accounts_open(false);
    ui.set_ms_waiting(false);
    // Офіційна збірка без ліцензійного акаунта і Altherra Offline.
    ui.set_accounts(ModelRc::new(VecModel::from(Vec::<AccountItem>::new())));
    ui.set_offline_allowed(false);
    ui.set_accounts_open(true);
    shot(&win, w, h, &format!("{out}/ui-official-locked.png"));
    ui.set_edition_official(false);
    ui.global::<Theme>().set_offline(true);
    ui.set_offline_allowed(true);
    shot(&win, w, h, &format!("{out}/ui-offline-accounts.png"));
    ui.set_accounts_open(false);
    shot(&win, w, h, &format!("{out}/ui-offline-home.png"));
    ui.set_page(2);
    shot(&win, w, h, &format!("{out}/ui-offline-settings.png"));
    ui.set_page(0);
    ui.global::<Theme>().set_offline(false);
    ui.set_edition_official(true);
    for (i, name) in [(1, "build"), (2, "settings"), (3, "log")] {
        ui.set_page(i);
        shot(&win, w, h, &format!("{out}/ui-{name}.png"));
    }
}
