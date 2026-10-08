//! A small JSON reader, enough for a glTF file's header and the model list, so the renderer needs no JSON crate.
//! Numbers are read as `f64`; this is drawing code, never simulation.

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<Value>),
    Object(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(fields) => fields.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn str(&self) -> Option<&str> {
        match self {
            Value::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn num(&self) -> Option<f64> {
        match self {
            Value::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// A whole number that fits an index.
    pub fn index(&self) -> Option<usize> {
        self.num().filter(|n| *n >= 0.0 && n.fract() == 0.0).map(|n| n as usize)
    }

    pub fn bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The items of an array; empty for anything else, so a missing list reads as no items.
    pub fn items(&self) -> &[Value] {
        match self {
            Value::Array(items) => items,
            _ => &[],
        }
    }

    /// The fields of an object; empty for anything else.
    pub fn fields(&self) -> &[(String, Value)] {
        match self {
            Value::Object(fields) => fields,
            _ => &[],
        }
    }

    /// Up to `N` numbers from an array, or `None` if it is not an array of exactly `N` numbers.
    pub fn floats<const N: usize>(&self) -> Option<[f32; N]> {
        let items = self.items();
        if items.len() != N {
            return None;
        }
        let mut out = [0.0; N];
        for (o, v) in out.iter_mut().zip(items) {
            *o = v.num()? as f32;
        }
        Some(out)
    }
}

/// Reads one JSON value, with nothing but white space after it.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut p = Parser { s: text.as_bytes(), at: 0 };
    let v = p.value()?;
    p.space();
    if p.at != p.s.len() {
        return Err(p.error("text after the value"));
    }
    Ok(v)
}

struct Parser<'a> {
    s: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> String {
        format!("JSON: {what} at byte {}", self.at)
    }

    fn space(&mut self) {
        while self.s.get(self.at).is_some_and(|c| c.is_ascii_whitespace()) {
            self.at += 1;
        }
    }

    fn eat(&mut self, c: u8) -> bool {
        self.space();
        let ok = self.s.get(self.at) == Some(&c);
        if ok {
            self.at += 1;
        }
        ok
    }

    fn word(&mut self, w: &str, v: Value) -> Result<Value, String> {
        if self.s[self.at..].starts_with(w.as_bytes()) {
            self.at += w.len();
            Ok(v)
        } else {
            Err(self.error("an unknown word"))
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.space();
        match self.s.get(self.at) {
            Some(b'{') => {
                self.at += 1;
                let mut fields = Vec::new();
                if self.eat(b'}') {
                    return Ok(Value::Object(fields));
                }
                loop {
                    self.space();
                    let key = self.string()?;
                    if !self.eat(b':') {
                        return Err(self.error("a missing ':'"));
                    }
                    fields.push((key, self.value()?));
                    if self.eat(b'}') {
                        return Ok(Value::Object(fields));
                    }
                    if !self.eat(b',') {
                        return Err(self.error("a missing ',' or '}'"));
                    }
                }
            }
            Some(b'[') => {
                self.at += 1;
                let mut items = Vec::new();
                if self.eat(b']') {
                    return Ok(Value::Array(items));
                }
                loop {
                    items.push(self.value()?);
                    if self.eat(b']') {
                        return Ok(Value::Array(items));
                    }
                    if !self.eat(b',') {
                        return Err(self.error("a missing ',' or ']'"));
                    }
                }
            }
            Some(b'"') => Ok(Value::Str(self.string()?)),
            Some(b't') => self.word("true", Value::Bool(true)),
            Some(b'f') => self.word("false", Value::Bool(false)),
            Some(b'n') => self.word("null", Value::Null),
            Some(_) => {
                let start = self.at;
                while self.s.get(self.at).is_some_and(|c| c.is_ascii_digit() || b"+-.eE".contains(c)) {
                    self.at += 1;
                }
                let text = std::str::from_utf8(&self.s[start..self.at]).unwrap_or("");
                text.parse().map(Value::Num).map_err(|_| self.error("a bad number"))
            }
            None => Err(self.error("the end of the text")),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        if self.s.get(self.at) != Some(&b'"') {
            return Err(self.error("a missing string"));
        }
        self.at += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.at) else { return Err(self.error("an unclosed string")) };
            self.at += 1;
            match c {
                b'"' => return String::from_utf8(out).map_err(|_| self.error("a string that is not UTF-8")),
                b'\\' => {
                    let Some(&e) = self.s.get(self.at) else { return Err(self.error("an unclosed string")) };
                    self.at += 1;
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self.s.get(self.at..self.at + 4).ok_or_else(|| self.error("a short \\u"))?;
                            let code = u32::from_str_radix(std::str::from_utf8(hex).unwrap_or("x"), 16)
                                .map_err(|_| self.error("a bad \\u"))?;
                            self.at += 4;
                            // Surrogate pairs never appear in the files read here; one stands in as U+FFFD.
                            let ch = char::from_u32(code).unwrap_or('\u{fffd}');
                            out.extend_from_slice(ch.encode_utf8(&mut [0; 4]).as_bytes());
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_nested_values() {
        let v = parse(r#" {"a": [1, -2.5e1, true, null], "b": {"c": "x\"yé"}} "#).unwrap();
        assert_eq!(v.get("a").unwrap().items()[1].num(), Some(-25.0));
        assert_eq!(v.get("a").unwrap().items()[2].bool(), Some(true));
        assert_eq!(v.get("b").unwrap().get("c").unwrap().str(), Some("x\"y\u{e9}"));
        assert_eq!(parse("[1, 2, 3]").unwrap().floats::<3>(), Some([1.0, 2.0, 3.0]));
        assert!(parse("{\"a\": 1,}").is_err());
        assert!(parse("[1] 2").is_err());
    }
}
