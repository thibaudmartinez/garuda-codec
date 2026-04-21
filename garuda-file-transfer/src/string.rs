/// Truncates a UTF-8 string slice to at most `max_bytes` bytes,
/// ensuring that no UTF-8 character is split.
pub fn truncate_utf8(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }

    let mut end = max_bytes;

    // Move backward until we hit a valid char boundary
    while !s.is_char_boundary(end) {
        end -= 1;
    }

    &s[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_original_if_shorter() {
        let s = "hello";
        assert_eq!(truncate_utf8(s, 10), "hello");
    }

    #[test]
    fn truncates_on_ascii_boundary() {
        let s = "hello world";
        assert_eq!(truncate_utf8(s, 5), "hello");
    }

    #[test]
    fn does_not_split_multibyte_char() {
        let s = "héllo"; // 'é' is 2 bytes
        let t = truncate_utf8(s, 2); // would cut inside 'é' if unsafe
        assert!(t.is_char_boundary(t.len()));
        assert_eq!(t, "h");
    }

    #[test]
    fn handles_emoji() {
        let s = "hi 👋 world"; // emoji is multi-byte
        let t = truncate_utf8(s, 5);

        assert!(t.is_char_boundary(t.len()));
        assert_eq!(t, "hi ");
    }

    #[test]
    fn exact_boundary_is_kept() {
        let s = "abcdef";
        assert_eq!(truncate_utf8(s, 3), "abc");
    }
}
