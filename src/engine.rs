//! Linear (1D) cutting optimizer.
//!
//! Design: lengths in integer units (1 / precision), pieces grouped by material, bars filled
//! one at a time by an evolutionary search over piece orders (population = optimization
//! level, mutation = n/10 + 1 random swaps, stop after 10 generations without improvement,
//! xoroshiro64** seeded with seed+1 / seed+2), and a greedy decoder for the cut geometry: no
//! kerf before the first piece, kerf T on square joins, T / sin(a) and an overlap of
//! H / tan(max angle) where angled ends nest, and `space_between_cuts` wherever two cut faces
//! do not match.

use crate::model::*;
use std::time::Instant;

const EPS: f64 = 1e-6;

/// xoroshiro64** pseudo-random generator
#[derive(Clone)]
pub struct Rng {
    s0: u32,
    s1: u32,
}

impl Rng {
    pub fn new(seed: i32) -> Rng {
        Rng { s0: (seed as u32).wrapping_add(1), s1: (seed as u32).wrapping_add(2) }
    }
    pub fn next(&mut self) -> u32 {
        let s0 = self.s0;
        let mut s1 = self.s1;
        let r = s0.wrapping_mul(0x9E37_79BB).rotate_left(5).wrapping_mul(5);
        s1 ^= s0;
        self.s0 = s0.rotate_left(26) ^ s1 ^ (s1 << 9);
        self.s1 = s1.rotate_left(13);
        r
    }
    pub fn range(&mut self, lo: u32, hi: u32) -> u32 {
        if hi <= lo {
            return lo;
        }
        lo + self.next() % (hi - lo)
    }
    pub fn unit(&mut self) -> f64 {
        self.next() as f64 * (1.0 / 4_294_967_296.0)
    }
}

#[derive(Clone, Debug)]
struct Inst {
    part: usize,
    len: i64,
    h: i64,
    a1: f64,
    a2: f64,
}

#[derive(Clone, Copy, Debug)]
struct Geo {
    t: i64,
    space: i64,
    max_pieces: i32,
    oz: bool,
}

fn fold(a: f64) -> f64 {
    if a > 90.0 {
        180.0 - a
    } else {
        a
    }
}

fn rad(a: f64) -> f64 {
    a * std::f64::consts::PI / 180.0
}

fn is90(a: f64) -> bool {
    (a - 90.0).abs() <= EPS
}

/// Angles of an instance after the decoder's orientation flag (swap ends = OZ rotation).
fn ends(p: &Inst, flip: bool) -> (f64, f64) {
    if flip {
        (p.a2, p.a1)
    } else {
        (p.a1, p.a2)
    }
}

/// Join cost between the previous piece (ending at angle `prev`) and the next (starting at `cur`):
/// (kerf along the bar, overlap of the envelopes, extra spacing).
fn join(prev: f64, cur: f64, h: i64, g: &Geo) -> (f64, f64, f64) {
    let spacing = if (prev + cur - 180.0).abs() <= EPS { 0.0 } else { g.space as f64 };
    let nested = (prev > 90.0 + EPS && cur < 90.0 - EPS) || (prev < 90.0 - EPS && cur > 90.0 + EPS);
    if nested {
        let m = fold(prev).max(fold(cur));
        let overlap = if is90(m) { 0.0 } else { h as f64 / rad(m).tan() };
        let kerf = g.t as f64 / rad(fold(prev)).sin();
        (kerf, overlap, spacing)
    } else {
        (g.t as f64, 0.0, spacing)
    }
}

#[derive(Clone, Debug, Default)]
struct Fill {
    order: Vec<usize>,
    flips: Vec<bool>,
    placed: i64,
    used: f64,
}

impl Fill {
    fn better(&self, o: &Fill) -> bool {
        self.placed > o.placed || (self.placed == o.placed && self.used + EPS < o.used)
    }
}

/// Greedy decoder: walk the order, keep every piece that still fits.
fn decode(pcs: &[Inst], order: &[usize], flip: bool, bar: i64, g: &Geo) -> Fill {
    let mut f = Fill::default();
    let mut prev_a2 = 90.0;
    let mut min_left = i64::MAX;
    for p in pcs {
        min_left = min_left.min(p.len);
    }
    for &i in order {
        let p = &pcs[i];
        let fl = flip && g.oz;
        let (a1, a2) = ends(p, fl);
        if f.order.is_empty() {
            if p.len > bar {
                continue;
            }
            f.used = p.len as f64;
        } else {
            if g.max_pieces > 0 && f.order.len() as i32 >= g.max_pieces {
                break;
            }
            let (k, o, s) = join(prev_a2, a1, p.h, g);
            let nu = f.used + k + p.len as f64 - o + s;
            if nu > bar as f64 + EPS {
                if bar as f64 - f.used < (min_left as f64) {
                    break;
                }
                continue;
            }
            f.used = nu;
        }
        f.placed += p.len;
        f.order.push(i);
        f.flips.push(fl);
        prev_a2 = a2;
    }
    f
}

