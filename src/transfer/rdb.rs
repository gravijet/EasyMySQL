// Redis-Datei (RDB, "dump.rdb") lesen und schreiben.
//
// Lesen versteht die gaengigen Kodierungen bis Redis 7 (Strings, Listen, Mengen, sortierte Mengen,
// Hashes, auch in den kompakten Formen ziplist, listpack, intset und quicklist; LZF-komprimierte
// Strings). Streams und Module werden uebersprungen gemeldet. Schreiben erzeugt eine Datei, die
// Redis ohne Weiteres laden kann (einfache Kodierung ohne Pruefsumme).

use std::io::{Read, Write};

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Str(Vec<u8>),
    List(Vec<Vec<u8>>),
    Set(Vec<Vec<u8>>),
    ZSet(Vec<(Vec<u8>, f64)>),
    Hash(Vec<(Vec<u8>, Vec<u8>)>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub db: u32,
    pub key: Vec<u8>,
    /// Ablaufzeitpunkt in Millisekunden seit 1970
    pub expire_ms: Option<i64>,
    pub item: Item,
}

/// Was beim Lesen nicht uebernommen werden konnte
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Skipped {
    pub keys: u64,
}

struct Rd<R: Read> {
    r: R,
}

type Res<T> = Result<T, String>;

fn bad(what: &str) -> String {
    crate::tr_format!("Redis-Datei: {what}", "Redis file: {what}")
}

fn eof() -> String {
    bad(crate::i18n::tr("unerwartetes Dateiende", "unexpected end of file"))
}

impl<R: Read> Rd<R> {
    fn byte(&mut self) -> Res<u8> {
        let mut b = [0u8; 1];
        self.r.read_exact(&mut b).map_err(|_| eof())?;
        Ok(b[0])
    }

    fn bytes(&mut self, n: usize) -> Res<Vec<u8>> {
        if n > 1 << 30 {
            return Err(bad(crate::i18n::tr("ungültige Länge", "invalid length")));
        }
        let mut v = vec![0u8; n];
        self.r.read_exact(&mut v).map_err(|_| eof())?;
        Ok(v)
    }

    /// Laenge oder Sonderkodierung: (Wert, ist_sonderkodierung)
    fn len(&mut self) -> Res<(u64, bool)> {
        let b = self.byte()?;
        Ok(match b >> 6 {
            0 => ((b & 0x3F) as u64, false),
            1 => ((((b & 0x3F) as u64) << 8) | self.byte()? as u64, false),
            2 => match b {
                0x80 => (u32::from_be_bytes(self.bytes(4)?.try_into().unwrap()) as u64, false),
                0x81 => (u64::from_be_bytes(self.bytes(8)?.try_into().unwrap()), false),
                _ => return Err(bad(crate::i18n::tr("ungültige Länge", "invalid length"))),
            },
            _ => ((b & 0x3F) as u64, true),
        })
    }

    fn plain_len(&mut self) -> Res<usize> {
        let (n, special) = self.len()?;
        if special {
            return Err(bad(crate::i18n::tr("ungültige Länge", "invalid length")));
        }
        Ok(n as usize)
    }

    fn string(&mut self) -> Res<Vec<u8>> {
        let (n, special) = self.len()?;
        if !special {
            return self.bytes(n as usize);
        }
        match n {
            0 => Ok((self.byte()? as i8).to_string().into_bytes()),
            1 => Ok(i16::from_le_bytes(self.bytes(2)?.try_into().unwrap()).to_string().into_bytes()),
            2 => Ok(i32::from_le_bytes(self.bytes(4)?.try_into().unwrap()).to_string().into_bytes()),
            3 => {
                let clen = self.plain_len()?;
                let ulen = self.plain_len()?;
                let data = self.bytes(clen)?;
                lzf(&data, ulen)
            }
            _ => Err(bad(crate::i18n::tr("unbekannte Zeichenketten-Kodierung", "unknown string encoding"))),
        }
    }

    fn double_text(&mut self) -> Res<f64> {
        let n = self.byte()?;
        match n {
            253 => Ok(f64::NAN),
            254 => Ok(f64::INFINITY),
            255 => Ok(f64::NEG_INFINITY),
            _ => {
                let b = self.bytes(n as usize)?;
                String::from_utf8_lossy(&b).parse().map_err(|_| bad(crate::i18n::tr("ungültige Zahl", "invalid number")))
            }
        }
    }

    fn double_bin(&mut self) -> Res<f64> {
        Ok(f64::from_le_bytes(self.bytes(8)?.try_into().unwrap()))
    }
}

