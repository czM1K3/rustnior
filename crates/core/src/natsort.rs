//! Natural ("human") ordering of file names: `img2` < `img10`, case-insensitive.

use std::cmp::Ordering;

/// Compares two strings treating runs of ASCII digits as numbers.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        match (a.first(), b.first()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let (na, ra) = split_digits(a);
                let (nb, rb) = split_digits(b);
                let ord = cmp_digit_runs(na, nb);
                if ord != Ordering::Equal {
                    return ord;
                }
                a = ra;
                b = rb;
            }
            (Some(_), Some(_)) => {
                // Compare one UTF-8 character at a time, case-insensitively.
                let (ca, la) = next_char(a);
                let (cb, lb) = next_char(b);
                let ord = lower(ca).cmp(&lower(cb));
                if ord != Ordering::Equal {
                    return ord;
                }
                a = &a[la..];
                b = &b[lb..];
            }
        }
    }
}

/// Natural order with a deterministic tie-break on the raw string, so
/// `a.jpg` and `A.jpg` always sort the same way.
pub fn natural_cmp_total(a: &str, b: &str) -> Ordering {
    natural_cmp(a, b).then_with(|| a.cmp(b))
}

fn split_digits(s: &[u8]) -> (&[u8], &[u8]) {
    let n = s.iter().take_while(|c| c.is_ascii_digit()).count();
    s.split_at(n)
}

fn cmp_digit_runs(a: &[u8], b: &[u8]) -> Ordering {
    let trim = |s: &[u8]| -> usize { s.iter().take_while(|&&c| c == b'0').count() };
    let (ta, tb) = (&a[trim(a)..], &b[trim(b)..]);
    ta.len()
        .cmp(&tb.len())
        .then_with(|| ta.cmp(tb))
        // "01" after "1" so equal values with different padding stay stable.
        .then_with(|| a.len().cmp(&b.len()))
}

fn next_char(s: &[u8]) -> (char, usize) {
    let len = match s.first() {
        Some(&c) if c < 0x80 => 1,
        Some(&c) if c >= 0xF0 => 4,
        Some(&c) if c >= 0xE0 => 3,
        Some(_) => 2,
        None => 0,
    }
    .min(s.len());
    let ch = std::str::from_utf8(&s[..len])
        .ok()
        .and_then(|t| t.chars().next())
        .unwrap_or(char::REPLACEMENT_CHARACTER);
    (ch, len.max(1))
}

fn lower(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sorted(mut v: Vec<&str>) -> Vec<&str> {
        v.sort_by(|a, b| natural_cmp_total(a, b));
        v
    }

    #[test]
    fn numbers_sort_numerically() {
        assert_eq!(
            sorted(vec!["img10.jpg", "img2.jpg", "img1.jpg", "img20.jpg"]),
            vec!["img1.jpg", "img2.jpg", "img10.jpg", "img20.jpg"]
        );
    }

    #[test]
    fn case_insensitive_with_stable_tiebreak() {
        assert_eq!(
            sorted(vec!["b.png", "A.png", "a.png"]),
            vec!["A.png", "a.png", "b.png"]
        );
    }

    #[test]
    fn leading_zeros() {
        assert_eq!(natural_cmp("x007", "x7"), Ordering::Greater);
        assert_eq!(natural_cmp("x007", "x8"), Ordering::Less);
        assert_eq!(sorted(vec!["2", "010", "1"]), vec!["1", "2", "010"]);
    }

    #[test]
    fn huge_numbers_do_not_overflow() {
        assert_eq!(
            natural_cmp("a99999999999999999999999", "a100000000000000000000000"),
            Ordering::Less
        );
    }

    #[test]
    fn unicode_and_prefixes() {
        assert_eq!(natural_cmp("été", "Été"), Ordering::Equal);
        assert_eq!(natural_cmp("abc", "abcd"), Ordering::Less);
        assert_eq!(natural_cmp("", ""), Ordering::Equal);
        assert_eq!(natural_cmp("photo 2", "photo10"), Ordering::Less);
    }
}
