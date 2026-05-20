use anyhow::Result;

#[derive(Debug)]
#[allow(unused)]
pub struct Magnet {
    pub file_name: String,
    pub tracker_url: String,
    pub info_hash: [u8; 20],
}

impl Magnet {
    pub fn parse(link: String) -> Result<Self> {
        anyhow::ensure!(&link[..8] == "magnet:?");
        let tokens: Vec<_> = link[8..].split('&').collect();

        let mut file_name = String::new();
        let mut tracker_url = String::new();
        let mut info_hash = Vec::new();

        for token in tokens {
            let Some((lhs, rhs)) = token.split_once('=') else {
                anyhow::bail!("unknow token: {token}");
            };
            match lhs {
                "xt" => {
                    let Some((sig, hash)) = rhs.rsplit_once(':') else {
                        anyhow::bail!("unknow token: {token}");
                    };
                    anyhow::ensure!(sig == "urn:btih");
                    info_hash = hex::decode(hash)?;
                }
                "dn" => {
                    file_name = rhs.to_string();
                }
                "tr" => {
                    tracker_url = decode_url(rhs)?;
                }
                _ => anyhow::bail!("unknow token: {token}"),
            }
        }

        anyhow::ensure!(!file_name.is_empty());
        anyhow::ensure!(!tracker_url.is_empty());
        anyhow::ensure!(!info_hash.is_empty());

        let info_hash: [u8; 20] = info_hash
            .try_into()
            .map_err(|_| anyhow::Error::msg("wrong hash format"))?;

        Ok(Self {
            file_name,
            tracker_url,
            info_hash,
        })
    }
}

fn decode_url(url: &str) -> Result<String> {
    let mut res = String::with_capacity(url.len());
    let mut iter = url.chars();
    while let Some(c) = iter.next() {
        if c == '%' {
            let Some(c1) = iter.next() else {
                anyhow::bail!("decode url failed")
            };
            let Some(c2) = iter.next() else {
                anyhow::bail!("decode url failed")
            };
            let hex_val: String = [c1, c2].iter().collect();
            res.push(char::from((u8::from_str_radix(&hex_val, 16))?));
        } else {
            res.push(c);
        }
    }
    Ok(res)
}

#[cfg(test)]
mod test {
    use anyhow::Result;

    use crate::bittorent::magnet::Magnet;

    #[test]
    fn test_parse_magnet() -> Result<()> {
        let magnet = Magnet::parse(String::from(
            "magnet:?xt=urn:btih:ad42ce8109f54c99613ce38f9b4d87e70f24a165&dn=magnet1.gif&tr=http%3A%2F%2Fbittorrent-test-tracker.codecrafters.io%2Fannounce",
        ))?;

        assert_eq!(magnet.file_name, "magnet1.gif");
        assert_eq!(
            magnet.tracker_url,
            "http://bittorrent-test-tracker.codecrafters.io/announce"
        );
        assert_eq!(
            hex::encode(magnet.info_hash),
            "ad42ce8109f54c99613ce38f9b4d87e70f24a165"
        );

        Ok(())
    }
}
