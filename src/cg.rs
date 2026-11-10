//! Column generation for square-cut jobs (Gilmore-Gomory): the LP relaxation of "which cutting
//! patterns, how many times" is solved with a small revised simplex; new patterns come from a
//! bounded knapsack priced with the duals. The LP solution is rounded down, the rest is solved
//! again, and the engine's own heuristic finishes the last few pieces.

/// A cutting pattern: stock index, pieces of each type, cost of one bar.
#[derive(Clone, Debug)]
pub struct Pattern {
    pub stock: usize,
    pub counts: Vec<usize>,
    pub cost: f64,
}

/// One stock length that may be used.
pub struct StockOpt {
    pub stock: usize,
    /// capacity in integer units (bar length + kerf: each piece takes its length + kerf)
    pub cap: i64,
    pub cost: f64,
}

const EPS: f64 = 1e-9;

fn gcd(a: i64, b: i64) -> i64 {
    if b == 0 {
        a.abs()
    } else {
        gcd(b, a % b)
    }
}

/// Bounded knapsack: the most valuable piece counts for one bar (weights in units).
fn knapsack(weights: &[i64], values: &[f64], bounds: &[usize], cap: i64) -> Option<(Vec<usize>, f64)> {
    let g = weights.iter().fold(0i64, |a, &w| gcd(a, w)).max(1);
    let c = (cap / g) as usize;
    if c > 60_000 {
        return None;
    }
    // binary splitting into 0/1 items
    let mut items: Vec<(usize, usize, usize)> = vec![]; // (type, multiplicity, weight in cells)
    for (i, (&w, &b)) in weights.iter().zip(bounds).enumerate() {
        if values[i] <= EPS || b == 0 {
            continue;
        }
        let wc = (w / g) as usize;
        if wc == 0 || wc > c {
            continue;
        }
        let mut left = b.min(c / wc);
        let mut k = 1;
        while left > 0 {
            let take = k.min(left);
            items.push((i, take, wc * take));
            left -= take;
            k *= 2;
        }
    }
    let mut best = vec![0.0f64; c + 1];
    let mut took = vec![false; items.len() * (c + 1)];
    for (n, &(i, m, w)) in items.iter().enumerate() {
        let v = values[i] * m as f64;
        for cap_i in (w..=c).rev() {
            let cand = best[cap_i - w] + v;
            if cand > best[cap_i] + EPS {
                best[cap_i] = cand;
                took[n * (c + 1) + cap_i] = true;
            }
        }
    }
    let mut counts = vec![0usize; weights.len()];
    let mut cap_i = c;
    for n in (0..items.len()).rev() {
        if took[n * (c + 1) + cap_i] {
            let (i, m, w) = items[n];
            counts[i] += m;
            cap_i -= w;
        }
    }
    Some((counts, best[c]))
}

