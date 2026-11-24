//! Вхід через Discord: перед запуском гри лаунчер «стукає» на сервер так само, як гра (протокол Minecraft,
//! стан входу), але з позначкою «проба» у відповіді altherra:mods. Сервер не пускає, а відʼєднує з відповіддю:
//!  - `ok` + ключ — вхід дозволено, ключ кладемо туди, звідки його візьме мод Altherra Client;
//!  - `code` — гравець має надіслати код боту в Discord; лаунчер показує його й питає знову, поки не підтвердять.
//! Ключ одноразовий: сервер щоразу видає новий.
use crate::paths;
use anyhow::{Result, bail};
use std::collections::HashMap;
use std::io::Read;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub enum Probe {
    /// Можна запускати гру; Some — новий ключ входу.
    Ok(Option<String>),
    /// Треба підтвердити вхід у Discord.
    Code { code: String, link: bool, bot: String },
    /// Сервер не знає про вхід через Discord (старий мод, ліцензійний вхід тощо) — запускаємо як є.
    Skip,
}

const PREFIX: &str = "ALTHERRA_AUTH ";

// ---------- Ключі (той самий файл, що й у моді Altherra Client) ----------

fn keys_file() -> std::path::PathBuf {
    paths::game_dir().join("altherra").join("login-keys.json")
}

fn key_id(address: &str, nick: &str) -> String {
    format!("{}|{}", address.trim().to_lowercase(), nick.to_lowercase())
}