/// LZF-Dekomprimierung
fn lzf(data: &[u8], out_len: usize) -> Res<Vec<u8>> {
    let mut out: Vec<u8> = Vec::with_capacity(out_len);
    let mut i = 0;
    let err = || bad(crate::i18n::tr("beschädigte komprimierte Daten", "damaged compressed data"));
    while i < data.len() {
        let ctrl = data[i] as usize;
        i += 1;
        if ctrl < 32 {
            let n = ctrl + 1;
            out.extend_from_slice(data.get(i..i + n).ok_or_else(err)?);
            i += n;
        } else {
            let mut len = ctrl >> 5;
            if len == 7 {
                len += *data.get(i).ok_or_else(err)? as usize;
                i += 1;
            }
            let back = ((ctrl & 0x1F) << 8) + *data.get(i).ok_or_else(err)? as usize + 1;
            i += 1;
            if back > out.len() {
                return Err(err());
            }
            let start = out.len() - back;
            for k in 0..len + 2 {
                let b = out[start + k];
                out.push(b);
            }
        }
        if out.len() > out_len + 1024 {
            return Err(err());
        }
    }
    out.truncate(out_len);
    Ok(out)
}

fn int_text(v: i64) -> Vec<u8> {
    v.to_string().into_bytes()
}

/// Eintraege einer ziplist
fn ziplist(b: &[u8]) -> Res<Vec<Vec<u8>>> {
    let err = || bad(crate::i18n::tr("beschädigte ziplist", "damaged ziplist"));
    let mut out = Vec::new();
    let mut i = 10;
    while i < b.len() && b[i] != 0xFF {
        // vorherige Laenge
        i += if b[i] == 0xFE { 5 } else { 1 };
        let enc = *b.get(i).ok_or_else(err)?;
        i += 1;
        let take = |i: &mut usize, n: usize| -> Res<&[u8]> {
            let s = b.get(*i..*i + n).ok_or_else(err)?;
            *i += n;
            Ok(s)
        };
        let item = match enc >> 6 {
            0 => take(&mut i, (enc & 0x3F) as usize)?.to_vec(),
            1 => {
                let l = (((enc & 0x3F) as usize) << 8) | *b.get(i).ok_or_else(err)? as usize;
                i += 1;
                take(&mut i, l)?.to_vec()
            }
            2 => {
                let l = u32::from_be_bytes(take(&mut i, 4)?.try_into().unwrap()) as usize;
                take(&mut i, l)?.to_vec()
            }
            _ => match enc {
                0xC0 => int_text(i16::from_le_bytes(take(&mut i, 2)?.try_into().unwrap()) as i64),
                0xD0 => int_text(i32::from_le_bytes(take(&mut i, 4)?.try_into().unwrap()) as i64),
                0xE0 => int_text(i64::from_le_bytes(take(&mut i, 8)?.try_into().unwrap())),
                0xF0 => {
                    let s = take(&mut i, 3)?;
                    int_text((((s[0] as i32) | ((s[1] as i32) << 8) | ((s[2] as i32) << 16)) << 8 >> 8) as i64)
                }
                0xFE => int_text(take(&mut i, 1)?[0] as i8 as i64),
                0xF1..=0xFD => int_text((enc & 0x0F) as i64 - 1),
                _ => return Err(err()),
            },
        };
        out.push(item);
    }
    Ok(out)
}