struct Indiv {
    perm: Vec<usize>,
    flip: bool,
    fill: Fill,
}

/// Evolutionary search for the best fill of one bar of length `bar`.
fn best_fill(pcs: &[Inst], bar: i64, g: &Geo, pop: usize, rng: &mut Rng, cancel: &dyn Fn() -> bool) -> Fill {
    let n = pcs.len();
    if n == 0 {
        return Fill::default();
    }
    let identity: Vec<usize> = (0..n).collect();
    let mut people: Vec<Indiv> = Vec::with_capacity(pop);
    let first = decode(pcs, &identity, false, bar, g);
    people.push(Indiv { perm: identity.clone(), flip: false, fill: first });
    for _ in 1..pop {
        let mut perm = identity.clone();
        for _ in 0..n / 2 {
            let a = rng.range(0, n as u32) as usize;
            let b = rng.range(0, n as u32) as usize;
            perm.swap(a, b);
        }
        let fill = decode(pcs, &perm, false, bar, g);
        people.push(Indiv { perm, flip: false, fill });
    }
    let mut best = people[0].fill.clone();
    for p in &people {
        if p.fill.better(&best) {
            best = p.fill.clone();
        }
    }
    let full = |f: &Fill| f.placed >= bar;
    let mut stagnant = 0;
    while stagnant < 10 && !full(&best) && n > 1 && !cancel() {
        let mut improved = false;
        for ind in people.iter_mut() {
            let mut perm = ind.perm.clone();
            for _ in 0..(n / 10 + 1) {
                let a = rng.range(0, n as u32) as usize;
                let b = rng.range(0, n as u32) as usize;
                perm.swap(a, b);
            }
            let flip = if rng.unit() <= 0.1 { !ind.flip } else { ind.flip };
            let fill = decode(pcs, &perm, flip, bar, g);
            if fill.better(&ind.fill) {
                if fill.better(&best) {
                    best = fill.clone();
                    improved = true;
                }
                ind.perm = perm;
                ind.flip = flip;
                ind.fill = fill;
            }
        }
        stagnant = if improved { 0 } else { stagnant + 1 };
    }
    best
}

pub struct Progress<'a> {
    pub cancel: &'a dyn Fn() -> bool,
    pub report: &'a dyn Fn(usize, usize),
}

