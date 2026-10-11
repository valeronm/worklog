//! The text of a request's address as what it spells.

fn hex(byte: u8) -> Option<u8> {
    char::from(byte)
        .to_digit(16)
        .and_then(|digit| u8::try_from(digit).ok())
}

pub fn decoded(text: &str, plus_is_space: bool) -> Option<String> {
    let mut bytes = text.bytes();
    let mut out = Vec::with_capacity(text.len());
    while let Some(byte) = bytes.next() {
        match byte {
            b'+' if plus_is_space => out.push(b' '),
            b'%' => {
                let high = hex(bytes.next()?)?;
                let low = hex(bytes.next()?)?;
                out.push((high << 4) | low);
            }
            other => out.push(other),
        }
    }
    String::from_utf8(out).ok()
}

pub struct Query(Vec<(String, String)>);

impl Query {
    // The error is the pair that holds an escape that does not decode.
    pub fn parse(text: &str) -> Result<Query, &str> {
        let mut pairs = Vec::new();
        for pair in text.split('&').filter(|pair| !pair.is_empty()) {
            let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
            let read = |part: &str| decoded(part, true).ok_or(pair);
            pairs.push((read(key)?, read(value)?));
        }
        Ok(Query(pairs))
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        (self.0.iter())
            .find(|(held, _)| held == key)
            .map(|(_, value)| value.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_query_decodes_its_escapes_and_reads_a_plus_as_a_space() {
        assert_eq!(
            decoded("a+b%20c%2Fd%C3%A9", true).as_deref(),
            Some("a b c/dé")
        );
        assert_eq!(decoded("a+b%2B", false).as_deref(), Some("a+b+"));
        assert_eq!(decoded("", true).as_deref(), Some(""));
        for broken in ["100%", "%4", "%zz", "%+1", "%ff"] {
            assert_eq!(decoded(broken, true), None, "{broken}");
        }
        let Ok(query) = Query::parse("q=x+y&all&q=z&=lost&a=b=c") else {
            panic!("a query that decodes");
        };
        assert_eq!(query.get("q"), Some("x y"));
        assert_eq!(query.get("all"), Some(""));
        assert_eq!(query.get("a"), Some("b=c"));
        assert_eq!(query.get("none"), None);
        assert!(Query::parse("q=%zz").is_err());
        assert!(Query::parse("%zz=q").is_err());
    }
}
