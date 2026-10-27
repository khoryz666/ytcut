//! Just enough JSON to send flat objects and arrays.

/// `fields` values must already be JSON (use [`string`] for text).
pub fn object(fields: &[(&str, String)]) -> String {
    let fields: Vec<String> = fields
        .iter()
        .map(|(k, v)| format!("{}:{v}", string(k)))
        .collect();
    format!("{{{}}}", fields.join(","))
}

pub fn array(items: impl Iterator<Item = String>) -> String {
    format!("[{}]", items.collect::<Vec<_>>().join(","))
}

pub fn string(s: &str) -> String {
    let mut out = String::from('"');
    for c in s.chars() {
        match c {
            '"' | '\\' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_escaped_objects_and_arrays() {
        let item = object(&[("name", string("a \"b\"\n")), ("size", 3.to_string())]);
        assert_eq!(item, r#"{"name":"a \"b\"\u000a","size":3}"#);
        assert_eq!(
            array([item.clone(), item].into_iter())
                .matches("name")
                .count(),
            2
        );
    }
}