/// Result codes: 0 ok, 9 settings cannot be satisfied, 12 material missing.
pub fn optimize(prj: &Project, p: &Progress) -> Solution {
    let start = Instant::now();
    let lin = &prj.technical.linear;
    let precision = if lin.precision > 0.0 { lin.precision } else { 0.001 };
    let scale = 1.0 / precision;
    let iu = |v: f64| (v * scale + 0.5).floor() as i64;
    let g = Geo {
        t: iu(lin.cutter_thickness),
        space: iu(lin.space_between_cuts),
        max_pieces: lin.max_pieces_per_bar,
        oz: !lin.use_global_rotation || lin.oz,
    };
    let pop = lin.optimization_level.clamp(1, 200) as usize;
    let mut rng = Rng::new(lin.random_seed);
    let mut sol = Solution { seed: lin.random_seed, ..Default::default() };
    let mut materials: Vec<String> = vec![];
    for part in prj.parts.iter().filter(|p| p.use_it && p.quantity > 0) {
        if !materials.contains(&part.material) {
            materials.push(part.material.clone());
        }
    }
    let total: usize = prj.parts.iter().filter(|p| p.use_it).map(|p| p.quantity.max(0) as usize).sum();
    let mut done = 0usize;
    let mut stock_left: Vec<i32> = prj.stock.iter().map(|s| if s.use_it { s.quantity.max(0) } else { 0 }).collect();
    let by_price = lin.optimization_type == 1 && prj.stock.iter().any(|s| s.price > 0.0);
    for mat in &materials {
        let mut pcs: Vec<Inst> = vec![];
        for (i, part) in prj.parts.iter().enumerate() {
            if !part.use_it || part.material != *mat {
                continue;
            }
            for _ in 0..part.quantity.max(0) {
                pcs.push(Inst { part: i, len: iu(part.length), h: iu(part.height), a1: part.a1, a2: part.a2 });
            }
        }
        // longest first: the identity individual is a first-fit-decreasing order
        pcs.sort_by(|a, b| b.len.cmp(&a.len).then(a.part.cmp(&b.part)));
        let stocks: Vec<usize> = (0..prj.stock.len()).filter(|&s| prj.stock[s].use_it && prj.stock[s].material == *mat).collect();
        if stocks.is_empty() {
            sol.code = 12;
        }
        let ctx = Ctx { prj, stocks: &stocks, g, pop, by_price, iu: &iu };
        // cutting patterns from column generation (square cuts), the rest by the heuristic;
        // when it reaches the LP bound (rounded up to whole bars) nothing can do better
        let mut best: Option<Plan> = None;
        let mut proven = false;
        if pcs.iter().all(|x| is90(x.a1) && is90(x.a2)) && !(p.cancel)() {
            if let Some((plan, bound)) = ctx.cg_plan(&pcs, &stock_left, &mut rng, p) {
                let used = if by_price { plan.cost } else { plan.stock_len };
                let unit = stocks.iter().map(|&s| if by_price { bar_price(prj, s) } else { prj.stock[s].length }).fold(f64::MAX, f64::min);
                proven = plan.unplaced == 0 && unit > 0.0 && used <= (bound / unit - 1e-6).ceil() * unit + 1e-6 && stocks.iter().all(|&s| prj.stock[s].length == prj.stock[stocks[0]].length);
                best = Some(plan);
            }
        }
        // several complete plans; the cheapest one wins (sequential heuristic)
        for strategy in [Strategy::Utilisation, Strategy::Lookahead, Strategy::LongestStock, Strategy::ShortestStock] {
            if (p.cancel)() || proven {
                break;
            }
            if strategy == Strategy::Lookahead && pcs.len() > 3000 {
                continue;
            }
            let plan = ctx.plan(strategy, pcs.clone(), stock_left.clone(), &mut rng, p);
            if best.as_ref().map(|b| plan.better(b)).unwrap_or(true) {
                best = Some(plan);
            }
        }
        if pcs.iter().all(|x| is90(x.a1) && is90(x.a2)) && !(p.cancel)() && !proven {
            let budget = (0.02 * pop as f64).clamp(0.2, 4.0);
            if let Some(plan) = ctx.svc(&pcs, &stock_left, &mut rng, p, pop * 2 + 20, budget) {
                if best.as_ref().map(|b| plan.better(b)).unwrap_or(true) {
                    best = Some(plan);
                }
            }
        }
        let Some(plan) = best else { continue };
        let mut left_pcs = pcs.clone();
        let mut bars: Vec<Bar> = vec![];
        for (s, fill) in &plan.bars {
            let bar = build_bar(prj, *s, &left_pcs, fill, scale, &g);
            stock_left[*s] -= 1;
            let mut taken = fill.order.clone();
            taken.sort_unstable_by(|a, b| b.cmp(a));
            for i in taken {
                left_pcs.remove(i);
            }
            done += fill.order.len();
            // identical patterns become one row with a quantity
            match bars.iter_mut().find(|b| b.stock == bar.stock && same_pattern(b, &bar)) {
                Some(b) => b.quantity += 1,
                None => bars.push(bar),
            }
        }
        (p.report)(done, total);
        sol.bars.extend(bars);
        // whatever is left could not be placed
        let mut left: Vec<(usize, i32)> = vec![];
        for p in &left_pcs {
            match left.iter_mut().find(|(i, _)| *i == p.part) {
                Some(e) => e.1 += 1,
                None => left.push((p.part, 1)),
            }
        }
        if !left.is_empty() && sol.code == 0 {
            sol.code = if stocks.is_empty() { 12 } else { 9 };
        }
        sol.unplaced.extend(left);
    }
    sol.running_time = start.elapsed().as_secs_f64();
    sol
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Strategy {
    /// best fill of every stock length, highest utilisation (or lowest cost per placed length)
    Utilisation,
    /// like Utilisation, but each candidate is scored by a first-fit-decreasing completion
    Lookahead,
    /// longest available stock first
    LongestStock,
    /// shortest stock that still holds the longest remaining piece
    ShortestStock,
}

struct Plan {
    bars: Vec<(usize, Fill)>,
    unplaced: usize,
    stock_len: f64,
    cost: f64,
    by_price: bool,
}

impl Plan {
    fn better(&self, o: &Plan) -> bool {
        if self.unplaced != o.unplaced {
            return self.unplaced < o.unplaced;
        }
        let (a, b) = if self.by_price { (self.cost, o.cost) } else { (self.stock_len, o.stock_len) };
        if (a - b).abs() > 1e-9 {
            return a < b;
        }
        self.bars.len() < o.bars.len()
    }
}

struct Ctx<'a> {
    prj: &'a Project,
    stocks: &'a [usize],
    g: Geo,
    pop: usize,
    by_price: bool,
    iu: &'a dyn Fn(f64) -> i64,
}