/// Eintraege eines listpack
fn listpack(b: &[u8]) -> Res<Vec<Vec<u8>>> {
    let err = || bad(crate::i18n::tr("beschädigter listpack", "damaged listpack"));
    let mut out = Vec::new();
    let mut i = 6;
    while i < b.len() && b[i] != 0xFF {
        let start = i;
        let enc = b[i];
        i += 1;
        let take = |i: &mut usize, n: usize| -> Res<&[u8]> {
            let s = b.get(*i..*i + n).ok_or_else(err)?;
            *i += n;
            Ok(s)
        };
        let item = if enc & 0x80 == 0 {
            int_text(enc as i64)
        } else if enc & 0xC0 == 0x80 {
            take(&mut i, (enc & 0x3F) as usize)?.to_vec()
        } else if enc & 0xE0 == 0xC0 {
            let v = (((enc & 0x1F) as i64) << 8) | *b.get(i).ok_or_else(err)? as i64;
            i += 1;
            int_text(if v >= 1 << 12 { v - (1 << 13) } else { v })
        } else if enc & 0xF0 == 0xE0 {
            let l = (((enc & 0x0F) as usize) << 8) | *b.get(i).ok_or_else(err)? as usize;
            i += 1;
            take(&mut i, l)?.to_vec()
        } else {
            match enc {
                0xF0 => {
                    let l = u32::from_le_bytes(take(&mut i, 4)?.try_into().unwrap()) as usize;
                    take(&mut i, l)?.to_vec()
                }
                0xF1 => int_text(i16::from_le_bytes(take(&mut i, 2)?.try_into().unwrap()) as i64),
                0xF2 => {
                    let s = take(&mut i, 3)?;
                    int_text((((s[0] as i32) | ((s[1] as i32) << 8) | ((s[2] as i32) << 16)) << 8 >> 8) as i64)
                }
                0xF3 => int_text(i32::from_le_bytes(take(&mut i, 4)?.try_into().unwrap()) as i64),
                0xF4 => int_text(i64::from_le_bytes(take(&mut i, 8)?.try_into().unwrap())),
                _ => return Err(err()),
            }
        };
        // Rueckwaertslaenge ueberspringen
        let size = i - start;
        i += match size {
            0..=127 => 1,
            128..=16382 => 2,
            16383..=2_097_150 => 3,
            2_097_151..=268_435_454 => 4,
            _ => 5,
        };
        out.push(item);
    }
    Ok(out)
}

fn intset(b: &[u8]) -> Res<Vec<Vec<u8>>> {
    let err = || bad(crate::i18n::tr("beschädigtes intset", "damaged intset"));
    if b.len() < 8 {
        return Err(err());
    }
    let width = u32::from_le_bytes(b[0..4].try_into().unwrap()) as usize;
    let n = u32::from_le_bytes(b[4..8].try_into().unwrap()) as usize;
    if !matches!(width, 2 | 4 | 8) || b.len() < 8 + n * width {
        return Err(err());
    }
    Ok((0..n)
        .map(|k| {
            let s = &b[8 + k * width..8 + (k + 1) * width];
            match width {
                2 => int_text(i16::from_le_bytes(s.try_into().unwrap()) as i64),
                4 => int_text(i32::from_le_bytes(s.try_into().unwrap()) as i64),
                _ => int_text(i64::from_le_bytes(s.try_into().unwrap())),
            }
        })
        .collect())
}

fn pairs(v: Vec<Vec<u8>>) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut it = v.into_iter();
    let mut out = Vec::new();
    while let (Some(a), Some(b)) = (it.next(), it.next()) {
        out.push((a, b));
    }
    out
}

fn zpairs(v: Vec<Vec<u8>>) -> Vec<(Vec<u8>, f64)> {
    pairs(v).into_iter().map(|(m, s)| (m, String::from_utf8_lossy(&s).parse().unwrap_or(0.0))).collect()
}

/// Wert lesen; `None` = Typ wird nicht unterstuetzt (und wurde nicht gelesen)
fn value<R: Read>(rd: &mut Rd<R>, t: u8) -> Res<Option<Item>> {
    Ok(Some(match t {
        0 => Item::Str(rd.string()?),
        1 => Item::List((0..rd.plain_len()?).map(|_| rd.string()).collect::<Res<_>>()?),
        2 => Item::Set((0..rd.plain_len()?).map(|_| rd.string()).collect::<Res<_>>()?),
        3 => Item::ZSet((0..rd.plain_len()?).map(|_| Ok((rd.string()?, rd.double_text()?))).collect::<Res<_>>()?),
        5 => Item::ZSet((0..rd.plain_len()?).map(|_| Ok((rd.string()?, rd.double_bin()?))).collect::<Res<_>>()?),
        4 => Item::Hash((0..rd.plain_len()?).map(|_| Ok((rd.string()?, rd.string()?))).collect::<Res<_>>()?),
        10 => Item::List(ziplist(&rd.string()?)?),
        11 => Item::Set(intset(&rd.string()?)?),
        12 => Item::ZSet(zpairs(ziplist(&rd.string()?)?)),
        13 => Item::Hash(pairs(ziplist(&rd.string()?)?)),
        14 => {
            let mut all = Vec::new();
            for _ in 0..rd.plain_len()? {
                all.extend(ziplist(&rd.string()?)?);
            }
            Item::List(all)
        }
        18 => {
            let mut all = Vec::new();
            for _ in 0..rd.plain_len()? {
                let container = rd.plain_len()?;
                let blob = rd.string()?;
                if container == 1 {
                    all.push(blob);
                } else {
                    all.extend(listpack(&blob)?);
                }
            }
            Item::List(all)
        }
        16 => Item::Hash(pairs(listpack(&rd.string()?)?)),
        17 => Item::ZSet(zpairs(listpack(&rd.string()?)?)),
        20 => Item::Set(listpack(&rd.string()?)?),
        _ => return Ok(None),
    }))
}