fn load_keys() -> HashMap<String, String> {
    std::fs::read(keys_file()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

pub fn load_key(address: &str, nick: &str) -> String {
    load_keys().remove(&key_id(address, nick)).unwrap_or_default()
}

pub fn save_key(address: &str, nick: &str, key: &str) {
    let mut m = load_keys();
    m.insert(key_id(address, nick), key.to_owned());
    let file = keys_file();
    if let Some(dir) = file.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(b) = serde_json::to_vec(&m) {
        let tmp = file.with_extension("json.tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(&tmp, &file);
        }
    }
}

// ---------- Протокол ----------

fn varint(mut v: i32, out: &mut Vec<u8>) {
    loop {
        let mut b = (v & 0x7f) as u8;
        v = ((v as u32) >> 7) as i32;
        if v != 0 {
            b |= 0x80;
        }
        out.push(b);
        if v == 0 {
            break;
        }
    }
}

fn string(s: &str, out: &mut Vec<u8>) {
    varint(s.len() as i32, out);
    out.extend_from_slice(s.as_bytes());
}

fn read_varint_buf(buf: &[u8], pos: &mut usize) -> Result<i32> {
    let mut result = 0i32;
    for i in 0..5 {
        let Some(&b) = buf.get(*pos) else { bail!("обрізаний пакет") };
        *pos += 1;
        result |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    bail!("поганий varint")
}

fn read_string_buf(buf: &[u8], pos: &mut usize) -> Result<String> {
    let n = read_varint_buf(buf, pos)? as usize;
    let Some(s) = buf.get(*pos..*pos + n) else { bail!("обрізаний рядок") };
    *pos += n;
    Ok(String::from_utf8_lossy(s).into_owned())
}

async fn read_varint(s: &mut TcpStream) -> Result<i32> {
    let mut result = 0i32;
    for i in 0..5 {
        let b = s.read_u8().await?;
        result |= ((b & 0x7f) as i32) << (7 * i);
        if b & 0x80 == 0 {
            return Ok(result);
        }
    }
    bail!("поганий varint")
}

struct Conn {
    s: TcpStream,
    threshold: i32,
}

impl Conn {
    async fn send(&mut self, id: i32, body: &[u8]) -> Result<()> {
        let mut inner = Vec::new();
        varint(id, &mut inner);
        inner.extend_from_slice(body);
        if self.threshold >= 0 {
            // Наші пакети малі — шлемо нестиснутими (довжина даних 0).
            let mut c = vec![0u8];
            c.extend(inner);
            inner = c;
        }
        let mut out = Vec::new();
        varint(inner.len() as i32, &mut out);
        out.extend(inner);
        self.s.write_all(&out).await?;
        Ok(())
    }

    async fn recv(&mut self) -> Result<(i32, Vec<u8>)> {
        let n = read_varint(&mut self.s).await? as usize;
        if n > 1 << 21 {
            bail!("завеликий пакет");
        }
        let mut buf = vec![0; n];
        self.s.read_exact(&mut buf).await?;
        let mut pos = 0;
        if self.threshold >= 0 {
            let ulen = read_varint_buf(&buf, &mut pos)? as usize;
            if ulen > 0 {
                let mut out = Vec::with_capacity(ulen.min(1 << 21));
                flate2::read::ZlibDecoder::new(&buf[pos..]).take(1 << 21).read_to_end(&mut out)?;
                buf = out;
                pos = 0;
            }
        }
        let id = read_varint_buf(&buf, &mut pos)?;
        Ok((id, buf[pos..].to_vec()))
    }
}

/// Текст із JSON-компонента повідомлення відʼєднання.
fn flatten(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Object(o) => {
            let mut s = o.get("text").and_then(|t| t.as_str()).unwrap_or("").to_owned();
            if let Some(extra) = o.get("extra").and_then(|e| e.as_array()) {
                for e in extra {
                    s += &flatten(e);
                }
            }
            s
        }
        serde_json::Value::Array(a) => a.iter().map(flatten).collect(),
        _ => String::new(),
    }
}

pub async fn probe(address: &str, nick: &str, key: &str) -> Result<Probe> {
    let (host, port) = match address.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_owned(), p.parse().unwrap()),
        _ => (address.to_owned(), 25565u16),
    };
    let protocol = crate::ping::status(address).await?.protocol;
    tokio::time::timeout(Duration::from_secs(10), async {
        let mut c = Conn { s: TcpStream::connect((host.as_str(), port)).await?, threshold: -1 };
        let mut hs = Vec::new();
        varint(protocol, &mut hs);
        string(&host, &mut hs);
        hs.extend_from_slice(&port.to_be_bytes());
        varint(2, &mut hs); // вхід
        c.send(0x00, &hs).await?;
        let mut hello = Vec::new();
        string(nick, &mut hello);
        hello.extend_from_slice(&[0u8; 16]);
        c.send(0x00, &hello).await?;
        loop {
            let (id, d) = c.recv().await?;
            let mut pos = 0;
            match id {
                0x03 => c.threshold = read_varint_buf(&d, &mut pos)?,
                0x04 => {
                    let msg = read_varint_buf(&d, &mut pos)?;
                    let channel = read_string_buf(&d, &mut pos)?;
                    let mut out = Vec::new();
                    varint(msg, &mut out);
                    if channel == "altherra:mods" {
                        out.push(1);
                        varint(1, &mut out); // протокол altherra:mods
                        varint(0, &mut out); // моди не потрібні — це проба
                        string("", &mut out); // відбиток збірки
                        string(key, &mut out);
                        out.push(1); // проба
                    } else {
                        out.push(0);
                    }
                    c.send(0x02, &out).await?;
                }
                0x00 => {
                    let text = read_string_buf(&d, &mut pos)?;
                    let msg = serde_json::from_str::<serde_json::Value>(&text).map(|v| flatten(&v)).unwrap_or(text);
                    let Some(json) = msg.strip_prefix(PREFIX) else { return Ok(Probe::Skip) };
                    let v: serde_json::Value = serde_json::from_str(json)?;
                    return Ok(match v["status"].as_str() {
                        Some("ok") => Probe::Ok(v["key"].as_str().map(str::to_owned)),
                        Some("code") => Probe::Code {
                            code: v["code"].as_str().unwrap_or_default().to_owned(),
                            link: v["link"].as_bool().unwrap_or(false),
                            bot: v["bot"].as_str().unwrap_or_default().to_owned(),
                        },
                        _ => Probe::Skip,
                    });
                }
                // Шифрування (ліцензійна перевірка) або успішний вхід — проба тут не потрібна.
                _ => return Ok(Probe::Skip),
            }
        }
    })
    .await?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flattens_kick_text() {
        let v: serde_json::Value = serde_json::from_str(r#"{"text":"","extra":[{"text":"ALTHERRA_AUTH "},{"text":"{\"status\":\"ok\"}"}]}"#).unwrap();
        assert_eq!(flatten(&v), "ALTHERRA_AUTH {\"status\":\"ok\"}");
        assert_eq!(key_id(" Altherra.com.ua ", "B3nzo"), "altherra.com.ua|b3nzo");
    }
}