impl Ctx<'_> {
    fn bar_len(&self, s: usize) -> i64 {
        (self.iu)(self.prj.stock[s].length)
    }

    /// Candidate stock rows: best priority first, one row per distinct length.
    fn candidates(&self, left: &[i32]) -> Vec<usize> {
        let avail: Vec<usize> = self.stocks.iter().copied().filter(|&s| left[s] > 0).collect();
        let Some(best_pri) = avail.iter().map(|&s| self.prj.stock[s].priority).min() else { return vec![] };
        let mut out: Vec<usize> = vec![];
        for s in avail.into_iter().filter(|&s| self.prj.stock[s].priority == best_pri) {
            if !out.iter().any(|&o| self.bar_len(o) == self.bar_len(s)) {
                out.push(s);
            }
        }
        out
    }

    fn score(&self, s: usize, fill: &Fill) -> f64 {
        let price = bar_price(self.prj, s);
        if self.by_price {
            -(price / fill.placed.max(1) as f64)
        } else {
            fill.placed as f64 / self.bar_len(s) as f64
        }
    }

    /// Cheap completion: first-fit-decreasing bars, utilisation choice. Returns (stock length, cost, unplaced).
    fn rollout(&self, mut pcs: Vec<Inst>, mut left: Vec<i32>) -> (f64, f64, usize) {
        let mut len = 0.0;
        let mut cost = 0.0;
        while !pcs.is_empty() {
            let mut choice: Option<(usize, Fill, f64)> = None;
            for s in self.candidates(&left) {
                let order: Vec<usize> = (0..pcs.len()).collect();
                let fill = decode(&pcs, &order, false, self.bar_len(s), &self.g);
                if fill.order.is_empty() {
                    continue;
                }
                let sc = self.score(s, &fill);
                if choice.as_ref().map(|c| sc > c.2 + 1e-12).unwrap_or(true) {
                    choice = Some((s, fill, sc));
                }
            }
            let Some((s, fill, _)) = choice else { break };
            left[s] -= 1;
            len += self.prj.stock[s].length;
            cost += bar_price(self.prj, s);
            let mut taken = fill.order;
            taken.sort_unstable_by(|a, b| b.cmp(a));
            for i in taken {
                pcs.remove(i);
            }
        }
        (len, cost, pcs.len())
    }

    fn plan(&self, strategy: Strategy, mut pcs: Vec<Inst>, mut left: Vec<i32>, rng: &mut Rng, p: &Progress) -> Plan {
        let mut plan = Plan { bars: vec![], unplaced: 0, stock_len: 0.0, cost: 0.0, by_price: self.by_price };
        while !pcs.is_empty() && !(p.cancel)() {
            let mut cands = self.candidates(&left);
            let longest_piece = pcs.iter().map(|x| x.len).max().unwrap_or(0);
            match strategy {
                Strategy::LongestStock => {
                    cands.sort_by_key(|&s| -self.bar_len(s));
                    cands.truncate(1);
                }
                Strategy::ShortestStock => {
                    cands.retain(|&s| self.bar_len(s) >= longest_piece);
                    cands.sort_by_key(|&s| self.bar_len(s));
                    cands.truncate(1);
                }
                _ => {}
            }
            let mut choice: Option<(usize, Fill, f64)> = None;
            for s in cands {
                let fill = best_fill(&pcs, self.bar_len(s), &self.g, self.pop, rng, p.cancel);
                if fill.order.is_empty() {
                    continue;
                }
                let sc = if strategy == Strategy::Lookahead {
                    let mut rest = pcs.clone();
                    let mut taken = fill.order.clone();
                    taken.sort_unstable_by(|a, b| b.cmp(a));
                    for i in taken {
                        rest.remove(i);
                    }
                    let mut l2 = left.clone();
                    l2[s] -= 1;
                    let (len, cost, un) = self.rollout(rest, l2);
                    let total = if self.by_price { cost + bar_price(self.prj, s) } else { len + self.prj.stock[s].length };
                    -(un as f64 * 1e12 + total) + self.score(s, &fill) * 1e-6
                } else {
                    self.score(s, &fill)
                };
                if choice.as_ref().map(|c| sc > c.2 + 1e-12 || ((sc - c.2).abs() <= 1e-12 && fill.placed > c.1.placed)).unwrap_or(true) {
                    choice = Some((s, fill, sc));
                }
            }
            let Some((s, fill, _)) = choice else { break };
            left[s] -= 1;
            plan.stock_len += self.prj.stock[s].length;
            plan.cost += bar_price(self.prj, s);
            let mut taken = fill.order.clone();
            taken.sort_unstable_by(|a, b| b.cmp(a));
            for i in taken {
                pcs.remove(i);
            }
            plan.bars.push((s, fill));
        }
        plan.unplaced = pcs.len();
        plan
    }
}

