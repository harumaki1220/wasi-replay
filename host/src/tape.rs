use anyhow::Result;

#[derive(Debug, PartialEq)]
enum Value {
    U64(u64),
    Datetime { seconds: u64, nanoseconds: u32 },
}

struct Entry {
    call: String,
    result: Value,
}

impl Value {
    fn to_text(&self) -> String {
        match self {
            Value::U64(n) => n.to_string(),
            Value::Datetime {
                seconds,
                nanoseconds,
            } => format!(
                "datetime {{ seconds: {}, nanoseconds: {} }}",
                seconds, nanoseconds
            ),
        }
    }

    fn parse(s: &str) -> Result<Value> {
        if s.starts_with("datetime") {
            let open = s
                .find('{')
                .ok_or_else(|| anyhow::anyhow!("{{ が見つかりません"))?;
            let close = s
                .find('}')
                .ok_or_else(|| anyhow::anyhow!("}} が見つかりません"))?;
            let inner = &s[open + 1..close];
            let (a, b) = inner
                .split_once(',')
                .ok_or_else(|| anyhow::anyhow!("カンマがありません"))?;
            let (_, sec_str) = a
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("seconds の : がありません"))?;
            let seconds = sec_str.trim().parse::<u64>()?;
            let (_, nano_str) = b
                .split_once(':')
                .ok_or_else(|| anyhow::anyhow!("nanoseconds の : がありません"))?;
            let nanoseconds = nano_str.trim().parse::<u32>()?;
            Ok(Value::Datetime {
                seconds,
                nanoseconds,
            })
        } else {
            let n = s.trim().parse::<u64>()?;
            Ok(Value::U64(n))
        }
    }
}

impl Entry {
    fn to_text(&self) -> String {
        format!("{} -> {}", self.call, self.result.to_text())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u64_to_text() {
        assert_eq!(Value::U64(42).to_text(), "42");
    }

    #[test]
    fn datetime_to_text() {
        let v = Value::Datetime {
            seconds: 1787837699,
            nanoseconds: 250915580,
        };
        assert_eq!(
            v.to_text(),
            "datetime { seconds: 1787837699, nanoseconds: 250915580 }"
        );
    }

    #[test]
    fn entry_to_text() {
        let e = Entry {
            call: "wasi:random/random@0.2.12#get-random-u64".into(),
            result: Value::U64(42),
        };
        assert_eq!(
            e.to_text(),
            "wasi:random/random@0.2.12#get-random-u64 -> 42"
        );
    }

    #[test]
    fn show() {
        let e = Entry {
            call: "wasi:clocks/wall-clock@0.2.12#now".into(),
            result: Value::Datetime {
                seconds: 1787837699,
                nanoseconds: 250915580,
            },
        };
        println!("{}", e.to_text());
    }

    #[test]
    fn parse_u64() {
        assert_eq!(Value::parse("42").unwrap(), Value::U64(42));
    }

    #[test]
    fn parse_datetime() {
        let s = "datetime { seconds: 1787837699, nanoseconds: 250915580 }";
        assert_eq!(
            Value::parse(s).unwrap(),
            Value::Datetime {
                seconds: 1787837699,
                nanoseconds: 250915580
            }
        );
    }

    #[test]
    fn roundtrip() {
        let v = Value::Datetime {
            seconds: 1787837699,
            nanoseconds: 250915580,
        };
        assert_eq!(Value::parse(&v.to_text()).unwrap(), v);

        let v = Value::U64(42);
        assert_eq!(Value::parse(&v.to_text()).unwrap(), v);
    }
}
