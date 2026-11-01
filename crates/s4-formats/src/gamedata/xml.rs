//! Small helpers that mimic how the game's `AdvXMLParser` accessors behave.
//!
//! The readers call `Element::operator()(name, n)` (n-th child element called `name`, or an
//! empty element if there is none) and run `atoi` on its text. So a missing or empty element reads
//! as 0, and trailing garbage after a number is ignored. These helpers reproduce that.

use roxmltree::Node;

/// The `n`-th child element named `name`.
pub fn child<'a, 'i>(node: Node<'a, 'i>, name: &str, n: usize) -> Option<Node<'a, 'i>> {
    node.children().filter(|c| c.is_element() && c.has_tag_name(name)).nth(n)
}

/// All child elements named `name`.
pub fn children<'a, 'i>(node: Node<'a, 'i>, name: &'a str) -> impl Iterator<Item = Node<'a, 'i>> + 'a {
    node.children().filter(move |c| c.is_element() && c.has_tag_name(name))
}

/// All child elements.
pub fn elements<'a, 'i>(node: Node<'a, 'i>) -> impl Iterator<Item = Node<'a, 'i>> {
    node.children().filter(|c| c.is_element())
}

/// Trimmed text of an element ("" if it has none).
pub fn text<'a>(node: Node<'a, '_>) -> &'a str {
    node.text().map(str::trim).unwrap_or("")
}

/// Trimmed text of the first child `name` ("" if missing).
pub fn child_text<'a>(node: Node<'a, '_>, name: &str) -> &'a str {
    child(node, name, 0).map(text).unwrap_or("")
}

/// Text of `a/b` ("" if missing).
pub fn path_text<'a>(node: Node<'a, '_>, a: &str, b: &str) -> &'a str {
    child(node, a, 0).map(|n| child_text(n, b)).unwrap_or("")
}

/// C `atoi`: optional whitespace and sign, then digits; anything else stops; no digits = 0.
pub fn atoi(s: &str) -> i32 {
    let s = s.trim_start();
    let (neg, digits) = match s.as_bytes().first() {
        Some(b'-') => (true, &s[1..]),
        Some(b'+') => (false, &s[1..]),
        _ => (false, s),
    };
    let mut v: i64 = 0;
    for b in digits.bytes() {
        if !b.is_ascii_digit() {
            break;
        }
        v = (v * 10 + (b - b'0') as i64).min(i64::from(i32::MAX) + 1);
    }
    let v = if neg { -v } else { v };
    v.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// `atoi` of the first child `name`.
pub fn child_int(node: Node<'_, '_>, name: &str) -> i32 {
    atoi(child_text(node, name))
}

/// `atoi` of `a/b`.
pub fn path_int(node: Node<'_, '_>, a: &str, b: &str) -> i32 {
    atoi(path_text(node, a, b))
}

/// The `id` attribute ("" if missing), trimmed.
pub fn id<'a>(node: Node<'a, '_>) -> &'a str {
    node.attribute("id").map(str::trim).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::atoi;

    #[test]
    fn atoi_like_c() {
        assert_eq!(atoi(""), 0);
        assert_eq!(atoi(" 42"), 42);
        assert_eq!(atoi("-16777216"), -16777216);
        assert_eq!(atoi("12abc"), 12);
        assert_eq!(atoi("abc"), 0);
    }
}