// ------------------------------------------------------------------ sequential value correction
//
// For square cuts the order of pieces on a bar does not matter, so a bar is a pattern (counts per
// piece length). Patterns are found with a bounded knapsack over "values" that are corrected after
// every bar (Mukhacheva / Belov): lengths that tend to leave waste become more valuable and are
// packed earlier. Many randomised passes are made and the cheapest complete plan is kept.

struct Knap<'a> {
    size: &'a [i64],
    value: &'a [f64],
    bound: &'a [usize],
    cap: i64,
    max_pieces: usize,
    nodes: usize,
    cur: Vec<usize>,
    best: Vec<usize>,
    best_v: f64,
    best_left: i64,
}

impl Knap<'_> {
    fn run(&mut self, k: usize, left: i64, v: f64, count: usize) {
        self.nodes += 1;
        if v > self.best_v + 1e-9 || ((v - self.best_v).abs() <= 1e-9 && left < self.best_left) {
            self.best_v = v;
            self.best_left = left;
            self.best.clone_from(&self.cur);
        }
        if k == self.size.len() || self.nodes > 40_000 || (self.max_pieces > 0 && count >= self.max_pieces) {
            return;
        }
        // LP bound over the remaining types (sorted by efficiency)
        let mut ub = v;
        let mut cap = left;
        for j in k..self.size.len() {
            if cap <= 0 {
                break;
            }
            let take = (self.bound[j] as i64).min(cap / self.size[j]);
            ub += take as f64 * self.value[j];
            cap -= take * self.size[j];
            if take < self.bound[j] as i64 {
                ub += self.value[j] * cap as f64 / self.size[j] as f64;
                break;
            }
        }
        if ub < self.best_v - 1e-9 || (ub <= self.best_v + 1e-9 && self.best_left == 0) {
            return;
        }
        let mut most = (self.bound[k] as i64).min(left / self.size[k]) as usize;
        if self.max_pieces > 0 {
            most = most.min(self.max_pieces - count);
        }
        for x in (0..=most).rev() {
            self.cur[k] = x;
            self.run(k + 1, left - x as i64 * self.size[k], v + x as f64 * self.value[k], count + x);
        }
        self.cur[k] = 0;
    }
}

/// Distinct piece lengths of the remaining pieces, with the indices of their instances.
fn piece_types(pcs: &[Inst]) -> Vec<(i64, Vec<usize>)> {
    let mut types: Vec<(i64, Vec<usize>)> = vec![];
    for (i, p) in pcs.iter().enumerate() {
        match types.iter_mut().find(|t| t.0 == p.len) {
            Some(t) => t.1.push(i),
            None => types.push((p.len, vec![i])),
        }
    }
    types
}

