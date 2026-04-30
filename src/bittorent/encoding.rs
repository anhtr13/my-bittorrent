use std::{
    collections::BTreeMap,
    fmt::Display,
    io::{BufRead, Cursor, Read},
};

use anyhow::{Context, Result};
use bytes::Buf;

pub enum Bencoding {
    String(Vec<u8>),
    Integer(i64),
    List(Vec<Bencoding>),
    Dictionary(BTreeMap<String, Bencoding>),
}

impl Display for Bencoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => {
                let s = str::from_utf8(&s).unwrap_or("a none-UTF8 string");
                write!(f, "\"{}\"", s)
            }
            Self::Integer(i) => write!(f, "{}", i),
            Self::List(l) => {
                write!(f, "[")?;
                for (i, val) in l.iter().enumerate() {
                    if i + 1 == l.len() {
                        write!(f, "{}", val)?;
                    } else {
                        write!(f, "{},", val)?;
                    }
                }
                write!(f, "]")
            }
            Self::Dictionary(d) => {
                write!(f, "{{")?;
                for (i, (key, val)) in d.iter().enumerate() {
                    if i + 1 == d.len() {
                        write!(f, "\"{}\":{}", key, val)?;
                    } else {
                        write!(f, "\"{}\":{},", key, val)?;
                    }
                }
                write!(f, "}}")
            }
        }
    }
}

impl Bencoding {
    pub fn decode(data: Vec<u8>) -> Result<Self> {
        let mut cur = Cursor::new(data);
        return Self::decode_from_cursor(&mut cur);
    }

    fn decode_from_cursor(cur: &mut Cursor<Vec<u8>>) -> Result<Self> {
        let c = cur.try_get_u8()?;
        match c {
            b'i' => {
                let mut buf = Vec::new();
                cur.read_until(b'e', &mut buf)?;
                buf.pop();
                let num: i64 = str::from_utf8(&buf)
                    .context("failed to parse integer")?
                    .parse()
                    .context("failed to parse integer")?;
                return Ok(Self::Integer(num));
            }
            b'l' => {
                let mut list = Vec::new();
                while cur.get_u8() != b'e' {
                    cur.set_position(cur.position() - 1);
                    list.push(Self::decode_from_cursor(cur).context("failed to parse list")?);
                }
                return Ok(Self::List(list));
            }
            b'd' => {
                let mut dict = BTreeMap::new();
                while cur.get_u8() != b'e' {
                    cur.set_position(cur.position() - 1);
                    let Bencoding::String(key) = Self::decode_from_cursor(cur)? else {
                        anyhow::bail!("key in dictionary must be string");
                    };
                    let key = String::from_utf8(key)?;
                    let val =
                        Self::decode_from_cursor(cur).context("failed to parse dictionary")?;
                    dict.insert(key, val);
                }
                return Ok(Self::Dictionary(dict));
            }
            _ => {
                let mut buf = vec![c];
                cur.read_until(b':', &mut buf)?;
                buf.pop();
                let len: usize = str::from_utf8(&buf)
                    .context("cannot parse string length")?
                    .parse()
                    .context("cannot parse string length")?;
                let mut s = vec![0u8; len];
                cur.read_exact(&mut s).context("failed to parse string")?;
                return Ok(Self::String(s));
            }
        }
    }
}
