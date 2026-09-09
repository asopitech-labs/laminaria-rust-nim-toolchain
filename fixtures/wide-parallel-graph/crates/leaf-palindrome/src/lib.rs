//! `wide-parallel-graph` fixture — independent leaf crate.

pub fn is_palindrome(s: &str) -> bool {
    let chars: Vec<char> = s.chars().collect();
    let n = chars.len();
    (0..n / 2).all(|i| chars[i] == chars[n - 1 - i])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn racecar_is_a_palindrome() {
        assert!(is_palindrome("racecar"));
    }

    #[test]
    fn hello_is_not_a_palindrome() {
        assert!(!is_palindrome("hello"));
    }
}
