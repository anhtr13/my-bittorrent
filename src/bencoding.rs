use std::{collections::HashMap, fmt::Display, str::Chars};

use anyhow::Result;

pub enum Bencoding {
    String(String),
    Integer(i64),
    List(Vec<Bencoding>),
    Dictionary(HashMap<String, Bencoding>),
}

impl Display for Bencoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::String(s) => write!(f, "\"{}\"", s),
            Self::Integer(i) => write!(f, "{}", i),
            Self::List(l) => {
                write!(f, "[")?;
                for (i, val) in l.iter().enumerate() {
                    if i + 1 == l.len() {
                        write!(f, "{}", val)?;
                    } else {
                        write!(f, "{} ", val)?;
                    }
                }
                write!(f, "]")
            }
            Self::Dictionary(d) => {
                write!(f, "{{")?;
                for (i, (key, val)) in d.iter().enumerate() {
                    if i + 1 == d.len() {
                        write!(f, "{}: {}", key, val)?;
                    } else {
                        write!(f, "{}: {} ", key, val)?;
                    }
                }
                write!(f, "}}")
            }
        }
    }
}

impl Bencoding {
    pub fn decode(iter: &mut Chars) -> Result<Option<Self>> {
        if let Some(c) = iter.next() {
            match c {
                'e' => return Ok(None),
                'i' => {
                    let num = read_util(iter, 'e')?;
                    let num: i64 = num.parse()?;
                    return Ok(Some(Self::Integer(num)));
                }
                'l' => {
                    let len = read_util(iter, ':')?;
                    let len: u64 = len.parse()?;
                    let mut list = Vec::new();
                    for i in 0..len {
                        let element = Self::decode(iter)?;
                        if let Some(element) = element {
                            list.push(element);
                        } else {
                            anyhow::ensure!(
                                i + 1 == len,
                                "list should has length {len}, but only found {}",
                                i + 1
                            );
                        }
                    }
                    return Ok(Some(Self::List(list)));
                }
                'd' => {
                    let mut dict = HashMap::new();
                    while let Some(encoding) = Self::decode(iter)? {
                        let Self::String(key) = encoding else {
                            anyhow::bail!("key in dictionary must be string")
                        };
                        let Some(val) = Self::decode(iter)? else {
                            anyhow::bail!("no corresponding value to key {key} in dictionary")
                        };
                        dict.insert(key, val);
                    }
                    return Ok(Some(Self::Dictionary(dict)));
                }
                c => {
                    let mut len = String::from(c);
                    len.push_str(&read_util(iter, ':')?);
                    let mut len: u64 = len.parse()?;
                    let mut s = String::new();
                    while len > 0
                        && let Some(c) = iter.next()
                    {
                        s.push(c);
                        len -= 1;
                    }
                    anyhow::ensure!(len == 0);
                    return Ok(Some(Self::String(s)));
                }
            }
        }
        anyhow::bail!("Cannot decode")
    }
}

fn read_util(iter: &mut Chars, delimiter: char) -> Result<String> {
    let mut res = String::new();
    while let Some(c) = iter.next() {
        res.push(c);
        if c == delimiter {
            break;
        }
    }
    anyhow::ensure!(res.ends_with(delimiter));
    res.pop();
    Ok(res)
}
