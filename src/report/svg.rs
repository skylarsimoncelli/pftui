#![allow(dead_code)]

pub fn escape_text(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

pub fn escape_attr(value: &str) -> String {
    escape_text(value)
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn element(name: &str, attrs: &[(&str, String)], content: Option<&str>) -> String {
    let attr_text = attrs
        .iter()
        .map(|(key, value)| format!(r#" {}="{}""#, key, escape_attr(value)))
        .collect::<String>();
    match content {
        Some(content) => format!("<{}{}>{}</{}>", name, attr_text, content, name),
        None => format!("<{}{} />", name, attr_text),
    }
}

pub fn rect(attrs: &[(&str, String)]) -> String {
    element("rect", attrs, None)
}

pub fn line(attrs: &[(&str, String)]) -> String {
    element("line", attrs, None)
}

pub fn text(attrs: &[(&str, String)], content: &str) -> String {
    element("text", attrs, Some(&escape_text(content)))
}

pub fn group(attrs: &[(&str, String)], children: &[String]) -> String {
    element("g", attrs, Some(&children.join("")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_text_like_python_helper() {
        assert_eq!(escape_text("A & B < C > D"), "A &amp; B &lt; C &gt; D");
    }

    #[test]
    fn primitives_escape_attributes_and_children() {
        let rendered = text(&[("data-label", "5\" < 6".to_string())], "A & B");
        assert_eq!(
            rendered,
            r#"<text data-label="5&quot; &lt; 6">A &amp; B</text>"#
        );
    }
}
