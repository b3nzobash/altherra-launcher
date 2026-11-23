//! Список серверів гри (servers.dat): Altherra завжди першою, сервери гравця лишаються.
//! Формат — NBT без стиснення. Тут мінімальний читач/записувач усіх типів тегів.
use anyhow::{Result, bail};
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
enum Tag {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    ByteArray(Vec<i8>),
    String(String),
    List(u8, Vec<Tag>),
    Compound(Vec<(String, Tag)>),
    IntArray(Vec<i32>),
    LongArray(Vec<i64>),
}

impl Tag {
    fn id(&self) -> u8 {
        match self {
            Tag::Byte(_) => 1,
            Tag::Short(_) => 2,
            Tag::Int(_) => 3,
            Tag::Long(_) => 4,
            Tag::Float(_) => 5,
            Tag::Double(_) => 6,
            Tag::ByteArray(_) => 7,
            Tag::String(_) => 8,
            Tag::List(..) => 9,
            Tag::Compound(_) => 10,
            Tag::IntArray(_) => 11,
            Tag::LongArray(_) => 12,
        }
    }
}

struct Reader<'a> {
    b: &'a [u8],
    pos: usize,
    depth: usize,
}

impl Reader<'_> {
    fn take(&mut self, n: usize) -> Result<&[u8]> {
        if self.pos + n > self.b.len() {
            bail!("servers.dat обрізаний");
        }
        let s = &self.b[self.pos..self.pos + n];
        self.pos += n;
        Ok(s)
    }
    fn u8(&mut self) -> Result<u8> {
        Ok(self.take(1)?[0])
    }
    fn i16(&mut self) -> Result<i16> {
        Ok(i16::from_be_bytes(self.take(2)?.try_into()?))
    }
    fn i32(&mut self) -> Result<i32> {
        Ok(i32::from_be_bytes(self.take(4)?.try_into()?))
    }
    fn i64(&mut self) -> Result<i64> {
        Ok(i64::from_be_bytes(self.take(8)?.try_into()?))
    }
    fn len(&mut self) -> Result<usize> {
        let n = self.i32()?;
        if n < 0 || n as usize > self.b.len() {
            bail!("неправильна довжина в servers.dat");
        }
        Ok(n as usize)
    }
    fn string(&mut self) -> Result<String> {
        let n = self.i16()? as u16 as usize;
        Ok(String::from_utf8_lossy(self.take(n)?).into_owned())
    }
    fn payload(&mut self, id: u8) -> Result<Tag> {
        self.depth += 1;
        if self.depth > 64 {
            bail!("servers.dat занадто вкладений");
        }
        let t = match id {
            1 => Tag::Byte(self.u8()? as i8),
            2 => Tag::Short(self.i16()?),
            3 => Tag::Int(self.i32()?),
            4 => Tag::Long(self.i64()?),
            5 => Tag::Float(f32::from_bits(self.i32()? as u32)),
            6 => Tag::Double(f64::from_bits(self.i64()? as u64)),
            7 => {
                let n = self.len()?;
                Tag::ByteArray(self.take(n)?.iter().map(|&b| b as i8).collect())
            }
            8 => Tag::String(self.string()?),
            9 => {
                let elem = self.u8()?;
                let n = self.len()?;
                let mut v = Vec::new();
                for _ in 0..n {
                    v.push(self.payload(elem)?);
                }
                Tag::List(elem, v)
            }
            10 => {
                let mut v = Vec::new();
                loop {
                    let tid = self.u8()?;
                    if tid == 0 {
                        break;
                    }
                    let name = self.string()?;
                    v.push((name, self.payload(tid)?));
                }
                Tag::Compound(v)
            }
            11 => {
                let n = self.len()?;
                Tag::IntArray((0..n).map(|_| self.i32()).collect::<Result<_>>()?)
            }
            12 => {
                let n = self.len()?;
                Tag::LongArray((0..n).map(|_| self.i64()).collect::<Result<_>>()?)
            }
            _ => bail!("невідомий тег NBT {id}"),
        };
        self.depth -= 1;
        Ok(t)
    }
}

fn write_str(out: &mut Vec<u8>, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(u16::MAX as usize);
    out.extend_from_slice(&(n as u16).to_be_bytes());
    out.extend_from_slice(&b[..n]);
}

fn write_payload(out: &mut Vec<u8>, t: &Tag) {
    match t {
        Tag::Byte(v) => out.push(*v as u8),
        Tag::Short(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Long(v) => out.extend_from_slice(&v.to_be_bytes()),
        Tag::Float(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::Double(v) => out.extend_from_slice(&v.to_bits().to_be_bytes()),
        Tag::ByteArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            out.extend(v.iter().map(|&b| b as u8));
        }
        Tag::String(s) => write_str(out, s),
        Tag::List(elem, v) => {
            out.push(if v.is_empty() { *elem } else { v[0].id() });
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for e in v {
                write_payload(out, e);
            }
        }
        Tag::Compound(v) => {
            for (name, e) in v {
                out.push(e.id());
                write_str(out, name);
                write_payload(out, e);
            }
            out.push(0);
        }
        Tag::IntArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
        Tag::LongArray(v) => {
            out.extend_from_slice(&(v.len() as i32).to_be_bytes());
            for x in v {
                out.extend_from_slice(&x.to_be_bytes());
            }
        }
    }
}

fn get_str<'a>(c: &'a [(String, Tag)], key: &str) -> Option<&'a str> {
    c.iter().find(|(k, _)| k == key).and_then(|(_, t)| if let Tag::String(s) = t { Some(s.as_str()) } else { None })
}

