#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    Insert { line: usize, text: String },
    Delete { line: usize },
}

/// Myers O(ND) - lightweight, no extra deps.
///
/// Invariant: `line` is always an OLD-coordinate index (position in `old`
/// at which to insert/delete). Deltas are emitted in forward order.
/// `apply_deltas` consumes them largest-line-first, so old positions stay
/// valid while applying.
pub fn myers_diff(old: &[String], new: &[String]) -> Vec<Delta> {
    let n = old.len() as i32;
    let m = new.len() as i32;
    if n == 0 && m == 0 {
        return Vec::new();
    }
    if n == 0 {
        // old-coordinate for empty old is 0 for every insert; forward emission
        // order + reverse application in apply_deltas restores new order.
        return new
            .iter()
            .map(|text| Delta::Insert { line: 0, text: text.clone() })
            .collect();
    }
    if m == 0 {
        return (0..n).map(|i| Delta::Delete { line: i as usize }).collect();
    }
    let max = (n + m) as usize;
    let offset = max as i32;
    let size = 2 * max + 1;
    let mut v = vec![0i32; size];
    let mut trace: Vec<Vec<i32>> = Vec::new();
    let mut d_found = 0i32;
    'outer: for d in 0..=max as i32 {
        for k in (-d..=d).step_by(2) {
            let k_offset = (k + offset) as usize;
            let mut x = if k == -d || (k != d && v[(k - 1 + offset) as usize] < v[(k + 1 + offset) as usize]) {
                v[(k + 1 + offset) as usize]
            } else {
                v[(k - 1 + offset) as usize] + 1
            };
            let mut y = x - k;
            while x < n && y < m && old[x as usize] == new[y as usize] {
                x += 1; y += 1;
            }
            v[k_offset] = x;
            if x >= n && y >= m {
                trace.push(v.clone());
                d_found = d;
                break 'outer;
            }
        }
        trace.push(v.clone());
    }
    let mut x = n;
    let mut y = m;
    let mut deltas_rev: Vec<Delta> = Vec::new();
    for d in (0..=d_found).rev() {
        let k = x - y;
        if d == 0 {
            while x > 0 && y > 0 && old[(x - 1) as usize] == new[(y - 1) as usize] { x -= 1; y -= 1; }
            break;
        }
        let v_prev = &trace[(d - 1) as usize];
        let prev_k = if k == -d || (k != d && v_prev[(k - 1 + offset) as usize] < v_prev[(k + 1 + offset) as usize]) { k + 1 } else { k - 1 };
        let prev_x = v_prev[(prev_k + offset) as usize];
        let prev_y = prev_x - prev_k;
        while x > prev_x && y > prev_y { x -= 1; y -= 1; }
        if x == prev_x {
            deltas_rev.push(Delta::Insert { line: prev_x as usize, text: new[prev_y as usize].clone() });
        } else {
            deltas_rev.push(Delta::Delete { line: prev_x as usize });
        }
        x = prev_x; y = prev_y;
    }
    deltas_rev.reverse();
    deltas_rev
}