impl Ctx<'_> {
    /// Best pattern for a bar of length `bar` under `values` (indexed like `types`).
    fn pattern(&self, types: &[(i64, Vec<usize>)], values: &[f64], bar: i64) -> (Vec<usize>, f64) {
        let t = self.g.t;
        let mut idx: Vec<usize> = (0..types.len()).filter(|&j| types[j].0 <= bar).collect();
        idx.sort_by(|&a, &b| {
            let ea = values[a] / (types[a].0 + t) as f64;
            let eb = values[b] / (types[b].0 + t) as f64;
            eb.partial_cmp(&ea).unwrap_or(std::cmp::Ordering::Equal)
        });
        let size: Vec<i64> = idx.iter().map(|&j| types[j].0 + t).collect();
        let value: Vec<f64> = idx.iter().map(|&j| values[j]).collect();
        let bound: Vec<usize> = idx.iter().map(|&j| types[j].1.len()).collect();
        let mut k = Knap {
            size: &size,
            value: &value,
            bound: &bound,
            cap: bar + t,
            max_pieces: self.g.max_pieces.max(0) as usize,
            nodes: 0,
            cur: vec![0; idx.len()],
            best: vec![0; idx.len()],
            best_v: 0.0,
            best_left: bar + t,
        };
        k.run(0, k.cap, 0.0, 0);
        let mut counts = vec![0usize; types.len()];
        for (p, &j) in idx.iter().enumerate() {
            counts[j] = k.best[p];
        }
        (counts, k.best_v)
    }

    fn svc_pass(&self, mut pcs: Vec<Inst>, mut left: Vec<i32>, values: &mut Vec<(i64, f64)>, rng: &mut Rng, power: f64) -> Plan {
        let mut plan = Plan { bars: vec![], unplaced: 0, stock_len: 0.0, cost: 0.0, by_price: self.by_price };
        while !pcs.is_empty() {
            let types = piece_types(&pcs);
            let vals: Vec<f64> = types.iter().map(|t| values.iter().find(|v| v.0 == t.0).map(|v| v.1).unwrap_or(t.0 as f64)).collect();
            let mut choice: Option<(usize, Vec<usize>, f64)> = None;
            for s in self.candidates(&left) {
                let bar = self.bar_len(s);
                let (counts, v) = self.pattern(&types, &vals, bar);
                if v <= 0.0 {
                    continue;
                }
                let denom = if self.by_price { bar_price(self.prj, s).max(1e-9) } else { bar as f64 };
                // a little noise separates the passes
                let sc = v / denom * (1.0 + 0.002 * rng.unit());
                if choice.as_ref().map(|c| sc > c.2).unwrap_or(true) {
                    choice = Some((s, counts, sc));
                }
            }
            let Some((s, counts, _)) = choice else { break };
            let bar = self.bar_len(s);
            let mut order: Vec<usize> = vec![];
            let mut used_len: i64 = 0;
            for (j, &c) in counts.iter().enumerate() {
                order.extend(types[j].1.iter().take(c));
                used_len += types[j].0 * c as i64;
            }
            let n = order.len() as i64;
            let fill = Fill { flips: vec![false; order.len()], placed: used_len, used: (used_len + (n - 1).max(0) * self.g.t) as f64, order };
            // value correction: y = l * (L / used)^p, blended into the running value
            let ratio = bar as f64 / used_len.max(1) as f64;
            for (j, &c) in counts.iter().enumerate() {
                if c == 0 {
                    continue;
                }
                let l = types[j].0;
                let target = l as f64 * ratio.powf(power);
                match values.iter_mut().find(|v| v.0 == l) {
                    Some(v) => v.1 = 0.6 * v.1 + 0.4 * target,
                    None => values.push((l, target)),
                }
            }
            left[s] -= 1;
            plan.stock_len += self.prj.stock[s].length;
            plan.cost += bar_price(self.prj, s);
            let mut taken = fill.order.clone();
            taken.sort_unstable_by(|a, b| b.cmp(a));
            for i in taken {
                pcs.remove(i);
            }
            plan.bars.push((s, fill));
        }
        plan.unplaced = pcs.len();
        plan
    }

    /// Column generation: patterns of the LP relaxation, rounded down; the LP is solved again
    /// for what is left (a few rounds) and the value-correction heuristic places the rest.
    fn cg_plan(&self, pcs: &[Inst], left: &[i32], rng: &mut Rng, p: &Progress) -> Option<(Plan, f64)> {
        if self.g.max_pieces > 0 || self.g.space > 0 {
            return None;
        }
        let t = self.g.t;
        let mut rem: Vec<Inst> = pcs.to_vec();
        let mut left = left.to_vec();
        let mut plan = Plan { bars: vec![], unplaced: 0, stock_len: 0.0, cost: 0.0, by_price: self.by_price };
        let mut lp_bound = 0.0f64;
        for _round in 0..4 {
            let types = piece_types(&rem);
            if types.is_empty() {
                break;
            }
            let weights: Vec<i64> = types.iter().map(|ty| ty.0 + t).collect();
            let demand: Vec<usize> = types.iter().map(|ty| ty.1.len()).collect();
            let stocks: Vec<crate::cg::StockOpt> = self
                .candidates(&left)
                .into_iter()
                .map(|s| crate::cg::StockOpt { stock: s, cap: self.bar_len(s) + t, cost: if self.by_price { bar_price(self.prj, s).max(1e-9) } else { self.prj.stock[s].length } })
                .collect();
            let Some(mut pats) = crate::cg::solve_lp(&weights, &demand, &stocks, p.cancel) else { break };
            if _round == 0 {
                // LP optimum: no plan can use less (stock length or cost)
                lp_bound = pats.iter().map(|(pt, x)| pt.cost * x).sum();
            }
            pats.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            let mut used_any = false;
            for (pat, x) in pats {
                for _ in 0..(x + 1e-7).floor() as usize {
                    if left[pat.stock] <= 0 {
                        break;
                    }
                    let mut order: Vec<usize> = vec![];
                    let mut placed = 0i64;
                    for (j, &c) in pat.counts.iter().enumerate() {
                        let len = types[j].0;
                        let mut k = 0;
                        for (pos, inst) in rem.iter().enumerate() {
                            if k == c {
                                break;
                            }
                            if inst.len == len && !order.contains(&pos) {
                                order.push(pos);
                                k += 1;
                                placed += len;
                            }
                        }
                    }
                    if order.is_empty() {
                        break;
                    }
                    let n = order.len() as i64;
                    let fill = Fill { flips: vec![false; order.len()], placed, used: (placed + (n - 1).max(0) * t) as f64, order };
                    left[pat.stock] -= 1;
                    plan.stock_len += self.prj.stock[pat.stock].length;
                    plan.cost += bar_price(self.prj, pat.stock);
                    let mut taken = fill.order.clone();
                    taken.sort_unstable_by(|a, b| b.cmp(a));
                    for i in taken {
                        rem.remove(i);
                    }
                    plan.bars.push((pat.stock, fill));
                    used_any = true;
                }
            }
            if !used_any {
                break;
            }
        }
        if !rem.is_empty() {
            let tail = self.svc(&rem, &left, rng, p, self.pop + 10, 1.0)?;
            plan.stock_len += tail.stock_len;
            plan.cost += tail.cost;
            plan.unplaced = tail.unplaced;
            plan.bars.extend(tail.bars);
        }
        Some((plan, lp_bound))
    }

    /// Repeated value-correction passes within the time budget.
    fn svc(&self, pcs: &[Inst], left: &[i32], rng: &mut Rng, p: &Progress, passes: usize, budget: f64) -> Option<Plan> {
        let start = Instant::now();
        let mut values: Vec<(i64, f64)> = vec![];
        let mut best: Option<Plan> = None;
        for pass in 0..passes {
            if (p.cancel)() || (pass > 0 && start.elapsed().as_secs_f64() > budget) {
                break;
            }
            let power = if pass == 0 { 1.0 } else { 1.0 + 0.2 * rng.unit() };
            let plan = self.svc_pass(pcs.to_vec(), left.to_vec(), &mut values, rng, power);
            if best.as_ref().map(|b| plan.better(b)).unwrap_or(true) {
                best = Some(plan);
            }
        }
        best
    }
}

