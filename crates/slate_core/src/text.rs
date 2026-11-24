//! "Did you mean ...?" suggestions.

/// Edit distance with adjacent transpositions counted as one edit
/// (optimal string alignment), case-insensitive. `rnu` -> `run` is 1.
pub fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.to_lowercase().chars().collect();
    let b: Vec<char> = b.to_lowercase().chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut d = vec![vec![0usize; m + 1]; n + 1];
    for (i, row) in d.iter_mut().enumerate() {
        row[0] = i;
    }
    d[0] = (0..=m).collect();
    for i in 1..=n {
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (d[i - 1][j] + 1).min(d[i][j - 1] + 1).min(d[i - 1][j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(d[i - 2][j - 2] + 1);
            }
            d[i][j] = v;
        }
    }
    d[n][m]
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
        assert_eq!(edit_distance("rnu", "run"), 1);
    }

    #[test]
    fn suggests_close_words_only() {
        let keys = ["sprite", "block", "tile", "solid"];
        assert_eq!(closest("sprit", keys), Some("sprite"));
        assert_eq!(closest("blok", keys), Some("block"));
        assert_eq!(closest("zzzzzz", keys), None);
    }
}
