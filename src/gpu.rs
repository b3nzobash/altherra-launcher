//! Вибір відеокарти для гри (Linux, Mesa): лаунчер показує всі відеокарти з назвою та адресою PCI,
//! а гра запускається з DRI_PRIME / MESA_VK_DEVICE_SELECT на обрану. Корисно для eGPU через Thunderbolt
//! і ноутбуків з двома відеокартами.

#[derive(Clone, Debug, PartialEq)]
pub struct Gpu {
    /// Адреса PCI, напр. «0000:04:00.0».
    pub slot: String,
    /// «AMD Radeon RX 470/480/570/570X/580/580X/590»
    pub name: String,
    /// «1002:67df» — для MESA_VK_DEVICE_SELECT.
    pub id: String,
}

/// Усі відеокарти системи. Не на Linux — порожньо (там відеокарту обирає система).
pub fn list() -> Vec<Gpu> {
    #[cfg(target_os = "linux")]
    {
        linux::list()
    }
    #[cfg(not(target_os = "linux"))]
    {
        Vec::new()
    }
}

/// Змінні середовища для гри, щоб вона малювалася на відеокарті `slot`. None — такої зараз немає (eGPU відʼєднано).
pub fn env_for(slot: &str) -> Option<Vec<(&'static str, String)>> {
    let g = list().into_iter().find(|g| g.slot == slot)?;
    Some(vec![("DRI_PRIME", format!("pci-{}", g.slot.replace([':', '.'], "_"))), ("MESA_VK_DEVICE_SELECT", g.id)])
}

fn vendor_short(id: &str) -> Option<&'static str> {
    match id {
        "1002" => Some("AMD"),
        "10de" => Some("NVIDIA"),
        "8086" => Some("Intel"),
        _ => None,
    }
}

/// Назва з pci.ids: «Ellesmere [Radeon RX 470/480/…]» → «Radeon RX 470/480/…» (у дужках — маркетингова назва).
fn pretty(device: &str) -> String {
    match (device.find('['), device.rfind(']')) {
        (Some(a), Some(b)) if b > a => device[a + 1..b].trim().to_owned(),
        _ => device.trim().to_owned(),
    }
}

/// Шукає назву пристрою в тексті pci.ids.
fn lookup(ids: &str, vendor: &str, device: &str) -> Option<String> {
    let mut in_vendor = false;
    for line in ids.lines() {
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        if !line.starts_with('\t') {
            in_vendor = line.get(..4).is_some_and(|v| v.eq_ignore_ascii_case(vendor));
            continue;
        }
        if in_vendor && !line.starts_with("\t\t") {
            let l = &line[1..];
            if l.get(..4).is_some_and(|d| d.eq_ignore_ascii_case(device)) {
                return Some(l[4..].trim().to_owned());
            }
        }
    }
    None
}

fn display_name(ids: Option<&str>, vendor: &str, device: &str) -> String {
    let v = vendor_short(vendor);
    let model = ids.and_then(|t| lookup(t, vendor, device)).map(|d| pretty(&d));
    match (v, model) {
        (Some(v), Some(m)) if m.starts_with(v) => m,
        (Some(v), Some(m)) => format!("{v} {m}"),
        (None, Some(m)) => m,
        (Some(v), None) => format!("{v} {device}"),
        (None, None) => format!("{vendor}:{device}"),
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;

    pub fn list() -> Vec<Gpu> {
        let ids = ["/usr/share/hwdata/pci.ids", "/usr/share/misc/pci.ids", "/usr/share/pci.ids"]
            .iter()
            .find_map(|p| std::fs::read_to_string(p).ok());
        let mut out: Vec<Gpu> = Vec::new();
        let Ok(rd) = std::fs::read_dir("/sys/class/drm") else { return out };
        let mut cards: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        cards.sort();
        for card in cards {
            let name = card.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            // card0, card1… (без card0-eDP-1 тощо)
            if !name.starts_with("card") || name.contains('-') {
                continue;
            }
            let Ok(uevent) = std::fs::read_to_string(card.join("device/uevent")) else { continue };
            let field = |k: &str| uevent.lines().find_map(|l| l.strip_prefix(k)).map(str::to_owned);
            let (Some(slot), Some(pci_id)) = (field("PCI_SLOT_NAME="), field("PCI_ID=")) else { continue };
            // Лише дисплейні контролери (клас 03xxxx).
            if !field("PCI_CLASS=").and_then(|c| u32::from_str_radix(c.trim(), 16).ok()).is_some_and(|c| c >> 16 == 0x03) {
                continue;
            }
            let pci_id = pci_id.to_ascii_lowercase();
            let (vendor, device) = pci_id.split_once(':').unwrap_or(("", ""));
            if out.iter().any(|g| g.slot == slot) {
                continue;
            }
            out.push(Gpu { name: display_name(ids.as_deref(), vendor, device), id: pci_id.clone(), slot });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const IDS: &str = "# x\n1002  Advanced Micro Devices, Inc. [AMD/ATI]\n\t67df  Ellesmere [Radeon RX 470/480/570/570X/580/580X/590]\n\t\t1043 0517  Something\n8086  Intel Corporation\n\t46a3  Alder Lake-P GT1 [UHD Graphics]\n10de  NVIDIA Corporation\n\t2504  GA106 [GeForce RTX 3060 Lite Hash Rate]\n";

    #[test]
    fn names() {
        assert_eq!(display_name(Some(IDS), "1002", "67df"), "AMD Radeon RX 470/480/570/570X/580/580X/590");
        assert_eq!(display_name(Some(IDS), "8086", "46a3"), "Intel UHD Graphics");
        assert_eq!(display_name(Some(IDS), "10de", "2504"), "NVIDIA GeForce RTX 3060 Lite Hash Rate");
        assert_eq!(display_name(None, "1002", "67df"), "AMD 67df");
    }
}

#[cfg(test)]
mod live {
    /// На машині з кількома відеокартами показує, що бачить лаунчер: cargo test gpu::live -- --nocapture
    #[test]
    fn list_and_env() {
        for g in super::list() {
            println!("{} | {} | {} | {:?}", g.slot, g.name, g.id, super::env_for(&g.slot));
        }
    }
}
