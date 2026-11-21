//! Статус сервера за протоколом Minecraft (Server List Ping): онлайн і кількість гравців.
use anyhow::{Result, bail};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

pub struct Status {
    pub online: u32,
    pub max: u32,
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

pub async fn status(address: &str) -> Result<Status> {
    let (host, port) = match address.rsplit_once(':') {
        Some((h, p)) if p.parse::<u16>().is_ok() => (h.to_owned(), p.parse().unwrap()),
        _ => (address.to_owned(), 25565u16),
    };
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
        Ok(Status { online: v["players"]["online"].as_u64().unwrap_or(0) as u32, max: v["players"]["max"].as_u64().unwrap_or(0) as u32 })
    })
    .await?
}
