//! Скільки награно в Minecraft через цей лаунчер (на цьому компʼютері). Час рахується, поки гра запущена,
//! і зберігається щохвилини — тож не пропадає, навіть якщо лаунчер чи компʼютер вимкнуться разом із грою.
use crate::paths;
use serde::{Deserialize, Serialize};

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct Data {
    seconds: u64,
}

fn file() -> std::path::PathBuf {
    paths::root().join("playtime.json")
}

pub fn total() -> u64 {
    std::fs::read(file()).ok().and_then(|b| serde_json::from_slice::<Data>(&b).ok()).unwrap_or_default().seconds
}

/// Додає час і повертає новий загальний.
pub fn add(seconds: u64) -> u64 {
    let total = total().saturating_add(seconds);
    if let Ok(b) = serde_json::to_vec(&Data { seconds: total }) {
        let tmp = file().with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, file());
        }
    }
    total
}

/// «3 год 25 хв», «42 хв», «менше хвилини».
pub fn format(seconds: u64) -> String {
    let (h, m) = (seconds / 3600, seconds / 60 % 60);
    match (h, m) {
        (0, 0) => "менше хвилини".into(),
        (0, m) => format!("{m} хв"),
        (h, 0) => format!("{h} год"),
        (h, m) => format!("{h} год {m} хв"),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn formats() {
        assert_eq!(super::format(30), "менше хвилини");
        assert_eq!(super::format(42 * 60), "42 хв");
        assert_eq!(super::format(7200), "2 год");
        assert_eq!(super::format(3 * 3600 + 25 * 60 + 10), "3 год 25 хв");
    }
}