/// Ставить сервер першим у списку. Якщо він уже є (за адресою) — оновлює назву й переносить нагору.
pub fn ensure_server(path: &Path, name: &str, address: &str) -> Result<()> {
    if address.trim().is_empty() {
        return Ok(());
    }
    let mut servers: Vec<Tag> = Vec::new();
    if let Ok(bytes) = std::fs::read(path) {
        // Пошкоджений файл не чіпаємо, щоб не стерти сервери гравця.
        let mut r = Reader { b: &bytes, pos: 0, depth: 0 };
        if r.u8()? != 10 {
            bail!("servers.dat має незнайомий формат");
        }
        r.string()?;
        if let Tag::Compound(root) = r.payload(10)? {
            if let Some((_, Tag::List(_, list))) = root.into_iter().find(|(k, _)| k == "servers") {
                servers = list;
            }
        }
    }
    let norm = |s: &str| s.trim().trim_end_matches(":25565").to_ascii_lowercase();
    // hidden = 0: гра сама додає «приховані» записи, коли заходить на сервер за адресою з лаунчера,
    // і такий прапорець ховає сервер зі списку «Мережева гра».
    let mut ours = vec![
        ("name".to_owned(), Tag::String(name.to_owned())),
        ("ip".to_owned(), Tag::String(address.to_owned())),
        ("hidden".to_owned(), Tag::Byte(0)),
    ];
    servers.retain(|t| {
        let Tag::Compound(c) = t else { return true };
        if get_str(c, "ip").map(norm) == Some(norm(address)) {
            // Зберігаємо іконку й налаштування ресурспаків, які вже запамʼятала гра.
            for (k, v) in c {
                if k != "name" && k != "ip" && k != "hidden" && !ours.iter().any(|(ok, _)| ok == k) {
                    ours.push((k.clone(), v.clone()));
                }
            }
            return false;
        }
        true
    });
    servers.insert(0, Tag::Compound(ours));

    let root = Tag::Compound(vec![("servers".to_owned(), Tag::List(10, servers))]);
    let mut out = vec![10u8];
    write_str(&mut out, "");
    write_payload(&mut out, &root);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("dat.tmp");
    std::fs::write(&tmp, out)?;
    std::fs::rename(tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unhides_server_hidden_by_the_game() {
        let dir = std::env::temp_dir().join(format!("altherra-servers-hidden-{}", std::process::id()));
        let p = dir.join("servers.dat");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // Так файл залишає гра після входу за адресою з лаунчера: два приховані записи.
        let hidden = |name: &str, ip: &str| Tag::Compound(vec![
            ("hidden".to_owned(), Tag::Byte(1)),
            ("ip".to_owned(), Tag::String(ip.to_owned())),
            ("name".to_owned(), Tag::String(name.to_owned())),
            ("icon".to_owned(), Tag::String("PNG".to_owned())),
        ]);
        let root = Tag::Compound(vec![("servers".to_owned(), Tag::List(10, vec![hidden("Minecraft Server", "altherra.com.ua:25565"), hidden("Altherra", "altherra.com.ua")]))]);
        let mut out = vec![10u8];
        write_str(&mut out, "");
        write_payload(&mut out, &root);
        std::fs::write(&p, out).unwrap();

        ensure_server(&p, "Altherra", "altherra.com.ua").unwrap();
        let bytes = std::fs::read(&p).unwrap();
        let mut r = Reader { b: &bytes, pos: 0, depth: 0 };
        r.u8().unwrap();
        r.string().unwrap();
        let Tag::Compound(root) = r.payload(10).unwrap() else { panic!() };
        let Tag::List(_, list) = &root[0].1 else { panic!() };
        assert_eq!(list.len(), 1, "дублікати прибрано");
        let Tag::Compound(ours) = &list[0] else { panic!() };
        let hidden: Vec<_> = ours.iter().filter(|(k, _)| k == "hidden").collect();
        assert_eq!(hidden.len(), 1);
        assert!(matches!(hidden[0].1, Tag::Byte(0)), "сервер видно в списку");
        assert_eq!(get_str(ours, "icon"), Some("PNG"), "іконку збережено");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn keeps_user_servers_and_puts_ours_first() {
        let dir = std::env::temp_dir().join(format!("altherra-servers-{}", std::process::id()));
        let p = dir.join("servers.dat");
        let _ = std::fs::remove_dir_all(&dir);
        ensure_server(&p, "Інший", "other.example:25566").unwrap();
        ensure_server(&p, "Altherra", "altherra.com.ua").unwrap();
        ensure_server(&p, "Altherra", "altherra.com.ua:25565").unwrap(); // той самий — не дублюємо
        let bytes = std::fs::read(&p).unwrap();
        let mut r = Reader { b: &bytes, pos: 0, depth: 0 };
        assert_eq!(r.u8().unwrap(), 10);
        r.string().unwrap();
        let Tag::Compound(root) = r.payload(10).unwrap() else { panic!() };
        let Tag::List(_, list) = &root[0].1 else { panic!() };
        assert_eq!(list.len(), 2);
        let Tag::Compound(first) = &list[0] else { panic!() };
        assert_eq!(get_str(first, "ip"), Some("altherra.com.ua:25565"));
        let Tag::Compound(second) = &list[1] else { panic!() };
        assert_eq!(get_str(second, "name"), Some("Інший"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