/// Alle Schluessel der Datei nacheinander an `f` geben; `f` liefert false zum Abbrechen.
pub fn read<R: Read>(input: R, f: &mut dyn FnMut(Entry) -> Res<bool>) -> Res<Skipped> {
    let mut rd = Rd { r: std::io::BufReader::with_capacity(256 * 1024, input) };
    let head = rd.bytes(9)?;
    if &head[..5] != b"REDIS" || !head[5..].iter().all(u8::is_ascii_digit) {
        return Err(bad(crate::i18n::tr("keine gültige RDB-Datei", "not a valid RDB file")));
    }
    let mut db = 0u32;
    let mut expire: Option<i64> = None;
    let skipped = Skipped::default();
    loop {
        let op = rd.byte()?;
        match op {
            0xFF => break,
            0xFA => {
                rd.string()?;
                rd.string()?;
            }
            0xFB => {
                rd.len()?;
                rd.len()?;
            }
            0xFC => expire = Some(i64::from_le_bytes(rd.bytes(8)?.try_into().unwrap())),
            0xFD => expire = Some(u32::from_le_bytes(rd.bytes(4)?.try_into().unwrap()) as i64 * 1000),
            0xFE => db = rd.plain_len()? as u32,
            0xF8 => {
                rd.len()?;
            }
            0xF9 => {
                rd.byte()?;
            }
            t => {
                let key = rd.string()?;
                match value(&mut rd, t)? {
                    Some(item) => {
                        let entry = Entry { db, key, expire_ms: expire.take(), item };
                        if !f(entry)? {
                            return Ok(skipped);
                        }
                    }
                    None => {
                        // Typ ohne Unterstuetzung: der Rest laesst sich nicht sicher lesen
                        return Err(bad(&crate::tr_format!("Typ {t} (Stream oder Modul) wird nicht unterstützt", "type {t} (stream or module) is not supported")));
                    }
                }
            }
        }
    }
    Ok(skipped)
}

// ---------------------------------------------------------------------------
// Schreiben

fn put_len(w: &mut dyn Write, n: usize) -> std::io::Result<()> {
    if n < 64 {
        w.write_all(&[n as u8])
    } else if n < 16384 {
        w.write_all(&[0x40 | (n >> 8) as u8, (n & 0xFF) as u8])
    } else {
        w.write_all(&[0x80])?;
        w.write_all(&(n as u32).to_be_bytes())
    }
}

fn put_str(w: &mut dyn Write, b: &[u8]) -> std::io::Result<()> {
    put_len(w, b.len())?;
    w.write_all(b)
}

/// Schreibt eine RDB-Datei (Version 9, eine Datenbank je `db`-Wechsel)
pub struct Writer<W: Write> {
    w: W,
    db: Option<u32>,
}

impl<W: Write> Writer<W> {
    pub fn new(mut w: W) -> std::io::Result<Self> {
        w.write_all(b"REDIS0009")?;
        w.write_all(&[0xFA])?;
        put_str(&mut w, b"redis-ver")?;
        put_str(&mut w, b"7.0.0")?;
        Ok(Self { w, db: None })
    }

    pub fn entry(&mut self, e: &Entry) -> std::io::Result<()> {
        if self.db != Some(e.db) {
            self.w.write_all(&[0xFE])?;
            put_len(&mut self.w, e.db as usize)?;
            self.db = Some(e.db);
        }
        if let Some(ms) = e.expire_ms {
            self.w.write_all(&[0xFC])?;
            self.w.write_all(&ms.to_le_bytes())?;
        }
        let w: &mut dyn Write = &mut self.w;
        match &e.item {
            Item::Str(v) => {
                w.write_all(&[0])?;
                put_str(w, &e.key)?;
                put_str(w, v)
            }
            Item::List(v) => {
                w.write_all(&[1])?;
                put_str(w, &e.key)?;
                put_len(w, v.len())?;
                v.iter().try_for_each(|x| put_str(w, x))
            }
            Item::Set(v) => {
                w.write_all(&[2])?;
                put_str(w, &e.key)?;
                put_len(w, v.len())?;
                v.iter().try_for_each(|x| put_str(w, x))
            }
            Item::ZSet(v) => {
                w.write_all(&[5])?;
                put_str(w, &e.key)?;
                put_len(w, v.len())?;
                v.iter().try_for_each(|(m, s)| {
                    put_str(w, m)?;
                    w.write_all(&s.to_le_bytes())
                })
            }
            Item::Hash(v) => {
                w.write_all(&[4])?;
                put_str(w, &e.key)?;
                put_len(w, v.len())?;
                v.iter().try_for_each(|(k, x)| {
                    put_str(w, k)?;
                    put_str(w, x)
                })
            }
        }
    }