#[allow(dead_code)]
pub fn apply_deltas(old: &[String], deltas: &[Delta]) -> Vec<String> {
    let mut res = old.to_vec();
    // Apply largest line first so earlier (old-coordinate) positions stay valid.
    // Same-line ties: deletes before inserts; inserts in reverse emission order
    // (reverse first, then stable sort preserves that for equal keys).
    let mut sorted = deltas.to_vec();
    sorted.reverse();
    sorted.sort_by(|a, b| {
        let la = match a { Delta::Insert { line, .. } => *line, Delta::Delete { line } => *line };
        let lb = match b { Delta::Insert { line, .. } => *line, Delta::Delete { line } => *line };
        lb.cmp(&la).then_with(|| match (a, b) {
            (Delta::Delete { .. }, Delta::Insert { .. }) => std::cmp::Ordering::Less,
            (Delta::Insert { .. }, Delta::Delete { .. }) => std::cmp::Ordering::Greater,
            _ => std::cmp::Ordering::Equal,
        })
    });
    for d in sorted {
        match d {
            Delta::Insert { line, text } => { if line <= res.len() { res.insert(line, text); } else { res.push(text); } }
            Delta::Delete { line } => { if line < res.len() { res.remove(line); } }
        }
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;
    fn s(v: &[&str]) -> Vec<String> { v.iter().map(|x| x.to_string()).collect() }
    #[test] fn identical() { let a=s(&["a","b","c"]); let b=s(&["a","b","c"]); assert_eq!(myers_diff(&a,&b), vec![]); assert_eq!(apply_deltas(&a,&myers_diff(&a,&b)), b); }
    #[test] fn single_insert() { let a=s(&["a","b"]); let b=s(&["a","c","b"]); let d=myers_diff(&a,&b); assert_eq!(d.len(),1); assert!(matches!(d[0], Delta::Insert{line:1,..})); assert_eq!(apply_deltas(&a,&d), b); }
    #[test] fn single_delete() { let a=s(&["a","b"]); let b=s(&["a"]); let d=myers_diff(&a,&b); assert_eq!(d, vec![Delta::Delete{line:1}]); assert_eq!(apply_deltas(&a,&d), b); }
    #[test] fn empty_old() { let a:Vec<String>=vec![]; let b=s(&["x","y"]); let d=myers_diff(&a,&b); assert_eq!(d.len(),2); assert_eq!(apply_deltas(&a,&d), b); }
    #[test] fn empty_new() { let a=s(&["x","y"]); let b:Vec<String>=vec![]; let d=myers_diff(&a,&b); assert_eq!(d.len(),2); assert_eq!(apply_deltas(&a,&d), b); }
    #[test] fn duplicate() { let a=s(&["a","a","b"]); let b=s(&["a","b","a"]); let d=myers_diff(&a,&b); assert_eq!(apply_deltas(&a,&d), b); }
    #[test] fn colons() { let a=s(&["a::b","c"]); let b=s(&["a::b","x::y::z","c"]); let d=myers_diff(&a,&b); assert_eq!(apply_deltas(&a,&d), b); }

    /// Tiny deterministic PRNG (xorshift64*) so property tests need no new deps.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545F4914F6CDD1D)
        }
        fn below(&mut self, n: usize) -> usize {
            if n == 0 { return 0; }
            (self.next() % n as u64) as usize
        }
    }

    /// Independent O(n*m) LCS oracle to prove Myers minimality: D == n + m - 2*LCS.
    fn lcs_len(a: &[String], b: &[String]) -> usize {
        let (n, m) = (a.len(), b.len());
        let mut prev = vec![0usize; m + 1];
        let mut cur = vec![0usize; m + 1];
        for i in 1..=n {
            for j in 1..=m {
                cur[j] = if a[i - 1] == b[j - 1] { prev[j - 1] + 1 } else { prev[j].max(cur[j - 1]) };
            }
            std::mem::swap(&mut prev, &mut cur);
        }
        prev[m]
    }

    fn rand_lines(rng: &mut Rng, words: &[&str], max_len: usize) -> Vec<String> {
        let n = rng.below(max_len + 1);
        (0..n).map(|_| words[rng.below(words.len())].to_string()).collect()
    }

    fn mutate(rng: &mut Rng, base: &[String], words: &[&str], edits: usize) -> Vec<String> {
        let mut v = base.to_vec();
        for _ in 0..edits {
            match rng.below(3) {
                0 => {
                    // insert
                    let pos = rng.below(v.len() + 1);
                    v.insert(pos, words[rng.below(words.len())].to_string());
                }
                1 => {
                    // delete
                    if !v.is_empty() {
                        v.remove(rng.below(v.len()));
                    }
                }
                _ => {
                    // replace
                    if !v.is_empty() {
                        let idx = rng.below(v.len());
                        let w = words[rng.below(words.len())].to_string();
                        v[idx] = w;
                    }
                }
            }
        }
        v
    }

    #[test]
    fn prop_roundtrip_and_minimal() {
        // small alphabet forces duplicates, repeats, adversarial overlaps
        let words = ["a", "b", "c", "a::b", ""];
        let mut rng = Rng(0x1234_5678_9abc_def1);
        for _ in 0..2000 {
            let a = rand_lines(&mut rng, &words, 25);
            // half the cases: mutate a (small realistic diffs), half: independent pair
            let pick = rng.below(2);
            let b = if pick == 0 {
                let edits = rng.below(6);
                mutate(&mut rng, &a, &words, edits)
            } else {
                rand_lines(&mut rng, &words, 25)
            };
            let d = myers_diff(&a, &b);
            assert_eq!(apply_deltas(&a, &d), b, "roundtrip failed for {a:?} -> {b:?}");
            let expect = a.len() + b.len() - 2 * lcs_len(&a, &b);
            assert_eq!(d.len(), expect, "not minimal for {a:?} -> {b:?}");
        }
    }

    #[test]
    fn prop_large_prefix_stays_fast() {
        // 2000 shared lines + small tail edit: must stay quick and exact
        let mut a: Vec<String> = (0..2000).map(|i| format!("line {i}")).collect();
        let mut b = a.clone();
        b.insert(1500, "inserted".to_string());
        b.remove(10);
        b.push("tail".to_string());
        let d = myers_diff(&a, &b);
        assert_eq!(apply_deltas(&a, &d), b);
        assert_eq!(d.len(), a.len() + b.len() - 2 * lcs_len(&a, &b));
        let _ = std::mem::replace(&mut a, vec![]);
    }
}
