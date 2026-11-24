//! Налаштування гравця.
use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 0 — брати рекомендоване сервером.
    pub ram_mb: u32,
    /// Порожньо — Java від Mojang, яку лаунчер ставить сам.
    pub java_path: String,
    pub jvm_args: String,
    /// "keep" | "minimize" | "close"
    pub after_launch: String,
    pub auto_connect: bool,
    pub width: u32,
    pub height: u32,
    /// Адреса PCI відеокарти для гри (Linux). Порожньо — як вирішить система.
    pub gpu: String,
    /// Назви необовʼязкових модів, які гравець вимкнув (назва не міняється між версіями мода).
    pub disabled_mods: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self { ram_mb: 0, java_path: String::new(), jvm_args: String::new(), after_launch: "minimize".into(), auto_connect: true, width: 0, height: 0, gpu: String::new(), disabled_mods: Vec::new() }
    }
}

impl Settings {
    pub fn load() -> Self {
        std::fs::read(paths::settings_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }
    pub fn save(&self) {
        let _ = std::fs::create_dir_all(paths::root());
        if let Ok(b) = serde_json::to_vec_pretty(self) {
            let _ = std::fs::write(paths::settings_file(), b);
        }
    }
}

/// Загальна памʼять компʼютера в МБ.
pub fn system_ram_mb() -> u32 {
    let Ok(mut sys) = sysinfo::System::new() else { return 8192 };
    sys.refresh_memory();
    (sys.total_memory() / 1024 / 1024) as u32
}
