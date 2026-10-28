//! App-name matching mirrors the macOS tier/gap/usage/length ordering.
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

pub fn normalized(value: &str) -> String {
    value
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(char::to_lowercase)
        .collect::<String>()
        .trim()
        .to_owned()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Match {
    pub tier: u8,
    pub gap: usize,
    pub length: usize,
}

pub fn match_name(query: &str, candidate: &str) -> Option<Match> {
    let query = normalized(query);
    let candidate = normalized(candidate);
    let length = candidate.chars().count();
    let tier = if candidate == query {
        0
    } else if candidate.starts_with(&query) {
        1
    } else if candidate
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&query))
    {
        2
    } else {
        3
    };
    let gap = if tier == 3 {
        let mut needle = query.chars().peekable();
        let mut first = None;
        let mut last = 0;
        for (index, c) in candidate.chars().enumerate() {
            if needle.peek() == Some(&c) {
                first.get_or_insert(index);
                last = index;
                needle.next();
            }
        }
        if needle.peek().is_some() {
            return None;
        }
        first.map_or(0, |first| last - first + 1 - query.chars().count())
    } else {
        0
    };
    Some(Match { tier, gap, length })
}

pub fn usage_score(count: u64, latest: u64, now: u64) -> f64 {
    let age_days = now.saturating_sub(latest) as f64 / 86_400.0;
    (count as f64).ln_1p() * 4.0 + (-age_days / 30.0).exp() * 8.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn match_tiers_and_gaps() {
        assert_eq!(match_name("code", "Code").unwrap().tier, 0);
        assert_eq!(match_name("code", "Code Editor").unwrap().tier, 1);
        assert_eq!(match_name("code", "Visual Code").unwrap().tier, 2);
        assert_eq!(match_name("vsc", "Visual Studio Code").unwrap().tier, 3);
        assert_eq!(match_name("ct", "cat").unwrap().gap, 1);
        assert!(match_name("xyz", "Code").is_none());
    }

    #[test]
    fn unicode_and_whitespace_normalization() {
        assert_eq!(match_name("  CAFE ", "Café").unwrap().tier, 0);
        assert_eq!(match_name("éd", "Éditeur").unwrap().tier, 1);
        assert!(match_name("éxyz", "Éditeur").is_none());
        assert!(match_name("", "Anything").is_some());
    }

    #[test]
    fn recent_frequent_usage_scores_higher_and_future_dates_are_safe() {
        assert!(usage_score(10, 1_000_000, 1_000_000) > usage_score(1, 1, 1_000_000));
        assert_eq!(usage_score(1, 100, 1), usage_score(1, 1, 1));
    }
}