    pub fn finish(mut self) -> std::io::Result<W> {
        self.w.write_all(&[0xFF])?;
        // Pruefsumme 0 = nicht geprueft (Redis akzeptiert das)
        self.w.write_all(&[0u8; 8])?;
        self.w.flush()?;
        Ok(self.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }

    #[test]
    fn write_then_read_roundtrip() {
        let entries = vec![
            Entry { db: 0, key: b("name"), expire_ms: None, item: Item::Str(b("Müller")) },
            Entry { db: 0, key: b("tmp"), expire_ms: Some(1_900_000_000_000), item: Item::Str(vec![0, 255, 7]) },
            Entry { db: 0, key: b("user:1"), expire_ms: None, item: Item::Hash(vec![(b("a"), b("1")), (b("b"), b("2"))]) },
            Entry { db: 0, key: b("q"), expire_ms: None, item: Item::List(vec![b("x"), b("y"), b("z")]) },
            Entry { db: 0, key: b("s"), expire_ms: None, item: Item::Set(vec![b("m1"), b("m2")]) },
            Entry { db: 1, key: b("z"), expire_ms: None, item: Item::ZSet(vec![(b("p"), 1.5), (b("r"), 2.0)]) },
            Entry { db: 1, key: b("long"), expire_ms: None, item: Item::Str(vec![b'x'; 20_000]) },
        ];
        let mut w = Writer::new(Vec::new()).unwrap();
        for e in &entries {
            w.entry(e).unwrap();
        }
        let bytes = w.finish().unwrap();
        let mut got = Vec::new();
        read(&bytes[..], &mut |e| {
            got.push(e);
            Ok(true)
        })
        .unwrap();
        assert_eq!(got, entries);
        // Abbrechen nach dem ersten Eintrag
        let mut n = 0;
        read(&bytes[..], &mut |_| {
            n += 1;
            Ok(false)
        })
        .unwrap();
        assert_eq!(n, 1);
        assert!(read(&b"NOTREDIS1"[..], &mut |_| Ok(true)).is_err());
    }

    #[test]
    fn compact_encodings() {
        // ziplist: ["a", 5, 300]
        let zl: Vec<u8> = {
            let mut v = vec![0u8; 10];
            v.extend([0, 0x01, b'a']); // prev 0, string len 1
            v.extend([3, 0xF0 | 6]); // prev 3, immediate 5
            v.extend([2, 0xC0, 0x2C, 0x01]); // prev 2, int16 300
            v.push(0xFF);
            v
        };
        assert_eq!(ziplist(&zl).unwrap(), vec![b("a"), b("5"), b("300")]);
        // listpack: [7, "hi", -2]
        let lp: Vec<u8> = {
            let mut v = vec![0u8; 6];
            v.extend([0x07, 0x01]); // 7, backlen 1
            v.extend([0x82, b'h', b'i', 0x03]); // string len 2, backlen 3
            v.extend([0xDF, 0xFE, 0x03]); // 13-bit -2, backlen
            v.push(0xFF);
            v
        };
        assert_eq!(listpack(&lp).unwrap(), vec![b("7"), b("hi"), b("-2")]);
        // intset (Breite 2): 1, 2, -3
        let is: Vec<u8> = [2u32.to_le_bytes().to_vec(), 3u32.to_le_bytes().to_vec(), 1i16.to_le_bytes().to_vec(), 2i16.to_le_bytes().to_vec(), (-3i16).to_le_bytes().to_vec()].concat();
        assert_eq!(intset(&is).unwrap(), vec![b("1"), b("2"), b("-3")]);
        // LZF: Literal "abc", dann Wiederholung der letzten 3 Zeichen
        let packed = [2u8, b'a', b'b', b'c', 0x20, 0x02];
        assert_eq!(lzf(&packed, 6).unwrap(), b("abcabc"));
        assert!(lzf(&[0x20, 0x05], 4).is_err());
    }
}
