//! "Did you mean ...?" suggestions.

/// Levenshtein edit distance (case-insensitive).
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The closest candidate within a distance that scales with the word length,
/// or `None` if nothing is close. Ties go to the first candidate.
pub fn closest<'a, I>(word: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let limit = (word.chars().count() / 3).max(1) + usize::from(word.len() > 5);
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        let d = edit_distance(word, c);
        let contains = c.contains(word) || word.contains(c);
        let score = if contains && d > limit { limit } else { d };
        if score <= limit && best.is_none_or(|(bd, _)| score < bd) {
            best = Some((score, c));
        }
    }
    best.map(|(_, c)| c)
}

/// A ready-made hint: "did you mean `x`?".
pub fn did_you_mean<'a, I>(word: &str, candidates: I) -> Option<String>
where
    I: IntoIterator<Item = &'a str>,
{
    closest(word, candidates).map(|c| format!("did you mean `{c}`?"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distance() {
        assert_eq!(edit_distance("sprit", "sprite"), 1);
        assert_eq!(edit_distance("kitten", "sitting"), 3);
        assert_eq!(edit_distance("", "abc"), 3);
    }

    #[test]
    fn suggests_close_words_only() {
        let keys = ["sprite", "block", "tile", "solid"];
        assert_eq!(closest("sprit", keys), Some("sprite"));
        assert_eq!(closest("blok", keys), Some("block"));
        assert_eq!(closest("zzzzzz", keys), None);
    }
}
