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
///
/// A candidate's score is its [`edit_distance`] to `word`, or `limit` when
/// one contains the other and the distance is more. The first candidate
/// with the lowest score at or under `limit` is the answer. Each candidate
/// is compared only as far as it could still be the answer, so a scene with
/// many names can afford a hint for every unknown one (ADR 0028).
pub fn closest<'a, I>(word: &str, candidates: I) -> Option<&'a str>
where
    I: IntoIterator<Item = &'a str>,
{
    let limit = (word.chars().count() / 3).max(1) + usize::from(word.len() > 5);
    let w: Vec<char> = word.to_lowercase().chars().collect();
    let mut chars: Vec<char> = Vec::new();
    let mut rows: [Vec<usize>; 3] = Default::default();
    let mut best: Option<(usize, &str)> = None;
    for c in candidates {
        // A later candidate wins only with a lower score.
        let cap = match best {
            None => limit,
            Some((0, _)) => break,
            Some((score, _)) => score - 1,
        };
        // A word inside the other scores `limit`: that matters only when
        // `limit` could win.
        let contains = limit <= cap && (c.contains(word) || word.contains(c));
        chars.clear();
        if c.is_ascii() {
            chars.extend(c.bytes().map(|b| b.to_ascii_lowercase() as char));
        } else {
            chars.extend(c.to_lowercase().chars());
        }
        let score = match distance_within(&w, &chars, cap, &mut rows) {
            Some(d) => d,
            // Over `cap`, so over `limit`.
            None if contains => limit,
            None => continue,
        };
        best = Some((score, c));
    }
    best.map(|(_, c)| c)
}

/// [`edit_distance`] of two lowercased words when it is at most `cap`.
/// `rows` is scratch space.
fn distance_within(a: &[char], b: &[char], cap: usize, rows: &mut [Vec<usize>; 3]) -> Option<usize> {
    let (n, m) = (a.len(), b.len());
    // Each edit changes the length by at most one.
    if n.abs_diff(m) > cap {
        return None;
    }
    if cap == 0 {
        return (a == b).then_some(0);
    }
    let [before, last, row] = rows;
    for r in [&mut *before, &mut *last, &mut *row] {
        r.clear();
        r.resize(m + 1, 0);
    }
    last.iter_mut().enumerate().for_each(|(j, v)| *v = j);
    // The least value of `last` (row 0 is 0..=m).
    let mut least_last = 0;
    for i in 1..=n {
        row[0] = i;
        let mut least = i;
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            let mut v = (last[j] + 1).min(row[j - 1] + 1).min(last[j - 1] + cost);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                v = v.min(before[j - 2] + 1);
            }
            row[j] = v;
            least = least.min(v);
        }
        // Every value of a row is at least the least of the row above it, or
        // of the row above that plus one: when both are over `cap`, every
        // later row is too.
        if least > cap && least_last > cap {
            return None;
        }
        std::mem::swap(before, last);
        std::mem::swap(last, row);
        least_last = least;
    }
    let d = last[m];
    (d <= cap).then_some(d)
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

    /// `closest` as it was before ADR 0028: every candidate compared in full.
    fn closest_reference<'a>(word: &str, candidates: &[&'a str]) -> Option<&'a str> {
        let limit = (word.chars().count() / 3).max(1) + usize::from(word.len() > 5);
        let mut best: Option<(usize, &str)> = None;
        for &c in candidates {
            let d = edit_distance(word, c);
            let contains = c.contains(word) || word.contains(c);
            let score = if contains && d > limit { limit } else { d };
            if score <= limit && best.is_none_or(|(bd, _)| score < bd) {
                best = Some((score, c));
            }
        }
        best.map(|(_, c)| c)
    }

    #[test]
    fn closest_gives_what_comparing_every_candidate_in_full_gives() {
        // Few letters, so that near words, ties, words inside words and
        // cases are common; some letters that lowercase to other lengths.
        const LETTERS: &[&str] = &["a", "b", "A", "_", "ab", "ba", "\u{e9}", "\u{c9}", "\u{3a3}", "x"];
        let mut rng = crate::rng::Rng::new(2028);
        let word = |rng: &mut crate::rng::Rng| -> String {
            let len = rng.below(9);
            (0..len).map(|_| LETTERS[rng.below(LETTERS.len() as u32) as usize]).collect()
        };
        for _ in 0..20_000 {
            let w = word(&mut rng);
            let n = rng.below(12);
            let owned: Vec<String> = (0..n).map(|_| word(&mut rng)).collect();
            let candidates: Vec<&str> = owned.iter().map(String::as_str).collect();
            assert_eq!(
                closest(&w, candidates.iter().copied()),
                closest_reference(&w, &candidates),
                "{w:?} among {candidates:?}"
            );
        }
    }

    #[test]
    fn suggests_close_words_only() {
        let keys = ["sprite", "block", "tile", "solid"];
        assert_eq!(closest("sprit", keys), Some("sprite"));
        assert_eq!(closest("blok", keys), Some("block"));
        assert_eq!(closest("zzzzzz", keys), None);
    }
}