/// Solves the LP for `demand` (pieces of each type; `weights` include the kerf). Returns the
/// patterns with their (fractional) counts, or None when the job does not suit the method.
pub fn solve_lp(weights: &[i64], demand: &[usize], stocks: &[StockOpt], cancel: &dyn Fn() -> bool) -> Option<Vec<(Pattern, f64)>> {
    let m = weights.len();
    if m == 0 || stocks.is_empty() {
        return None;
    }
    let d: Vec<f64> = demand.iter().map(|&x| x as f64).collect();
    // columns: patterns, then the surplus of each row (-e_j, cost 0)
    let mut cols: Vec<Pattern> = vec![];
    // starting basis: for every type the cheapest homogeneous pattern
    let mut basis: Vec<usize> = vec![];
    for i in 0..m {
        let mut pick: Option<(usize, usize, f64)> = None;
        for (k, s) in stocks.iter().enumerate() {
            let n = (s.cap / weights[i].max(1)) as usize;
            if n == 0 {
                continue;
            }
            let per = s.cost / n as f64;
            if pick.map(|p| per < p.2).unwrap_or(true) {
                pick = Some((k, n, per));
            }
        }
        let (k, n, _) = pick?;
        let mut counts = vec![0; m];
        counts[i] = n;
        cols.push(Pattern { stock: stocks[k].stock, counts, cost: stocks[k].cost });
        basis.push(cols.len() - 1);
    }
    let surplus = |j: usize| usize::MAX - j;
    let col_vec = |cols: &Vec<Pattern>, c: usize| -> Vec<f64> {
        if c >= usize::MAX - m {
            let j = usize::MAX - c;
            let mut v = vec![0.0; m];
            v[j] = -1.0;
            v
        } else {
            cols[c].counts.iter().map(|&x| x as f64).collect()
        }
    };
    let col_cost = |cols: &Vec<Pattern>, c: usize| if c >= usize::MAX - m { 0.0 } else { cols[c].cost };
    let stock_of = |st: usize| stocks.iter().position(|s| s.stock == st).unwrap();
    for _iter in 0..2000 {
        if cancel() {
            return None;
        }
        // basis inverse (Gauss-Jordan)
        let mut a = vec![vec![0.0f64; 2 * m]; m];
        for (k, &c) in basis.iter().enumerate() {
            let v = col_vec(&cols, c);
            for r in 0..m {
                a[r][k] = v[r];
            }
        }
        for r in 0..m {
            a[r][m + r] = 1.0;
        }
        for k in 0..m {
            let p = (k..m).max_by(|&x, &y| a[x][k].abs().partial_cmp(&a[y][k].abs()).unwrap())?;
            if a[p][k].abs() < 1e-12 {
                return None;
            }
            a.swap(k, p);
            let piv = a[k][k];
            for x in a[k].iter_mut() {
                *x /= piv;
            }
            for r in 0..m {
                if r != k && a[r][k].abs() > 0.0 {
                    let f = a[r][k];
                    for cc in 0..2 * m {
                        a[r][cc] -= f * a[k][cc];
                    }
                }
            }
        }
        let binv: Vec<Vec<f64>> = a.iter().map(|row| row[m..].to_vec()).collect();
        let xb: Vec<f64> = (0..m).map(|r| (0..m).map(|c| binv[r][c] * d[c]).sum()).collect();
        let cb: Vec<f64> = basis.iter().map(|&c| col_cost(&cols, c)).collect();
        let pi: Vec<f64> = (0..m).map(|c| (0..m).map(|r| cb[r] * binv[r][c]).sum()).collect();
        // entering column: a surplus with negative dual, else the best new pattern
        let mut enter: Option<(usize, f64)> = None;
        for j in 0..m {
            if pi[j] < -1e-9 && !basis.contains(&surplus(j)) && enter.map(|e| pi[j] < e.1).unwrap_or(true) {
                enter = Some((surplus(j), pi[j]));
            }
        }
        if enter.is_none() {
            let mut best: Option<(Pattern, f64)> = None;
            for s in stocks {
                let bounds: Vec<usize> = demand.to_vec();
                let Some((counts, v)) = knapsack(weights, &pi, &bounds, s.cap) else { return None };
                let rc = s.cost - v;
                if rc < -1e-7 * s.cost.max(1.0) && best.as_ref().map(|b| rc / s.cost < b.1).unwrap_or(true) {
                    best = Some((Pattern { stock: s.stock, counts, cost: s.cost }, rc / s.cost));
                }
            }
            match best {
                Some((p, rc)) => match cols.iter().position(|c| c.stock == p.stock && c.counts == p.counts) {
                    // a known pattern that is not in the basis enters again
                    Some(k) if !basis.contains(&k) => enter = Some((k, rc)),
                    Some(_) => break, // already basic: rounding noise
                    None => {
                        cols.push(p);
                        enter = Some((cols.len() - 1, rc));
                    }
                },
                None => break,
            }
        }
        let (q, _) = enter.unwrap();
        let aq = col_vec(&cols, q);
        let u: Vec<f64> = (0..m).map(|r| (0..m).map(|c| binv[r][c] * aq[c]).sum()).collect();
        let mut leave: Option<(usize, f64)> = None;
        for r in 0..m {
            if u[r] > 1e-9 {
                let t = xb[r] / u[r];
                if leave.map(|l| t < l.1 - 1e-12).unwrap_or(true) {
                    leave = Some((r, t));
                }
            }
        }
        let Some((r, _)) = leave else { return None };
        basis[r] = q;
        let _ = stock_of;
    }
    // final primal values
    let mut a = vec![vec![0.0f64; m + 1]; m];
    for (k, &c) in basis.iter().enumerate() {
        let v = col_vec(&cols, c);
        for r in 0..m {
            a[r][k] = v[r];
        }
    }
    for r in 0..m {
        a[r][m] = d[r];
    }
    for k in 0..m {
        let p = (k..m).max_by(|&x, &y| a[x][k].abs().partial_cmp(&a[y][k].abs()).unwrap())?;
        if a[p][k].abs() < 1e-12 {
            return None;
        }
        a.swap(k, p);
        let piv = a[k][k];
        for x in a[k].iter_mut() {
            *x /= piv;
        }
        for r in 0..m {
            if r != k {
                let f = a[r][k];
                if f != 0.0 {
                    for cc in 0..=m {
                        a[r][cc] -= f * a[k][cc];
                    }
                }
            }
        }
    }
    let mut out = vec![];
    for (k, &c) in basis.iter().enumerate() {
        if c < usize::MAX - m && a[k][m] > 1e-9 {
            out.push((cols[c].clone(), a[k][m]));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knapsack_fills_the_bar() {
        // 12 m bar, pieces 5.5 and 6.5 (units of 0.05 m)
        let (counts, v) = knapsack(&[110, 130], &[110.0, 130.0], &[5, 5], 240).unwrap();
        assert_eq!(counts, vec![1, 1]);
        assert_eq!(v, 240.0);
    }

    #[test]
    fn lp_reaches_the_bound_for_a_simple_job() {
        // 100 pieces of 4 m and 100 of 8 m in 12 m bars: 100 bars (4+8)
        let r = solve_lp(&[4, 8], &[100, 100], &[StockOpt { stock: 0, cap: 12, cost: 12.0 }], &|| false).unwrap();
        let bars: f64 = r.iter().map(|(_, x)| x).sum();
        assert!((bars - 100.0).abs() < 1e-6, "{bars}");
    }
}
