//! Статус сервера за протоколом Minecraft (Server List Ping): онлайн і кількість гравців.
use anyhow::{Result, bail};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Status {
    pub online: u32,
    pub max: u32,
    /// Номер протоколу сервера — потрібен, щоб лаунчер міг «постукати» на вхід (discordauth).
    pub protocol: i32,
}

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

fn packet(id: i32, body: &[u8]) -> Vec<u8> {
    let mut inner = Vec::new();
    varint(id, &mut inner);
    inner.extend_from_slice(body);
    let mut out = Vec::new();
    varint(inner.len() as i32, &mut out);
    out.extend(inner);
    out
}

/// «хост[:порт]» → куди насправді підключатися. Як і сама гра, за стандартного порту 25565 дивимося SRV-запис
/// `_minecraft._tcp.хост`: так сайт і Minecraft живуть на одному домені (сайт — через Cloudflare, гра — на іншій адресі).
pub async fn resolve(address: &str) -> (String, u16) {
    let (host, port) = match address.trim().rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_owned(), p.parse().unwrap()),
        _ => (address.trim().to_owned(), 25565u16),
    };
    if port == 25565 && host.parse::<std::net::IpAddr>().is_err() {
        if let Some(target) = srv_lookup(&host).await {
            return target;
        }
    }
    (host, port)
}

/// SRV через DNS-over-HTTPS (Cloudflare): без окремого DNS-клієнта, той самий reqwest, що й для решти.
async fn srv_lookup(host: &str) -> Option<(String, u16)> {
    let url = format!("https://cloudflare-dns.com/dns-query?name=_minecraft._tcp.{host}&type=SRV");
    let resp = reqwest::Client::new()
        .get(url)
        .header("accept", "application/dns-json")
        .timeout(Duration::from_secs(4))
        .send()
        .await
        .ok()?;
    let v: serde_json::Value = resp.json().await.ok()?;
    parse_srv(&v)
}

fn parse_srv(v: &serde_json::Value) -> Option<(String, u16)> {
    // data: «пріоритет вага порт ціль.»; беремо запис із найменшим пріоритетом.
    v["Answer"]
        .as_array()?
        .iter()
        .filter(|a| a["type"].as_u64() == Some(33))
        .filter_map(|a| {
            let parts: Vec<&str> = a["data"].as_str()?.split_whitespace().collect();
            let (prio, port, target) = (parts.first()?.parse::<u32>().ok()?, parts.get(2)?.parse::<u16>().ok()?, parts.get(3)?.trim_end_matches('.'));
            (!target.is_empty()).then(|| (prio, target.to_owned(), port))
        })
        .min_by_key(|(p, _, _)| *p)
        .map(|(_, t, p)| (t, p))
}

pub async fn status(address: &str) -> Result<Status> {
    let (host, port) = resolve(address).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut s = TcpStream::connect((host.as_str(), port)).await?;
        let mut hs = Vec::new();
        varint(-1, &mut hs); // протокол: будь-який для статусу
        varint(host.len() as i32, &mut hs);
        hs.extend_from_slice(host.as_bytes());
        hs.extend_from_slice(&port.to_be_bytes());
        varint(1, &mut hs); // наступний стан: статус
        s.write_all(&packet(0, &hs)).await?;
        s.write_all(&packet(0, &[])).await?;
        let _len = read_varint(&mut s).await?;
        let _id = read_varint(&mut s).await?;
        let n = read_varint(&mut s).await? as usize;
        if n > 1 << 20 {
            bail!("завелика відповідь");
        }
        let mut buf = vec![0; n];
        s.read_exact(&mut buf).await?;
        let v: serde_json::Value = serde_json::from_slice(&buf)?;
        Ok(Status {
            online: v["players"]["online"].as_u64().unwrap_or(0) as u32,
            max: v["players"]["max"].as_u64().unwrap_or(0) as u32,
            protocol: v["version"]["protocol"].as_i64().unwrap_or(-1) as i32,
        })
    })
    .await?
}

#[cfg(test)]
mod tests {
    #[test]
    fn srv() {
        let v: serde_json::Value = serde_json::from_str(r#"{"Status":0,"Answer":[{"name":"_minecraft._tcp.altherra.com.ua","type":33,"TTL":300,"data":"10 5 25570 b.altherra.com.ua."},{"name":"_minecraft._tcp.altherra.com.ua","type":33,"TTL":300,"data":"0 5 25565 play.altherra.com.ua."}]}"#).unwrap();
        assert_eq!(super::parse_srv(&v), Some(("play.altherra.com.ua".into(), 25565)));
        assert_eq!(super::parse_srv(&serde_json::json!({"Status":3})), None);
    }
}
