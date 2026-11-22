//! Акаунти: ліцензія через Microsoft (вхід за кодом пристрою) і гра без ліцензії.
#![cfg_attr(not(feature = "microsoft"), allow(dead_code))]
use crate::net;
use crate::paths;
use anyhow::{Context, Result, anyhow, bail};
use md5::{Digest, Md5};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Microsoft,
    Offline,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String, // UUID без дефісів
    pub name: String,
    pub kind: Kind,
    #[serde(default)]
    pub refresh_token: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub expires_at: u64,
    #[serde(default)]
    pub skin_url: String,
}

#[derive(Default, Serialize, Deserialize)]
pub struct Accounts {
    pub selected: Option<String>,
    pub list: Vec<Account>,
}

impl Accounts {
    pub fn load() -> Self {
        std::fs::read(paths::accounts_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        std::fs::create_dir_all(paths::root())?;
        let path = paths::accounts_file();
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        // Токени входу — лише для власника файлу.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o600))?;
        }
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn selected(&self) -> Option<&Account> {
        let sel = self.selected.as_deref();
        self.list.iter().find(|a| Some(a.id.as_str()) == sel).or(self.list.first())
    }

    /// Додає або замінює акаунт і робить його вибраним.
    pub fn upsert(&mut self, acc: Account) {
        self.selected = Some(acc.id.clone());
        match self.list.iter_mut().find(|a| a.id == acc.id) {
            Some(a) => *a = acc,
            None => self.list.push(acc),
        }
    }
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

// ---------- Без ліцензії ----------

pub fn valid_nick(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Той самий UUID, який дає сервер в офлайн-режимі: UUID v3 від «OfflinePlayer:нік».
pub fn offline_uuid(name: &str) -> String {
    let mut h: [u8; 16] = Md5::digest(format!("OfflinePlayer:{name}").as_bytes()).into();
    h[6] = (h[6] & 0x0f) | 0x30;
    h[8] = (h[8] & 0x3f) | 0x80;
    hex::encode(h)
}

pub fn offline(name: &str) -> Result<Account> {
    if !valid_nick(name) {
        bail!("Нік: 3–16 символів, лише латиниця, цифри й _");
    }
    Ok(Account { id: offline_uuid(name), name: name.to_owned(), kind: Kind::Offline, refresh_token: String::new(), access_token: String::new(), expires_at: 0, skin_url: String::new() })
}

// ---------- Microsoft ----------

const SCOPE: &str = "XboxLive.signin offline_access";
const AUTHORITY: &str = "https://login.microsoftonline.com/consumers/oauth2/v2.0";

pub struct DeviceCode {
    pub user_code: String,
    pub verification_uri: String,
    device_code: String,
    interval: u64,
    expires_in: u64,
}

pub async fn start_device_login(client_id: &str) -> Result<DeviceCode> {
    let res: Value = net::client()
        .post(format!("{AUTHORITY}/devicecode"))
        .form(&[("client_id", client_id), ("scope", SCOPE)])
        .send()
        .await
        .context("немає звʼязку з Microsoft")?
        .json()
        .await?;
    if let Some(err) = res["error_description"].as_str() {
        bail!("Microsoft: {}", err.lines().next().unwrap_or(err));
    }
    let uri = res["verification_uri"].as_str().filter(|u| u.starts_with("https://")).unwrap_or("https://www.microsoft.com/link");
    Ok(DeviceCode {
        user_code: res["user_code"].as_str().unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(16).collect(),
        verification_uri: uri.to_owned(),
        device_code: res["device_code"].as_str().unwrap_or_default().to_owned(),
        interval: res["interval"].as_u64().unwrap_or(5),
        expires_in: res["expires_in"].as_u64().unwrap_or(900),
    })
}

/// Чекає, поки гравець введе код у браузері, і завершує вхід.
pub async fn finish_device_login(client_id: &str, dc: &DeviceCode, cancel: &tokio::sync::watch::Receiver<bool>) -> Result<Account> {
    let deadline = now() + dc.expires_in;
    let mut interval = dc.interval;
    loop {
        if *cancel.borrow() {
            bail!("Вхід скасовано");
        }
        if now() > deadline {
            bail!("Код застарів — спробуй ще раз");
        }
        tokio::time::sleep(Duration::from_secs(interval)).await;
        let res: Value = net::client()
            .post(format!("{AUTHORITY}/token"))
            .form(&[("client_id", client_id), ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"), ("device_code", &dc.device_code)])
            .send()
            .await?
            .json()
            .await?;
        match res["error"].as_str() {
            None => return complete(&res).await,
            Some("authorization_pending") => continue,
            Some("slow_down") => interval += 5,
            Some("authorization_declined") => bail!("Вхід відхилено"),
            Some("expired_token") => bail!("Код застарів — спробуй ще раз"),
            Some(other) => bail!("Microsoft: {}", res["error_description"].as_str().unwrap_or(other).lines().next().unwrap_or(other)),
        }
    }
}

/// Оновлює токен гри, якщо він от-от закінчиться. Повертає оновлений акаунт.
pub async fn ensure_fresh(acc: &Account, client_id: &str) -> Result<Account> {
    if acc.kind != Kind::Microsoft || acc.expires_at > now() + 300 {
        return Ok(acc.clone());
    }
    if client_id.is_empty() {
        bail!("Вхід через Microsoft ще не налаштований на сервері");
    }
    let res: Value = net::client()
        .post(format!("{AUTHORITY}/token"))
        .form(&[("client_id", client_id), ("grant_type", "refresh_token"), ("refresh_token", &acc.refresh_token), ("scope", SCOPE)])
        .send()
        .await
        .context("немає звʼязку з Microsoft")?
        .json()
        .await?;
    if res["error"].is_string() {
        bail!("Сесія Microsoft закінчилася — увійди в акаунт заново");
    }
    let mut fresh = complete(&res).await?;
    // Microsoft не завжди видає новий refresh-токен — тоді лишаємо старий.
    if fresh.refresh_token.is_empty() {
        fresh.refresh_token = acc.refresh_token.clone();
    }
    Ok(fresh)
}

/// Microsoft → Xbox Live → XSTS → Minecraft → профіль.
async fn complete(ms: &Value) -> Result<Account> {
    let ms_token = ms["access_token"].as_str().ok_or_else(|| anyhow!("Microsoft не видав токен"))?;
    let refresh = ms["refresh_token"].as_str().unwrap_or_default();
    let http = net::client();

    let xbl: Value = http
        .post("https://user.auth.xboxlive.com/user/authenticate")
        .json(&json!({ "Properties": { "AuthMethod": "RPS", "SiteName": "user.auth.xboxlive.com", "RpsTicket": format!("d={ms_token}") }, "RelyingParty": "http://auth.xboxlive.com", "TokenType": "JWT" }))
        .send()
        .await?
        .json()
        .await
        .context("Xbox Live не відповів")?;
    let xbl_token = xbl["Token"].as_str().ok_or_else(|| anyhow!("Xbox Live не прийняв вхід"))?;
    let uhs = xbl["DisplayClaims"]["xui"][0]["uhs"].as_str().unwrap_or_default();

    let xsts: Value = http
        .post("https://xsts.auth.xboxlive.com/xsts/authorize")
        .json(&json!({ "Properties": { "SandboxId": "RETAIL", "UserTokens": [xbl_token] }, "RelyingParty": "rp://api.minecraftservices.com/", "TokenType": "JWT" }))
        .send()
        .await?
        .json()
        .await?;
    if let Some(code) = xsts["XErr"].as_u64() {
        bail!(match code {
            2148916233 => "У цього акаунта Microsoft немає профілю Xbox. Створи його на xbox.com і спробуй знову.",
            2148916235 => "Xbox Live недоступний у твоїй країні.",
            2148916236 | 2148916237 => "Акаунт потребує підтвердження віку на xbox.com.",
            2148916238 => "Дитячий акаунт: його треба додати до сімейної групи Microsoft.",
            _ => "Xbox Live відхилив вхід.",
        });
    }
    let xsts_token = xsts["Token"].as_str().ok_or_else(|| anyhow!("XSTS не видав токен"))?;

    let mc_res = http
        .post("https://api.minecraftservices.com/authentication/login_with_xbox")
        .json(&json!({ "identityToken": format!("XBL3.0 x={uhs};{xsts_token}") }))
        .send()
        .await?;
    if mc_res.status() == reqwest::StatusCode::FORBIDDEN {
        bail!("Mojang ще не схвалив Client ID цього лаунчера для входу в Minecraft. Поки що грай без ліцензії.");
    }
    let mc: Value = mc_res.json().await?;
    let mc_token = mc["access_token"].as_str().ok_or_else(|| anyhow!("Minecraft не видав токен"))?;
    let expires_in = mc["expires_in"].as_u64().unwrap_or(86400);

    let prof_res = http.get("https://api.minecraftservices.com/minecraft/profile").bearer_auth(mc_token).send().await?;
    if prof_res.status() == reqwest::StatusCode::NOT_FOUND {
        bail!("На цьому акаунті немає купленого Minecraft: Java Edition. Можна грати без ліцензії.");
    }
    let prof: Value = prof_res.json().await?;
    let skin_url = prof["skins"]
        .as_array()
        .and_then(|s| s.iter().find(|k| k["state"] == "ACTIVE").or(s.first()))
        .and_then(|k| k["url"].as_str())
        .unwrap_or_default()
        .to_owned();
    Ok(Account {
        id: prof["id"].as_str().ok_or_else(|| anyhow!("немає профілю Minecraft"))?.to_owned(),
        name: prof["name"].as_str().unwrap_or("Player").to_owned(),
        kind: Kind::Microsoft,
        refresh_token: refresh.to_owned(),
        access_token: mc_token.to_owned(),
        expires_at: now() + expires_in,
        skin_url,
    })
}

/// Обличчя зі скіна 8×8 (з другим шаром), збільшене до `scale` пікселів на піксель.
pub async fn skin_face(url: &str, scale: u32) -> Result<(Vec<u8>, u32)> {
    let bytes = net::get_bytes(url, 1024 * 1024).await?;
    let img = crate::decode_image(&bytes, 512)?.to_rgba8();
    // Скін — щонайменше 64×32; менша картинка — не скін.
    if img.width() < 64 || img.height() < 32 {
        bail!("неправильний скін");
    }
    let size = 8 * scale;
    let mut out = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let (sx, sy) = (x / scale, y / scale);
            let base = img.get_pixel(8 + sx, 8 + sy).0;
            let hat = img.get_pixel(40 + sx, 8 + sy).0;
            let px = if hat[3] > 0 { hat } else { base };
            let i = ((y * size + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&px);
        }
    }
    Ok((out, size))
}
