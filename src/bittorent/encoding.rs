use std::{
    collections::BTreeMap,
    fmt::Display,
    io::{BufRead, Cursor, Read},
};

use anyhow::{Context, Result};
use bytes::Buf;

#[derive(Debug, PartialEq)]
pub enum Bencoding {
    Raw(Vec<u8>),
    Integer(i64),
    List(Vec<Bencoding>),
    Dictionary(BTreeMap<String, Bencoding>),
}

impl Display for Bencoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Raw(s) => {
                let s = match str::from_utf8(s) {
                    Ok(s) => s,
                    Err(_) => &hex::encode(s),
                };
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
        Self::decode_from_cursor(&mut cur)
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
                Ok(Self::Integer(num))
            }
            b'l' => {
                let mut list = Vec::new();
                while cur.try_get_u8()? != b'e' {
                    cur.set_position(cur.position() - 1);
                    list.push(Self::decode_from_cursor(cur).context("failed to parse list")?);
                }
                Ok(Self::List(list))
            }
            b'd' => {
                let mut dict = BTreeMap::new();
                while cur.try_get_u8()? != b'e' {
                    cur.set_position(cur.position() - 1);
                    let Bencoding::Raw(key) = Self::decode_from_cursor(cur)? else {
                        anyhow::bail!("key in dictionary must be string");
                    };
                    let key = String::from_utf8(key)?;
                    let val =
                        Self::decode_from_cursor(cur).context("failed to parse dictionary")?;
                    dict.insert(key, val);
                }
                Ok(Self::Dictionary(dict))
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
                Ok(Self::Raw(s))
            }
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::Raw(s) => {
                let mut bytes = s.len().to_string().into_bytes();
                bytes.push(b':');
                bytes.extend(s);
                bytes
            }
            Self::Integer(i) => {
                let mut bytes = vec![b'i'];
                bytes.extend(i.to_string().into_bytes());
                bytes.push(b'e');
                bytes
            }
            Self::List(l) => {
                let mut bytes = vec![b'l'];
                for val in l.iter() {
                    bytes.extend(val.encode());
                }
                bytes.push(b'e');
                bytes
            }
            Self::Dictionary(d) => {
                let mut bytes = vec![b'd'];
                for (key, val) in d.iter() {
                    bytes.extend(Self::Raw(key.clone().into_bytes()).encode());
                    bytes.extend(val.encode());
                }
                bytes.push(b'e');
                bytes
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::{collections::BTreeMap, io::Write};

    use anyhow::Result;

    use crate::bittorent::encoding::Bencoding;

    #[test]
    fn test_string() -> Result<()> {
        let bvalue = Bencoding::Raw(b"Hello world".to_vec());
        let encoded = bvalue.encode();
        assert_eq!(encoded, b"11:Hello world");

        let encoded = b"15:bencoded string".to_vec();
        let decoded = Bencoding::decode(encoded)?;
        assert_eq!(decoded, Bencoding::Raw(b"bencoded string".to_vec()));

        Ok(())
    }

    #[test]
    fn test_integer() -> Result<()> {
        let bvalue = Bencoding::Integer(-123);
        let encoded = bvalue.encode();
        assert_eq!(encoded, b"i-123e");

        let encoded = b"i12345678e".to_vec();
        let decoded = Bencoding::decode(encoded)?;
        assert_eq!(decoded, Bencoding::Integer(12345678));

        Ok(())
    }

    #[test]
    fn test_list() {
        let bvalue = Bencoding::List(vec![
            Bencoding::Raw(b"hello world".to_vec()),
            Bencoding::Integer(123456),
            Bencoding::List(vec![Bencoding::Raw(b"inside nested list".to_vec())]),
            Bencoding::Dictionary(BTreeMap::from([(
                String::from("wololo"),
                Bencoding::Raw(b"inside nested dictionary".to_vec()),
            )])),
        ]);

        assert_eq!(
            bvalue.encode(),
            b"l11:hello worldi123456el18:inside nested listed6:wololo24:inside nested dictionaryee"
        );
    }

    #[test]
    fn test_dictionary() {
        let bvalue = Bencoding::Dictionary(BTreeMap::from([
            (
                String::from("string"),
                Bencoding::Raw(b"hello world".to_vec()),
            ),
            (String::from("integer"), Bencoding::Integer(12345678)),
            (
                String::from("list"),
                Bencoding::List(vec![Bencoding::Raw(b"inside nested list".to_vec())]),
            ),
            (
                String::from("dictionary"),
                Bencoding::Dictionary(BTreeMap::from([(
                    String::from("wololo"),
                    Bencoding::Raw(b"inside nested dictionary".to_vec()),
                )])),
            ),
        ]));

        assert_eq!(
            bvalue.encode(),
            b"d10:dictionaryd6:wololo24:inside nested dictionarye7:integeri12345678e4:listl18:inside nested liste6:string11:hello worlde"
        );
    }

    #[test]
    fn test_display() -> Result<()> {
        let bvalue = Bencoding::Dictionary(BTreeMap::from([
            (
                String::from("string"),
                Bencoding::Raw(b"hello world".to_vec()),
            ),
            (String::from("integer"), Bencoding::Integer(12345678)),
            (
                String::from("list"),
                Bencoding::List(vec![Bencoding::Raw(b"inside nested list".to_vec())]),
            ),
            (
                String::from("dictionary"),
                Bencoding::Dictionary(BTreeMap::from([(
                    String::from("wololo"),
                    Bencoding::Raw(b"inside nested dictionary".to_vec()),
                )])),
            ),
        ]));

        let mut display_output = Vec::new();
        write!(&mut display_output, "{bvalue}")?;

        let expected = r#"{"dictionary":{"wololo":"inside nested dictionary"},"integer":12345678,"list":["inside nested list"],"string":"hello world"}"#;

        assert_eq!(display_output, expected.as_bytes());
        Ok(())
    }
}