fn bar_price(prj: &Project, s: usize) -> f64 {
    let st = &prj.stock[s];
    if prj.technical.price_per_what == 1 {
        st.price * st.length
    } else {
        st.price
    }
}

/// Same lengths in the same order (pieces of different part rows with equal lengths count
/// as one pattern, so the plan has one row per way of cutting a bar).
fn same_pattern(a: &Bar, b: &Bar) -> bool {
    a.pieces.len() == b.pieces.len()
        && a.pieces.iter().zip(&b.pieces).all(|(x, y)| (x.length - y.length).abs() < EPS && (x.a1 - y.a1).abs() < EPS && (x.a2 - y.a2).abs() < EPS)
}

/// Positions of the placed pieces (bottom/top edges) in user units.
fn build_bar(prj: &Project, s: usize, pcs: &[Inst], fill: &Fill, scale: f64, g: &Geo) -> Bar {
    let st = &prj.stock[s];
    let lin = &prj.technical.linear;
    let mut order: Vec<(usize, bool)> = fill.order.iter().copied().zip(fill.flips.iter().copied()).collect();
    if order.iter().all(|&(i, _)| is90(pcs[i].a1) && is90(pcs[i].a2)) {
        // square cuts: the order does not change the material used
        order.sort_by(|a, b| pcs[b.0].len.cmp(&pcs[a.0].len).then(pcs[a.0].part.cmp(&pcs[b.0].part)));
    }
    let mut pieces = vec![];
    let mut x = 0.0f64; // envelope start, integer units
    let mut prev_a2 = 90.0;
    for (k, &(i, flip)) in order.iter().enumerate() {
        let p = &pcs[i];
        let (a1, a2) = ends(p, flip);
        if k > 0 {
            let (kerf, overlap, spacing) = join(prev_a2, a1, p.h, g);
            x += kerf - overlap + spacing;
        }
        let h = p.h as f64;
        let len = p.len as f64;
        let off = |a: f64| if is90(a) || h <= 0.0 { 0.0 } else { h / rad(fold(a)).tan() };
        // a1 < 90: bottom starts at the envelope, top is inset; a1 > 90: the reverse
        let (sb, st_) = if a1 < 90.0 - EPS { (x, x + off(a1)) } else if a1 > 90.0 + EPS { (x + off(a1), x) } else { (x, x) };
        let (eb, et) = if a2 < 90.0 - EPS { (x + len, x + len - off(a2)) } else if a2 > 90.0 + EPS { (x + len - off(a2), x + len) } else { (x + len, x + len) };
        let part = &prj.parts[p.part];
        pieces.push(Placed {
            part: p.part,
            length: part.length,
            a1,
            a2,
            original_a1: part.a1,
            original_a2: part.a2,
            rot: (false, false, flip),
            label: part.label.clone(),
            customer: part.customer.clone(),
            color: part.color,
            id: part.id,
            start_bottom_x: sb / scale,
            end_bottom_x: eb / scale,
            start_top_x: st_ / scale,
            end_top_x: et / scale,
            price: 0.0,
        });
        x += len;
        prev_a2 = a2;
    }
    let bar_len = (st.length * scale + 0.5).floor();
    let kerf_end = if pieces.is_empty() { 0.0 } else if is90(prev_a2) { g.t as f64 } else { g.t as f64 / rad(fold(prev_a2)).sin() };
    let offcut = ((bar_len - fill.used - kerf_end).max(0.0)) / scale;
    let useful = offcut > 0.0 && offcut + EPS >= lin.minimal_useful_size;
    let price = bar_price(prj, s);
    // piece price share by length
    let placed: f64 = pieces.iter().map(|p| p.length).sum();
    for p in pieces.iter_mut() {
        p.price = if placed > 0.0 { price * p.length / placed } else { 0.0 };
    }
    let garbage = (st.length - placed - if useful { offcut } else { 0.0 }).max(0.0);
    Bar {
        stock: s,
        length: st.length,
        height: st.height,
        quantity: 1,
        material: st.material.clone(),
        label: st.label.clone(),
        priority: st.priority,
        id: st.id,
        pieces,
        waste: offcut,
        garbage_waste: garbage,
        bar_price: price,
        trim_left: 0.0,
        trim_right: 0.0,
        waste_angle: 180.0 - prev_a2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rng_matches_xoroshiro64_formula() {
        let mut r = Rng::new(0);
        // first output for state (1, 2)
        assert_eq!(r.next(), 1u32.wrapping_mul(0x9E3779BB).rotate_left(5).wrapping_mul(5));
    }

    #[test]
    fn sample_places_everything() {
        let prj = Project::sample();
        let s = optimize(&prj, &Progress { cancel: &|| false, report: &|_, _| {} });
        assert!(s.unplaced.is_empty());
        let used: f64 = s.bars.iter().map(|b| b.length * b.quantity as f64).sum();
        for b in &s.bars {
            eprintln!("{} x{} waste {} : {:?}", b.length, b.quantity, b.waste, b.pieces.iter().map(|p| p.length).collect::<Vec<_>>());
        }
        let parts: f64 = prj.parts.iter().map(|p| p.length * p.quantity as f64).sum();
        assert!(used >= parts);
        // the example job (37,610 of pieces) fits in less than 4 % offcut
        assert!(used <= parts * 1.04, "used {used} for {parts}");
    }

    /// The rebar job of the comparison with another program (DOICUT_DATA/TESTDATA.csv,
    /// 12 m bars, no kerf): that program needs DB10 905, DB12 1099, DB16 365, DB20 86 bars.
    #[test]
    fn rebar_job_needs_no_more_bars_than_the_reference_result() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../DOICUT_DATA/TESTDATA.csv");
        let Ok(bytes) = std::fs::read(&path) else { return };
        let text = String::from_utf8_lossy(&bytes);
        let mut prj = Project::default();
        for line in text.lines().skip(1) {
            let f: Vec<&str> = line.split(',').collect();
            if f.len() < 4 {
                continue;
            }
            let (Ok(len), Ok(q)) = (f[2].trim().parse::<f64>(), f[3].trim().parse::<i32>()) else { continue };
            prj.parts.push(Part { length: len, quantity: q, material: f[1].trim().into(), use_it: true, a1: 90.0, a2: 90.0, ..Part::default() });
        }
        for m in ["DB10", "DB12", "DB16", "DB20", "DB25"] {
            prj.materials.push(Material::named(m));
            prj.stock.push(Stock { length: 12.0, quantity: 99999, material: m.into(), use_it: true, ..Stock::default() });
        }
        prj.technical.linear.optimization_level = 100;
        let s = optimize(&prj, &Progress { cancel: &|| false, report: &|_, _| {} });
        assert!(s.unplaced.is_empty());
        let mut bars = std::collections::BTreeMap::new();
        for b in &s.bars {
            *bars.entry(prj.stock[b.stock].material.clone()).or_insert(0) += b.quantity;
        }
        eprintln!("bars {bars:?} in {:.2} s", s.running_time);
        for (m, reference) in [("DB10", 905), ("DB12", 1099), ("DB16", 365), ("DB20", 86)] {
            assert!(bars.get(m).copied().unwrap_or(0) <= reference, "{m}: {:?} > {reference}", bars.get(m));
        }
    }

    #[test]
    fn nested_miters_share_the_cut() {
        let g = Geo { t: 3, space: 0, max_pieces: 0, oz: false };
        // 45 deg / 135 deg ends lean the same way: overlap H/tan(45) = H
        let (k, o, s) = join(45.0, 135.0, 50, &g);
        assert!((o - 50.0).abs() < 1e-9);
        assert!((k - 3.0 / (45f64.to_radians()).sin()).abs() < 1e-9);
        assert_eq!(s, 0.0);
    }
}
