//! The real-time dungeon layer, restored from Delve and adapted to Campus Crypt:
//! generated floors with walls that block line of sight (recursive
//! shadowcasting), monsters that path to you with a BFS flow field, melee
//! combat, projectiles, coffee pickups, and the Floor Keeper's wardens (bosses).
//!
//! This module never touches the run's resources. It reports what happened as
//! `WorldEvent`s (player hit, coffee drunk, warden down) and main.rs turns them
//! into `state::Run` calls, so state.rs stays the only place state changes.

use crate::art::*;
use crate::audio::Sfx;
use macroquad::prelude::*;
use macroquad::rand::gen_range;
use std::collections::VecDeque;

pub const TILE: f32 = 40.0;
const VIEW_RADIUS: i32 = 8;
const PLAYER_RADIUS: f32 = 13.0;
const PLAYER_SPEED: f32 = 235.0;
// ---- the foam-dart blaster (twin-stick, Enter the Gungeon style, school friendly)
/// How long the muzzle puff shows after a shot.
const MUZZLE_TIME: f32 = 0.08;
const DART_SPEED: f32 = 640.0;
const DART_RADIUS: f32 = 5.0;
/// How far a dart flies before it flops to the floor.
const DART_RANGE: f32 = 7.0 * TILE;
/// Dart damage as a share of `Power::damage` (one swing's worth).
const DART_DAMAGE: f32 = 0.6;
/// Time between darts as a share of `Power::cooldown` (one swing's).
const DART_RATE: f32 = 0.55;
/// Random spread per dart, radians.
const DART_SPREAD: f32 = 0.05;
// ---- the dodge roll
const DODGE_TIME: f32 = 0.32;
/// Can't be hit for this long from the start of a roll.
const DODGE_IFRAMES: f32 = 0.26;
/// Roll speed as a multiple of walking speed.
const DODGE_SPEED: f32 = 2.1;
/// Wait after a roll ends before the next one.
const DODGE_CD: f32 = 0.45;
// ---- slower, gentler enemies
/// Floor monsters' speed on floor 1 (share of their listed speed); it grows a
/// little each floor (`Tier::enemy_speed_scale`).
const ENEMY_SPEED_BASE: f32 = 0.78;
const ENEMY_SPEED_PER_LEVEL: f32 = 0.015;
/// Wardens walk and charge at this share of their listed speed.
const BOSS_SPEED_SCALE: f32 = 0.85;
/// Every enemy shot flies at this share of its listed speed (same range).
const ENEMY_SHOT_SPEED: f32 = 0.85;
/// Monsters per floor, one entry per difficulty level (floor 1 to 10).
const ENEMY_COUNT: [usize; 10] = [4, 5, 5, 6, 6, 7, 7, 8, 8, 9];
const INVULN: f32 = 1.4;
/// Warden health on floor 1, and how much each step of `Tier::depth` adds.
/// Tuned with `warden_difficulty` (tests below) on the original 4-floor run so
/// two wardens in a row are beatable from full health but still cost a few
/// hits; the 10-floor run stretches that same curve (see `Tier`).
const WARDEN_HP: f32 = 160.0;
#[cfg(test)]
thread_local!(static HIT_SOURCES: std::cell::Cell<[u32; 3]> = const { std::cell::Cell::new([0; 3]) });
/// test-only tally of what hit the player: 0 = a charge, 1 = a monster, 2 = a projectile
#[cfg(test)]
fn tally(src: usize) {
    HIT_SOURCES.with(|c| {
        let mut v = c.get();
        v[src] += 1;
        c.set(v);
    });
}
const WARDEN_HP_PER_FLOOR: f32 = 32.0;
/// The deepest `Tier::depth`: ten difficulty levels, 0 (floor 1) to 9 (floor 10).
pub const MAX_DEPTH: f32 = 9.0;
/// Where the last floor sits on the original 4-floor tuning (0 = its floor 1,
/// 3 = its floor 4). Scaled back from 3.0 so the whole run is gentler.
const CURVE_TOP: f32 = 2.25;
/// Warden health is scaled back by this much on every floor.
const WARDEN_EASE: f32 = 0.85;
const PICKUP_RANGE: f32 = 26.0;
const UNREACHED: u16 = u16::MAX;
const STEP: f32 = 1.0 / 120.0;
/// Floor maps (tiles). The arena fits the 1280x664 play area without scrolling.
const FLOOR_W: i32 = 44;
const FLOOR_H: i32 = 32;
const ARENA_W: i32 = 30;
const ARENA_H: i32 = 15;

// ---------------------------------------------------------------- difficulty

/// Where a floor sits on the difficulty curve.
///
/// Ten difficulty levels, one step per floor: `depth` goes from 0.0 on floor 1
/// to `MAX_DEPTH` (9.0) on floor 10, and every floor is a little harder than
/// the one before (monster count, speed and health, warden health, the monster
/// mix). The numbers still come from the original 4-floor tuning, read through
/// `curve()`: floor 10 plays like that run's floor 3.25 (`CURVE_TOP`), not its
/// floor 4, so the whole run is gentler.
///
/// Knowledge also grows per floor (at least +1 per lesson), and each point
/// makes your swing hit harder, so over 10 floors you'd outgrow the stretched
/// curve. `knowledge_scale` cancels that for the least-knowledgeable player
/// possible on this floor (one point per floor): they need the same number of
/// swings per warden and monster as on the 4-floor run at this depth. Anyone
/// who learned more still hits harder than the curve expects.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tier {
    /// 0-based floor index
    pub floor: u32,
    /// 0.0 ..= MAX_DEPTH (the difficulty level minus one)
    pub depth: f32,
    /// adaptive pacing on top of the curve (`Pace::NORMAL` unless the UI applies one)
    pub pace: Pace,
}

/// Adaptive pacing (from the AI-features skill tier, decided by code): monster
/// speed and count on a floor, warden health in the arena. `NORMAL` is the
/// tuned curve; main.rs caps what it applies so the warden sim stays fair.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pace {
    /// percent of normal floor-monster speed
    pub monster_speed_pct: u32,
    /// monsters added to (or removed from) a floor
    pub monster_count_delta: i32,
    /// percent of normal warden health
    pub warden_hp_pct: u32,
}

impl Pace {
    pub const NORMAL: Pace = Pace { monster_speed_pct: 100, monster_count_delta: 0, warden_hp_pct: 100 };
    /// Fastest floor monsters adaptive pacing may ask for.
    pub const MAX_SPEED_PCT: u32 = 110;
    /// Wardens never get tougher than the tuned curve: a slow player who missed a
    /// Keeper must still beat floor 10's pair about half the time (`skill_tier_pacing_*` tests).
    pub const MAX_WARDEN_HP_PCT: u32 = 100;

    /// The pace actually applied for suggested knobs (speed and warden health capped).
    pub fn capped(monster_speed_pct: u32, monster_count_delta: i32, warden_hp_pct: u32) -> Pace {
        Pace {
            monster_speed_pct: monster_speed_pct.clamp(50, Pace::MAX_SPEED_PCT),
            monster_count_delta: monster_count_delta.clamp(-2, 2),
            warden_hp_pct: warden_hp_pct.clamp(50, Pace::MAX_WARDEN_HP_PCT),
        }
    }
}

impl Default for Pace {
    fn default() -> Pace {
        Pace::NORMAL
    }
}

impl Tier {
    /// Floor `floor` (0-based) of a run with `floors` floors.
    pub fn of(floor: u32, floors: u32) -> Tier {
        let last = floors.max(1) - 1;
        let floor = floor.min(last);
        let depth = if last == 0 { 0.0 } else { MAX_DEPTH * floor as f32 / last as f32 };
        Tier { floor, depth, pace: Pace::NORMAL }
    }

    /// The same floor with adaptive pacing applied.
    pub fn with_pace(self, pace: Pace) -> Tier {
        Tier { pace, ..self }
    }

    /// Monsters actually placed: the curve's count plus the pace delta (at least 3).
    pub fn paced_enemy_count(self) -> usize {
        (self.enemy_count() as i64 + i64::from(self.pace.monster_count_delta)).max(3) as usize
    }

    /// Difficulty level, 1 (floor 1) to 10 (floor 10).
    pub fn level(self) -> u32 {
        self.depth.round() as u32 + 1
    }

    /// Where this floor sits on the original 4-floor tuning (0.0 ..= CURVE_TOP).
    pub fn curve(self) -> f32 {
        self.depth / MAX_DEPTH * CURVE_TOP
    }

    /// Floor monsters' speed, as a share of their listed speed.
    pub fn enemy_speed_scale(self) -> f32 {
        ENEMY_SPEED_BASE + ENEMY_SPEED_PER_LEVEL * self.depth
    }

    /// Swing damage at `k` Knowledge (the float form of `Power::damage`).
    fn swing(k: f32) -> f32 {
        10.0 + 1.5 * k.clamp(0.0, 20.0)
    }

    /// See the type docs: 1.0 on a 4-floor run, above 1 on deeper floors of a longer one.
    pub fn knowledge_scale(self) -> f32 {
        Tier::swing(self.floor as f32 + 1.0) / Tier::swing(self.curve() + 1.0)
    }

    /// Health of a warden with toughness 1.0.
    pub fn warden_hp(self) -> f32 {
        (WARDEN_HP + WARDEN_HP_PER_FLOOR * self.curve()) * WARDEN_EASE * self.knowledge_scale()
    }

    pub fn enemy_hp_scale(self) -> f32 {
        (0.9 + 0.12 * self.curve()) * self.knowledge_scale()
    }

    /// Monsters on the floor (4 on floor 1, 9 on floor 10).
    pub fn enemy_count(self) -> usize {
        ENEMY_COUNT[(self.level() as usize - 1).min(ENEMY_COUNT.len() - 1)]
    }

    /// Rooms the generator aims for.
    fn rooms(self) -> usize {
        (7 + self.curve().round() as usize).min(11)
    }

    /// Map size in tiles: deeper floors are a little bigger (44x32 up to 49x34).
    pub fn map_size(self) -> (i32, i32) {
        (FLOOR_W + (2.0 * self.curve()).round() as i32, FLOOR_H + self.curve().round() as i32)
    }
}

// ---------------------------------------------------------------- map

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tile {
    Wall,
    Floor,
    Stairs,
}

#[derive(Clone, Copy, Debug)]
pub struct Room {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Room {
    pub fn center(&self) -> (i32, i32) {
        (self.x + self.w / 2, self.y + self.h / 2)
    }
    fn intersects(&self, o: &Room, margin: i32) -> bool {
        self.x - margin < o.x + o.w && self.x + self.w + margin > o.x && self.y - margin < o.y + o.h && self.y + self.h + margin > o.y
    }
    fn random_tile(&self) -> (i32, i32) {
        (gen_range(self.x + 1, self.x + self.w - 1), gen_range(self.y + 1, self.y + self.h - 1))
    }
}

pub struct Map {
    pub w: i32,
    pub h: i32,
    pub tiles: Vec<Tile>,
    pub seen: Vec<bool>,
    pub visible: Vec<bool>,
    pub rooms: Vec<Room>,
    pub stairs: (i32, i32),
    /// BFS steps toward the player, rebuilt when the player changes tile
    flow: Vec<u16>,
    flow_queue: VecDeque<u32>,
    flow_origin: (i32, i32),
    fov_origin: (i32, i32),
}

pub fn tile_center(t: (i32, i32)) -> Vec2 {
    vec2((t.0 as f32 + 0.5) * TILE, (t.1 as f32 + 0.5) * TILE)
}

pub fn tile_of(p: Vec2) -> (i32, i32) {
    ((p.x / TILE).floor() as i32, (p.y / TILE).floor() as i32)
}

impl Map {
    fn blank(w: i32, h: i32) -> Map {
        let n = (w * h) as usize;
        Map {
            w,
            h,
            tiles: vec![Tile::Wall; n],
            seen: vec![false; n],
            visible: vec![false; n],
            rooms: Vec::new(),
            stairs: (0, 0),
            flow: vec![UNREACHED; n],
            flow_queue: VecDeque::with_capacity(n),
            flow_origin: (-1, -1),
            fov_origin: (-1, -1),
        }
    }
    pub fn idx(&self, x: i32, y: i32) -> usize {
        (y * self.w + x) as usize
    }
    pub fn in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && y >= 0 && x < self.w && y < self.h
    }
    pub fn get(&self, x: i32, y: i32) -> Tile {
        if self.in_bounds(x, y) {
            self.tiles[self.idx(x, y)]
        } else {
            Tile::Wall
        }
    }
    pub fn solid(&self, x: i32, y: i32) -> bool {
        self.get(x, y) == Tile::Wall
    }
    pub fn solid_at(&self, p: Vec2) -> bool {
        let (x, y) = tile_of(p);
        self.solid(x, y)
    }
    pub fn is_visible(&self, p: Vec2) -> bool {
        let (x, y) = tile_of(p);
        self.in_bounds(x, y) && self.visible[self.idx(x, y)]
    }
    pub fn is_seen(&self, p: Vec2) -> bool {
        let (x, y) = tile_of(p);
        self.in_bounds(x, y) && self.seen[self.idx(x, y)]
    }
    fn carve(&mut self, x: i32, y: i32) {
        if x > 0 && y > 0 && x < self.w - 1 && y < self.h - 1 {
            let i = self.idx(x, y);
            self.tiles[i] = Tile::Floor;
        }
    }

    /// Rooms joined by corridors (Delve's generator, sized for Campus Crypt).
    /// The stairs go in the room farthest from the start room.
    pub fn generate(tier: Tier) -> Map {
        let (w, h) = tier.map_size();
        let mut m = Map::blank(w, h);
        let target_rooms = tier.rooms();
        for _ in 0..600 {
            if m.rooms.len() >= target_rooms {
                break;
            }
            let w = gen_range(6, 11);
            let h = gen_range(5, 9);
            let r = Room { x: gen_range(1, m.w - w - 1), y: gen_range(1, m.h - h - 1), w, h };
            if m.rooms.iter().any(|o| r.intersects(o, 2)) {
                continue;
            }
            for yy in r.y..r.y + r.h {
                for xx in r.x..r.x + r.w {
                    m.carve(xx, yy);
                }
            }
            // a pillar pair in big rooms gives line of sight something to do
            if r.w >= 9 && r.h >= 7 && gen_range(0, 2) == 0 {
                let (cx, cy) = r.center();
                let (a, b) = (m.idx(cx - 2, cy - 1), m.idx(cx + 2, cy + 1));
                m.tiles[a] = Tile::Wall;
                m.tiles[b] = Tile::Wall;
            }
            if let Some(prev) = m.rooms.last().copied() {
                let (ax, ay) = prev.center();
                let (bx, by) = r.center();
                let wide = gen_range(0, 2) == 0;
                if gen_range(0, 2) == 0 {
                    m.h_corridor(ax, bx, ay, wide);
                    m.v_corridor(ay, by, bx, wide);
                } else {
                    m.v_corridor(ay, by, ax, wide);
                    m.h_corridor(ax, bx, by, wide);
                }
            }
            m.rooms.push(r);
        }
        // a couple of loops so it's not one long line
        for _ in 0..2 {
            let n = m.rooms.len();
            if n < 4 {
                break;
            }
            let a = m.rooms[gen_range(0, n)].center();
            let b = m.rooms[gen_range(0, n)].center();
            m.h_corridor(a.0, b.0, a.1, false);
            m.v_corridor(a.1, b.1, b.0, false);
        }
        for r in m.rooms.clone() {
            let (cx, cy) = r.center();
            let i = m.idx(cx, cy);
            m.tiles[i] = Tile::Floor;
        }
        let (sx, sy) = m.rooms[0].center();
        let far = m
            .rooms
            .iter()
            .skip(1)
            .max_by_key(|r| {
                let (cx, cy) = r.center();
                (cx - sx).pow(2) + (cy - sy).pow(2)
            })
            .copied()
            .unwrap_or(m.rooms[0]);
        m.stairs = far.center();
        let i = m.idx(m.stairs.0, m.stairs.1);
        m.tiles[i] = Tile::Stairs;
        m
    }

    /// One open hall with four pillars, fully lit: where the wardens fight.
    pub fn arena() -> Map {
        let mut m = Map::blank(ARENA_W, ARENA_H);
        for y in 1..m.h - 1 {
            for x in 1..m.w - 1 {
                m.carve(x, y);
            }
        }
        for (x, y) in [(7, 4), (22, 4), (7, 10), (22, 10)] {
            let i = m.idx(x, y);
            m.tiles[i] = Tile::Wall;
        }
        m.rooms.push(Room { x: 1, y: 1, w: m.w - 2, h: m.h - 2 });
        m.stairs = (-1, -1);
        m.seen.fill(true);
        m.visible.fill(true);
        m
    }

    fn h_corridor(&mut self, x1: i32, x2: i32, y: i32, wide: bool) {
        for x in x1.min(x2)..=x1.max(x2) {
            self.carve(x, y);
            if wide {
                self.carve(x, y + 1);
            }
        }
    }
    fn v_corridor(&mut self, y1: i32, y2: i32, x: i32, wide: bool) {
        for y in y1.min(y2)..=y1.max(y2) {
            self.carve(x, y);
            if wide {
                self.carve(x + 1, y);
            }
        }
    }

    /// Recursive shadowcasting from tile (cx, cy). Walls block sight but are
    /// themselves visible. Recomputed only when the player changes tile.
    pub fn compute_fov(&mut self, cx: i32, cy: i32, radius: i32) {
        if (cx, cy) == self.fov_origin {
            return;
        }
        self.fov_origin = (cx, cy);
        self.visible.fill(false);
        if !self.in_bounds(cx, cy) {
            return;
        }
        let i = self.idx(cx, cy);
        self.visible[i] = true;
        self.seen[i] = true;
        const MULT: [[i32; 8]; 4] = [
            [1, 0, 0, -1, -1, 0, 0, 1],
            [0, 1, -1, 0, 0, -1, 1, 0],
            [0, 1, 1, 0, 0, -1, -1, 0],
            [1, 0, 0, 1, -1, 0, 0, -1],
        ];
        for oct in 0..8 {
            self.cast_light(cx, cy, 1, 1.0, 0.0, radius, [MULT[0][oct], MULT[1][oct], MULT[2][oct], MULT[3][oct]]);
        }
        // remember walls that hug any visible floor tile, so lit corridors never float in the void
        let r = radius + 1;
        for y in (cy - r).max(1)..(cy + r).min(self.h - 1) {
            for x in (cx - r).max(1)..(cx + r).min(self.w - 1) {
                let i = self.idx(x, y);
                if !self.visible[i] || self.tiles[i] == Tile::Wall {
                    continue;
                }
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
                    let j = self.idx(x + dx, y + dy);
                    if self.tiles[j] == Tile::Wall {
                        self.seen[j] = true;
                        self.visible[j] = true;
                    }
                }
            }
        }
    }

    // the classic recursive-shadowcasting signature (ported from Delve); bundling it into a struct only adds noise
    #[allow(clippy::too_many_arguments)]
    fn cast_light(&mut self, cx: i32, cy: i32, row: i32, mut start: f32, end: f32, radius: i32, m: [i32; 4]) {
        if start < end {
            return;
        }
        let [xx, xy, yx, yy] = m;
        let r2 = radius * radius + radius;
        let mut new_start = 0.0;
        let mut j = row;
        while j <= radius {
            let dy = -j;
            let mut dx = -j - 1;
            let mut blocked = false;
            while dx <= 0 {
                dx += 1;
                let x = cx + dx * xx + dy * xy;
                let y = cy + dx * yx + dy * yy;
                let l_slope = (dx as f32 - 0.5) / (dy as f32 + 0.5);
                let r_slope = (dx as f32 + 0.5) / (dy as f32 - 0.5);
                if start < r_slope {
                    continue;
                } else if end > l_slope {
                    break;
                }
                if dx * dx + dy * dy <= r2 && self.in_bounds(x, y) {
                    let i = self.idx(x, y);
                    self.visible[i] = true;
                    self.seen[i] = true;
                }
                let wall = self.solid(x, y);
                if blocked {
                    if wall {
                        new_start = r_slope;
                        continue;
                    }
                    blocked = false;
                    start = new_start;
                } else if wall && j < radius {
                    blocked = true;
                    self.cast_light(cx, cy, j + 1, start, l_slope, radius, m);
                    new_start = r_slope;
                }
            }
            if blocked {
                break;
            }
            j += 1;
        }
    }

    /// True if a straight segment between two points crosses no wall tile.
    pub fn line_clear(&self, a: Vec2, b: Vec2) -> bool {
        let d = b - a;
        let steps = (d.length() / 8.0).ceil().max(1.0) as i32;
        (1..steps).all(|k| !self.solid_at(a + d * (k as f32 / steps as f32)))
    }

    /// Breadth-first distances (in steps) from `origin` into `out`.
    fn bfs(&self, origin: (i32, i32), max_steps: u16, out: &mut Vec<u16>, queue: &mut VecDeque<u32>) {
        out.clear();
        out.resize((self.w * self.h) as usize, UNREACHED);
        queue.clear();
        if self.solid(origin.0, origin.1) {
            return;
        }
        let oi = self.idx(origin.0, origin.1);
        out[oi] = 0;
        queue.push_back(oi as u32);
        while let Some(c) = queue.pop_front() {
            let cd = out[c as usize];
            if cd >= max_steps {
                continue;
            }
            let (cx, cy) = (c as i32 % self.w, c as i32 / self.w);
            for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                let (nx, ny) = (cx + dx, cy + dy);
                if self.solid(nx, ny) {
                    continue;
                }
                let ni = self.idx(nx, ny);
                if out[ni] == UNREACHED {
                    out[ni] = cd + 1;
                    queue.push_back(ni as u32);
                }
            }
        }
    }

    pub fn update_flow(&mut self, origin: (i32, i32)) {
        if origin == self.flow_origin || self.solid(origin.0, origin.1) {
            return;
        }
        self.flow_origin = origin;
        let mut flow = std::mem::take(&mut self.flow);
        let mut q = std::mem::take(&mut self.flow_queue);
        self.bfs(origin, 40, &mut flow, &mut q);
        self.flow = flow;
        self.flow_queue = q;
    }

    /// Step direction down a distance field (8-way, no corner cutting).
    fn downhill(&self, field: &[u16], pos: Vec2) -> Vec2 {
        let t = tile_of(pos);
        let at = |t: (i32, i32)| if self.in_bounds(t.0, t.1) { field[self.idx(t.0, t.1)] } else { UNREACHED };
        let here = at(t);
        if here == UNREACHED {
            return Vec2::ZERO;
        }
        let (mut best, mut best_t) = (here, t);
        for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)] {
            if dx != 0 && dy != 0 && (self.solid(t.0 + dx, t.1) || self.solid(t.0, t.1 + dy)) {
                continue;
            }
            let n = (t.0 + dx, t.1 + dy);
            let d = at(n);
            if d < best {
                best = d;
                best_t = n;
            }
        }
        if best_t == t {
            return Vec2::ZERO;
        }
        (tile_center(best_t) - pos).normalize_or_zero()
    }

    pub fn flow_dir(&self, pos: Vec2) -> Vec2 {
        self.downhill(&self.flow, pos)
    }

    fn random_floor_in(&self, room: &Room) -> (i32, i32) {
        for _ in 0..30 {
            let t = room.random_tile();
            if self.get(t.0, t.1) == Tile::Floor {
                return t;
            }
        }
        room.center()
    }
}

/// Slide along walls: try x and y separately (Delve's collision).
fn move_and_collide(map: &Map, pos: &mut Vec2, delta: Vec2, r: f32) {
    let blocked = |p: Vec2| {
        map.solid_at(p + vec2(-r, -r)) || map.solid_at(p + vec2(r, -r)) || map.solid_at(p + vec2(-r, r)) || map.solid_at(p + vec2(r, r))
    };
    let nx = *pos + vec2(delta.x, 0.0);
    if !blocked(nx) {
        *pos = nx;
    }
    let ny = *pos + vec2(0.0, delta.y);
    if !blocked(ny) {
        *pos = ny;
    }
}

// ---------------------------------------------------------------- entities

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
/// The crawl monsters: a playful campus-life roster in black and gold (original
/// art; no real logos, marks or mascot likeness).
pub enum EnemyKind {
    /// the dining hall's mystery mac and cheese, wobbling after you
    MysteryMeat,
    /// an owl that won't stop forwarding junk mail (swoops side to side)
    SpamOwl,
    /// an all-nighter owl: faster, tougher, appears from floor 5
    NightOwl,
    /// finals week in a hoodie: slow, heavy, shrugs off knockback
    FinalsBrute,
    /// writes you a parking ticket from across the room (ranged)
    ParkingPatrol,
    /// a campus Wi-Fi dead zone that lobs no-signal orbs (ranged)
    DeadZoneCaster,
    /// the Kennesaw-Marietta shuttle, running late and straight at you; from floor 8
    ShuttleRusher,
}

impl EnemyKind {
    pub const ALL: [EnemyKind; 7] = [
        EnemyKind::MysteryMeat,
        EnemyKind::SpamOwl,
        EnemyKind::NightOwl,
        EnemyKind::FinalsBrute,
        EnemyKind::ParkingPatrol,
        EnemyKind::DeadZoneCaster,
        EnemyKind::ShuttleRusher,
    ];
    pub fn name(self) -> &'static str {
        match self {
            EnemyKind::MysteryMeat => "Mystery Mac Slime",
            EnemyKind::SpamOwl => "Spam Owl",
            EnemyKind::NightOwl => "Night Owl",
            EnemyKind::FinalsBrute => "Finals Week Brute",
            EnemyKind::ParkingPatrol => "Parking Patrol",
            EnemyKind::DeadZoneCaster => "Dead-Zone Caster",
            EnemyKind::ShuttleRusher => "Late Shuttle",
        }
    }
    fn ranged(self) -> bool {
        matches!(self, EnemyKind::ParkingPatrol | EnemyKind::DeadZoneCaster)
    }
    fn owl(self) -> bool {
        matches!(self, EnemyKind::SpamOwl | EnemyKind::NightOwl)
    }
    /// (hp, speed, radius)
    fn stats(self) -> (f32, f32, f32) {
        match self {
            EnemyKind::MysteryMeat => (24.0, 80.0, 15.0),
            EnemyKind::SpamOwl => (14.0, 150.0, 12.0),
            EnemyKind::NightOwl => (20.0, 165.0, 12.0),
            EnemyKind::FinalsBrute => (52.0, 92.0, 18.0),
            EnemyKind::ParkingPatrol => (18.0, 95.0, 13.0),
            EnemyKind::DeadZoneCaster => (22.0, 80.0, 13.0),
            EnemyKind::ShuttleRusher => (30.0, 125.0, 16.0),
        }
    }
    fn sprite(self) -> &'static [&'static str; 16] {
        match self {
            EnemyKind::MysteryMeat => &MYSTERY_MEAT,
            EnemyKind::SpamOwl => &SPAM_OWL,
            EnemyKind::NightOwl => &NIGHT_OWL,
            EnemyKind::FinalsBrute => &FINALS_BRUTE,
            EnemyKind::ParkingPatrol => &PARKING_PATROL,
            EnemyKind::DeadZoneCaster => &DEAD_ZONE_CASTER,
            EnemyKind::ShuttleRusher => &LATE_SHUTTLE,
        }
    }
    fn color(self) -> Color {
        match self {
            EnemyKind::MysteryMeat => Color::from_rgba(196, 150, 70, 255),
            EnemyKind::SpamOwl => Color::from_rgba(255, 196, 40, 255),
            EnemyKind::NightOwl => Color::from_rgba(150, 150, 170, 255),
            EnemyKind::FinalsBrute => Color::from_rgba(210, 70, 50, 255),
            EnemyKind::ParkingPatrol => Color::from_rgba(250, 230, 120, 255),
            EnemyKind::DeadZoneCaster => Color::from_rgba(70, 200, 230, 255),
            EnemyKind::ShuttleRusher => Color::from_rgba(255, 196, 40, 255),
        }
    }
}

/// What the monsters on a floor at `depth` are drawn from: percent weights per
/// kind (same order as `EnemyKind::ALL`), one row per difficulty level, so
/// each floor brings in a little more of the tougher kinds.
pub fn enemy_mix(depth: f32) -> [u32; 7] {
    const MIX: [[u32; 7]; 10] = [
        //meat owl night brute patrol deadzone shuttle
        [60, 40, 0, 0, 0, 0, 0],
        [50, 40, 0, 0, 10, 0, 0],
        [42, 35, 0, 0, 23, 0, 0],
        [35, 30, 0, 0, 25, 10, 0],
        [30, 25, 10, 0, 25, 10, 0],
        [27, 22, 12, 5, 22, 12, 0],
        [24, 20, 12, 10, 20, 14, 0],
        [21, 18, 12, 12, 18, 15, 4],
        [19, 16, 11, 13, 17, 16, 8],
        [17, 15, 10, 13, 17, 16, 12],
    ];
    MIX[(depth.max(0.0).round() as usize).min(MIX.len() - 1)]
}

fn roll_enemy(depth: f32) -> EnemyKind {
    let mix = enemy_mix(depth);
    let total: u32 = mix.iter().sum();
    let mut roll = gen_range(0, total);
    for (k, w) in EnemyKind::ALL.iter().zip(mix) {
        if roll < w {
            return *k;
        }
        roll -= w;
    }
    EnemyKind::MysteryMeat
}

pub struct Enemy {
    pub kind: EnemyKind,
    pub pos: Vec2,
    hp: f32,
    speed: f32,
    radius: f32,
    flash: f32,
    knock: Vec2,
    wander: Vec2,
    wander_t: f32,
    anim: f32,
    aggro: bool,
    last_seen: f32,
    flank: f32,
    attack_cd: f32,
    windup: f32,
    facing: Vec2,
}

impl Enemy {
    fn new(kind: EnemyKind, pos: Vec2, tier: Tier) -> Enemy {
        let (hp, speed, radius) = kind.stats();
        Enemy {
            kind,
            pos,
            hp: hp * tier.enemy_hp_scale(),
            speed: speed * tier.enemy_speed_scale() * tier.pace.monster_speed_pct as f32 / 100.0,
            radius,
            flash: 0.0,
            knock: Vec2::ZERO,
            wander: Vec2::ZERO,
            wander_t: 0.0,
            anim: gen_range(0.0, 6.0),
            aggro: false,
            last_seen: 99.0,
            flank: if gen_range(0, 2) == 0 { 1.0 } else { -1.0 },
            attack_cd: gen_range(0.5, 1.5),
            windup: 0.0,
            facing: vec2(1.0, 0.0),
        }
    }
}

/// The Floor Keeper's wardens. Each has one readable tell before it attacks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum BossKind {
    /// a black-and-gold tow truck from the parking deck: aims, then charges
    TowTyrant,
    /// a stack of finals-week blue books: a ring of exam pages
    FinalsGolem,
    /// a crowned owl that sprays club flyers and calls Spam Owls
    OwlQueen,
    /// haunts the library stacks: fades, reappears, spins padlocks
    LibraryLich,
}

impl BossKind {
    pub const ALL: [BossKind; 4] = [BossKind::TowTyrant, BossKind::FinalsGolem, BossKind::OwlQueen, BossKind::LibraryLich];
    pub fn name(self) -> &'static str {
        match self {
            BossKind::TowTyrant => "THE TOW TRUCK TYRANT",
            BossKind::FinalsGolem => "THE FINALS WEEK GOLEM",
            BossKind::OwlQueen => "THE NIGHT OWL QUEEN",
            BossKind::LibraryLich => "THE LIBRARY STACKS LICH",
        }
    }
    /// One-line hint shown when the warden enters.
    pub fn tell(self) -> &'static str {
        match self {
            BossKind::TowTyrant => "Its beacon flashes and a red line points at you... then it CHARGES. Sidestep.",
            BossKind::FinalsGolem => "A red ring grows on the floor, then exam pages fly out in every direction.",
            BossKind::OwlQueen => "She sprays club flyers and calls Spam Owls. Clear the owls, then chase her.",
            BossKind::LibraryLich => "It fades into the stacks, then spins padlock bolts. Hit it while it's solid.",
        }
    }
    pub fn sprite(self) -> &'static [&'static str; 16] {
        match self {
            BossKind::TowTyrant => &TOW_TYRANT,
            BossKind::FinalsGolem => &FINALS_GOLEM,
            BossKind::OwlQueen => &OWL_QUEEN,
            BossKind::LibraryLich => &LIBRARY_LICH,
        }
    }
    pub fn color(self) -> Color {
        match self {
            BossKind::TowTyrant => Color::from_rgba(255, 196, 40, 255),
            BossKind::FinalsGolem => Color::from_rgba(214, 64, 70, 255),
            BossKind::OwlQueen => Color::from_rgba(250, 230, 120, 255),
            BossKind::LibraryLich => Color::from_rgba(150, 156, 180, 255),
        }
    }
    fn speed(self) -> f32 {
        BOSS_SPEED_SCALE
            * match self {
                BossKind::TowTyrant => 95.0,
                BossKind::FinalsGolem => 60.0,
                BossKind::OwlQueen => 100.0,
                BossKind::LibraryLich => 45.0,
            }
    }
}

/// Warden pairs by floor (indices into `BossKind::ALL`: Tow, Golem, Queen,
/// Lich), easiest first. Which pair you meet matters more than warden health
/// (the two ranged wardens, the Queen and the Lich, are the hard ones, and the
/// second warden is fought with whatever hearts are left), so the order was
/// picked with `warden_difficulty` to fall smoothly from "always survive" on
/// floor 1 to about the old floor 4 on floor 10, meeting all six pairings.
const WARDEN_PAIRS: [(usize, usize); 10] = [(0, 1), (0, 2), (1, 2), (0, 3), (1, 3), (2, 3), (0, 3), (2, 3), (0, 3), (2, 3)];

/// The two wardens that guard floor `floor` after a wrong answer (always two different ones).
pub fn wardens_for(floor: usize) -> [BossKind; 2] {
    let (a, b) = WARDEN_PAIRS[floor % WARDEN_PAIRS.len()];
    [BossKind::ALL[a], BossKind::ALL[b]]
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum BossState {
    /// moving / positioning
    Walk,
    /// telegraphing the next attack (drawn on screen)
    Tell,
    /// attacking
    Act,
}

pub struct Boss {
    pub kind: BossKind,
    pub pos: Vec2,
    pub hp: f32,
    pub max_hp: f32,
    radius: f32,
    state: BossState,
    t: f32,
    dir: Vec2,
    flash: f32,
    anim: f32,
    spin: f32,
    shot_t: f32,
    volleys: u32,
    /// 1 = solid, 0 = faded out (the Lich can't be hit while faded)
    alpha: f32,
    teleport_to: Option<Vec2>,
    entering: f32,
}

impl Boss {
    fn new(kind: BossKind, pos: Vec2, tier: Tier) -> Boss {
        // the ranged wardens keep their distance and fire more, so they have less health
        let toughness = match kind {
            BossKind::TowTyrant | BossKind::FinalsGolem => 1.0,
            BossKind::OwlQueen => 0.75,
            BossKind::LibraryLich => 0.8,
        };
        let max_hp = tier.warden_hp() * toughness * tier.pace.warden_hp_pct as f32 / 100.0;
        Boss {
            kind,
            pos,
            hp: max_hp,
            max_hp,
            radius: 34.0,
            state: BossState::Walk,
            t: 1.6,
            dir: Vec2::ZERO,
            flash: 0.0,
            anim: 0.0,
            spin: 0.0,
            shot_t: 0.0,
            volleys: 0,
            alpha: 1.0,
            teleport_to: None,
            entering: 1.0,
        }
    }
    pub fn enraged(&self) -> bool {
        self.hp < self.max_hp * 0.5
    }
    fn hittable(&self) -> bool {
        self.alpha > 0.6 && self.entering <= 0.0
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProjKind {
    /// the Tow Tyrant's tow hooks
    Hook,
    /// Parking Patrol's tickets
    Ticket,
    Orb,
    Page,
    Envelope,
    Padlock,
}

struct Projectile {
    pos: Vec2,
    vel: Vec2,
    radius: f32,
    life: f32,
    kind: ProjKind,
}

/// Enemy shots fly at `ENEMY_SHOT_SPEED` and live longer, so they still reach as far.
fn slow_shot(mut p: Projectile) -> Projectile {
    p.vel *= ENEMY_SHOT_SPEED;
    p.life /= ENEMY_SHOT_SPEED;
    p
}

/// A foam dart from the player's blaster.
struct Dart {
    pos: Vec2,
    vel: Vec2,
    life: f32,
}

struct Pickup {
    pos: Vec2,
    phase: f32,
}

struct Particle {
    pos: Vec2,
    vel: Vec2,
    life: f32,
    max: f32,
    color: Color,
    size: f32,
}

struct FloatText {
    pos: Vec2,
    text: String,
    life: f32,
    color: Color,
}

pub struct Player {
    pub pos: Vec2,
    pub facing: Vec2,
    attack_cd: f32,
    /// muzzle puff timer after a shot
    swing_t: f32,
    /// where the blaster points (aim, updated every frame)
    swing_dir: Vec2,
    /// rolling while > 0
    dodge_t: f32,
    /// next roll allowed when <= 0
    dodge_cd: f32,
    dodge_dir: Vec2,
    pub invuln: f32,
    knock: Vec2,
    walk: f32,
}

/// What happened this frame, for main.rs to turn into `Run` calls / sounds / hardware events.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WorldEvent {
    PlayerHit,
    Healed,
    EnemyKilled,
    BossDown(BossKind),
    BossEntered(BossKind),
}

/// One frame of player intent.
#[derive(Clone, Copy, Default, Debug)]
pub struct Control {
    pub mv: Vec2,
    pub attack: bool,
    /// aim point in world coordinates (mouse); None = aim assist (nearest
    /// monster or warden in sight and range), else where you're facing
    pub aim: Option<Vec2>,
    /// start a dodge roll (pressed this frame)
    pub dodge: bool,
}

/// Player-side numbers that come from the run's resources.
#[derive(Clone, Copy, Debug)]
pub struct Power {
    /// Knowledge: each point hits harder
    pub knowledge: i32,
    /// Focus: each point fires faster
    pub focus: i32,
    /// below max health (coffee only gets drunk when it would help)
    pub can_heal: bool,
}

impl Power {
    pub fn damage(&self) -> f32 {
        10.0 + 1.5 * self.knowledge.clamp(0, 20) as f32
    }
    pub fn cooldown(&self) -> f32 {
        0.42 - 0.035 * self.focus.clamp(0, 5) as f32
    }
}

/// A side prop on a floor (main.rs uses it for the AI-features mini-games:
/// a suspicious message, a pair of messages to compare). Drawn like a pickup;
/// the world never acts on it.
#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub pos: Vec2,
    pub sprite: &'static [&'static str; 16],
    pub glow: Color,
    /// read / answered: drawn dim, no glow
    pub done: bool,
}

pub struct World {
    pub map: Map,
    /// optional side prop (see `Prop`)
    pub prop: Option<Prop>,
    /// floor index and where it sits on the difficulty curve
    pub tier: Tier,
    pub arena: bool,
    pub player: Player,
    pub enemies: Vec<Enemy>,
    projectiles: Vec<Projectile>,
    darts: Vec<Dart>,
    pickups: Vec<Pickup>,
    particles: Vec<Particle>,
    texts: Vec<FloatText>,
    pub boss: Option<Boss>,
    pub boss_queue: Vec<BossKind>,
    pub bosses_total: usize,
    boss_delay: f32,
    /// the floor's character (the informational encounter)
    pub npc: Vec2,
    /// the Floor Keeper, standing on the stairs until it steps aside
    pub keeper: Vec2,
    pub keeper_aside: Vec2,
    pub stairs: Vec2,
    pub cam: Vec2,
    shake: f32,
    pub events: Vec<WorldEvent>,
    pub sounds: Vec<Sfx>,
    /// CC_GOD=1: nothing hurts (screenshots / automated play-tests)
    pub god: bool,
    time: f32,
    acc: f32,
    tex: Option<Texture2D>,
    seed: u32,
    auto_field: Vec<u16>,
    auto_target: (i32, i32),
    auto_queue: VecDeque<u32>,
}

impl World {
    fn base(map: Map, tier: Tier, arena: bool, start: Vec2) -> World {
        World {
            map,
            prop: None,
            tier,
            arena,
            player: Player {
                pos: start,
                facing: vec2(1.0, 0.0),
                attack_cd: 0.0,
                swing_t: 0.0,
                swing_dir: vec2(1.0, 0.0),
                dodge_t: 0.0,
                dodge_cd: 0.0,
                dodge_dir: vec2(1.0, 0.0),
                invuln: 0.0,
                knock: Vec2::ZERO,
                walk: 0.0,
            },
            enemies: Vec::new(),
            projectiles: Vec::new(),
            darts: Vec::new(),
            pickups: Vec::new(),
            particles: Vec::new(),
            texts: Vec::new(),
            boss: None,
            boss_queue: Vec::new(),
            bosses_total: 0,
            boss_delay: 0.0,
            npc: Vec2::ZERO,
            keeper: Vec2::ZERO,
            keeper_aside: Vec2::ZERO,
            stairs: Vec2::ZERO,
            cam: start,
            shake: 0.0,
            events: Vec::new(),
            sounds: Vec::new(),
            god: false,
            time: 0.0,
            acc: 0.0,
            tex: None,
            seed: gen_range(0u32, u32::MAX),
            auto_field: Vec::new(),
            auto_target: (-1, -1),
            auto_queue: VecDeque::new(),
        }
    }

    /// A fresh floor: start room, the floor's character mid-way, the Keeper on the stairs.
    pub fn floor(tier: Tier) -> World {
        let map = Map::generate(tier);
        let start = tile_center(map.rooms[0].center());
        let stairs_t = map.stairs;
        let mut w = World::base(map, tier, false, start);
        w.stairs = tile_center(stairs_t);
        w.keeper = w.stairs;
        w.keeper_aside = [(1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (-1, 1)]
            .iter()
            .map(|&(dx, dy)| (stairs_t.0 + dx, stairs_t.1 + dy))
            .find(|&(x, y)| w.map.get(x, y) == Tile::Floor)
            .map_or(w.stairs + vec2(TILE, 0.0), tile_center);
        // the character waits in a room about half-way (by walking distance) to the stairs
        let mut field = Vec::new();
        let mut q = VecDeque::new();
        w.map.bfs(w.map.rooms[0].center(), u16::MAX - 1, &mut field, &mut q);
        let dist = |r: &Room| {
            let c = r.center();
            field[w.map.idx(c.0, c.1)]
        };
        let total = field[w.map.idx(stairs_t.0, stairs_t.1)];
        let mut candidates: Vec<Room> =
            w.map.rooms.iter().skip(1).copied().filter(|r| r.center() != stairs_t && dist(r) != UNREACHED).collect();
        candidates.sort_by_key(|r| (i32::from(dist(r)) - i32::from(total) / 2).abs());
        let npc_room = candidates.first().copied().unwrap_or(w.map.rooms[0]);
        let (nx, ny) = npc_room.center();
        w.npc = if w.map.get(nx, ny - 1) == Tile::Floor { tile_center((nx, ny - 1)) } else { tile_center((nx, ny)) };
        w.populate();
        let p = tile_of(w.player.pos);
        w.map.compute_fov(p.0, p.1, VIEW_RADIUS);
        w
    }

    /// Put a side prop in a reachable room that isn't the start, the floor
    /// character's room or the stairs room (falls back to any other room).
    pub fn place_prop(&mut self, sprite: &'static [&'static str; 16], glow: Color) -> Vec2 {
        let mut field = Vec::new();
        let mut q = VecDeque::new();
        self.map.bfs(self.map.rooms[0].center(), u16::MAX - 1, &mut field, &mut q);
        let (npc_t, stairs_t) = (tile_of(self.npc), tile_of(self.stairs));
        let inside = |r: &Room, t: (i32, i32)| t.0 >= r.x && t.0 < r.x + r.w && t.1 >= r.y && t.1 < r.y + r.h;
        let ok: Vec<Room> = self
            .map
            .rooms
            .iter()
            .skip(1)
            .copied()
            .filter(|r| !inside(r, npc_t) && !inside(r, stairs_t))
            .filter(|r| {
                let c = r.center();
                field[self.map.idx(c.0, c.1)] != UNREACHED
            })
            .collect();
        let room = if ok.is_empty() { self.map.rooms[self.map.rooms.len().min(2) - 1] } else { ok[gen_range(0, ok.len())] };
        let mut t = self.map.random_floor_in(&room);
        if t == npc_t || t == stairs_t {
            t = room.center();
        }
        let pos = tile_center(t);
        self.prop = Some(Prop { pos, sprite, glow, done: false });
        pos
    }

    /// The warden arena: `kinds` fight one after another.
    pub fn arena(tier: Tier, kinds: &[BossKind]) -> World {
        // pacing reaches the arena only as warden health (monster speed and count are floor knobs)
        let tier = tier.with_pace(Pace { monster_speed_pct: 100, monster_count_delta: 0, ..tier.pace });
        let map = Map::arena();
        let start = tile_center((4, map.h / 2));
        let mut w = World::base(map, tier, true, start);
        w.cam = vec2(ARENA_W as f32 * TILE * 0.5, ARENA_H as f32 * TILE * 0.5);
        w.boss_queue = kinds.iter().rev().copied().collect();
        w.bosses_total = kinds.len();
        w.boss_delay = 0.4;
        w
    }

    fn populate(&mut self) {
        let tier = self.tier;
        let count = tier.paced_enemy_count();
        let rooms = self.map.rooms.clone();
        let npc_t = tile_of(self.npc);
        let pick_room = |n: usize| rooms[gen_range(1, n.max(2)).min(n - 1)];
        let mut placed = 0;
        let mut tries = 0;
        while placed < count && tries < 400 {
            tries += 1;
            let r = pick_room(rooms.len());
            let t = self.map.random_floor_in(&r);
            let pos = tile_center(t);
            if pos.distance(self.player.pos) < 6.0 * TILE || (t.0 - npc_t.0).abs() + (t.1 - npc_t.1).abs() < 3 {
                continue;
            }
            let kind = roll_enemy(tier.depth);
            self.enemies.push(Enemy::new(kind, pos, tier));
            placed += 1;
        }
        // two coffees somewhere on the floor
        for _ in 0..2 {
            let t = self.map.random_floor_in(&pick_room(rooms.len()));
            self.pickups.push(Pickup { pos: tile_center(t), phase: 0.0 });
        }
    }

    fn burst(&mut self, at: Vec2, color: Color, n: usize, speed: f32) {
        for _ in 0..n {
            let a = gen_range(0.0, std::f32::consts::TAU);
            let s = gen_range(0.3, 1.0) * speed;
            let life = gen_range(0.25, 0.6);
            self.particles.push(Particle { pos: at, vel: vec2(a.cos(), a.sin()) * s, life, max: life, color, size: gen_range(3.0, 6.0) });
        }
    }

    fn float_text(&mut self, at: Vec2, text: String, color: Color) {
        self.texts.push(FloatText { pos: at, text, life: 0.9, color });
    }

    // ------------------------------------------------------------ update

    /// Advance the simulation by `dt` in fixed steps.
    pub fn update(&mut self, dt: f32, ctl: Control, power: Power) {
        self.acc = (self.acc + dt).min(0.1);
        let mut power = power;
        while self.acc >= STEP {
            self.acc -= STEP;
            self.step(STEP, ctl, power);
            if self.events.contains(&WorldEvent::Healed) {
                power.can_heal = false; // one coffee per frame is plenty
            }
        }
        // the camera follows the player (the arena is static)
        if !self.arena {
            let view = vec2(VW, VH - 56.0);
            let world = vec2(self.map.w as f32, self.map.h as f32) * TILE;
            self.cam = self.cam.lerp(self.player.pos, (dt * 8.0).min(1.0));
            self.cam.x = self.cam.x.clamp(view.x * 0.5, (world.x - view.x * 0.5).max(view.x * 0.5));
            self.cam.y = self.cam.y.clamp(view.y * 0.5, (world.y - view.y * 0.5).max(view.y * 0.5));
        }
    }

    /// Snap the camera onto the player (new floor / back from the arena).
    pub fn snap_camera(&mut self) {
        if self.arena {
            return;
        }
        let view = vec2(VW, VH - 56.0);
        let world = vec2(self.map.w as f32, self.map.h as f32) * TILE;
        self.cam = self.player.pos;
        self.cam.x = self.cam.x.clamp(view.x * 0.5, (world.x - view.x * 0.5).max(view.x * 0.5));
        self.cam.y = self.cam.y.clamp(view.y * 0.5, (world.y - view.y * 0.5).max(view.y * 0.5));
    }

    fn step(&mut self, dt: f32, ctl: Control, power: Power) {
        self.time += dt;
        self.shake = (self.shake - dt * 2.0).max(0.0);
        self.update_player(dt, ctl, power);
        let t = tile_of(self.player.pos);
        self.map.update_flow(t);
        if !self.arena {
            self.map.compute_fov(t.0, t.1, VIEW_RADIUS);
        }
        self.update_enemies(dt);
        self.update_boss(dt);
        self.update_projectiles(dt);
        self.update_darts(dt, power);
        self.update_pickups(dt, power);
        for p in &mut self.particles {
            p.pos += p.vel * dt;
            p.vel *= 1.0 - 4.0 * dt;
            p.life -= dt;
        }
        self.particles.retain(|p| p.life > 0.0);
        for t in &mut self.texts {
            t.pos.y -= 40.0 * dt;
            t.life -= dt;
        }
        self.texts.retain(|t| t.life > 0.0);
    }

    fn update_player(&mut self, dt: f32, ctl: Control, power: Power) {
        let aim = self.aim_dir(ctl);
        let p = &mut self.player;
        p.attack_cd -= dt;
        p.swing_t -= dt;
        p.invuln -= dt;
        p.dodge_cd -= dt;
        let dir = ctl.mv.clamp_length_max(1.0);
        // start a roll: a burst of speed you can't be hit during
        if ctl.dodge && p.dodge_cd <= 0.0 && p.dodge_t <= 0.0 {
            p.dodge_t = DODGE_TIME;
            p.dodge_cd = DODGE_TIME + DODGE_CD;
            p.dodge_dir = if dir != Vec2::ZERO { dir.normalize() } else { p.facing };
            p.invuln = p.invuln.max(DODGE_IFRAMES);
            let at = p.pos;
            self.sounds.push(Sfx::Swing);
            self.burst(at, Color::new(0.85, 0.8, 0.7, 0.8), 8, 120.0);
        }
        let p = &mut self.player;
        let rolling = p.dodge_t > 0.0;
        let delta = if rolling {
            p.dodge_t -= dt;
            p.walk += dt * 2.0;
            (p.dodge_dir * PLAYER_SPEED * DODGE_SPEED + p.knock) * dt
        } else {
            if dir != Vec2::ZERO {
                p.walk += dt;
            }
            (dir * PLAYER_SPEED + p.knock) * dt
        };
        // you face where you aim (twin-stick); rolling faces the roll
        p.facing = if rolling { p.dodge_dir } else { aim };
        p.swing_dir = aim;
        p.knock *= 1.0 - 9.0 * dt;
        let mut pos = p.pos;
        move_and_collide(&self.map, &mut pos, delta, PLAYER_RADIUS);
        self.player.pos = pos;
        if ctl.attack && !rolling && self.player.attack_cd <= 0.0 {
            self.fire(aim, power);
        }
    }

    /// Where the blaster points: the mouse, else aim assist (the nearest monster
    /// or hittable warden in sight and range), else the way you're walking or facing.
    fn aim_dir(&self, ctl: Control) -> Vec2 {
        let p = &self.player;
        if let Some(a) = ctl.aim {
            return (a - p.pos).normalize_or(p.facing);
        }
        let reach = DART_RANGE + 20.0;
        let boss = self.boss.iter().filter(|b| b.hittable()).map(|b| b.pos);
        let target = self
            .enemies
            .iter()
            .filter(|e| self.map.is_visible(e.pos) || self.arena)
            .map(|e| e.pos)
            .chain(boss)
            .filter(|t| t.distance(p.pos) < reach && self.map.line_clear(p.pos, *t))
            .min_by(|a, b| a.distance(p.pos).total_cmp(&b.distance(p.pos)));
        match target {
            Some(t) => (t - p.pos).normalize_or(p.facing),
            None if ctl.mv != Vec2::ZERO => ctl.mv.normalize_or(p.facing),
            None => p.facing,
        }
    }

    /// Fire one foam dart toward `aim`.
    fn fire(&mut self, aim: Vec2, power: Power) {
        self.player.attack_cd = power.cooldown() * DART_RATE;
        self.player.swing_t = MUZZLE_TIME;
        let a = aim.y.atan2(aim.x) + gen_range(-DART_SPREAD, DART_SPREAD);
        let dir = vec2(a.cos(), a.sin());
        self.darts.push(Dart { pos: self.player.pos + dir * 18.0, vel: dir * DART_SPEED, life: DART_RANGE / DART_SPEED });
        self.sounds.push(Sfx::Pew);
    }

    /// Move the darts; a dart stops at the first wall, monster or warden it touches.
    fn update_darts(&mut self, dt: f32, power: Power) {
        if self.darts.is_empty() {
            return;
        }
        let base = power.damage();
        let mut darts = std::mem::take(&mut self.darts);
        let mut texts = Vec::new();
        let mut pops = Vec::new();
        darts.retain_mut(|d| {
            d.pos += d.vel * dt;
            d.life -= dt;
            if d.life <= 0.0 || self.map.solid_at(d.pos) {
                pops.push(d.pos);
                return false;
            }
            let crit = gen_range(0, 100) < 12;
            let dmg = base * DART_DAMAGE * gen_range(0.85, 1.15) * if crit { 2.0 } else { 1.0 };
            let push = d.vel.normalize_or_zero();
            if let Some(e) = self.enemies.iter_mut().find(|e| e.pos.distance(d.pos) < e.radius + DART_RADIUS) {
                e.hp -= dmg;
                e.flash = 0.12;
                e.aggro = true;
                e.last_seen = 0.0;
                e.windup = 0.0; // a hit interrupts a ranged attack
                e.knock = push * if e.kind == EnemyKind::FinalsBrute { 60.0 } else { 140.0 };
                texts.push((e.pos, dmg, crit));
                return false;
            }
            if let Some(b) = &mut self.boss {
                if b.hittable() && b.pos.distance(d.pos) < b.radius + DART_RADIUS {
                    b.hp -= dmg;
                    b.flash = 0.1;
                    texts.push((b.pos - vec2(0.0, 30.0), dmg, crit));
                    return false;
                }
            }
            true
        });
        self.darts = darts;
        for at in pops {
            self.burst(at, Color::from_rgba(255, 140, 40, 255), 3, 70.0);
        }
        let hit = !texts.is_empty();
        for (pos, dmg, crit) in texts {
            let txt = if crit { format!("{}!", dmg as i32) } else { format!("{}", dmg as i32) };
            self.float_text(pos - vec2(0.0, 18.0), txt, if crit { KSU_GOLD } else { WHITE });
            self.burst(pos, Color::from_rgba(255, 160, 60, 230), 5, 140.0);
        }
        if hit {
            self.shake = self.shake.max(0.06);
            self.sounds.push(Sfx::Hit);
        }
        self.resolve_deaths();
    }

    /// Remove beaten monsters and wardens (bursts, drops, events).
    fn resolve_deaths(&mut self) {
        let mut i = 0;
        while i < self.enemies.len() {
            if self.enemies[i].hp <= 0.0 {
                let e = self.enemies.swap_remove(i);
                self.burst(e.pos, e.kind.color(), 22, 230.0);
                self.sounds.push(Sfx::Death);
                self.events.push(WorldEvent::EnemyKilled);
                self.float_text(e.pos - vec2(0.0, 34.0), e.kind.name().to_string(), e.kind.color());
                if gen_range(0, 100) < 14 {
                    self.pickups.push(Pickup { pos: e.pos, phase: 0.0 });
                }
            } else {
                i += 1;
            }
        }
        if self.boss.as_ref().is_some_and(|b| b.hp <= 0.0) {
            if let Some(b) = self.boss.take() {
                self.burst(b.pos, b.kind.color(), 70, 340.0);
                self.burst(b.pos, KSU_GOLD, 40, 260.0);
                self.shake = 0.6;
                self.sounds.push(Sfx::LevelUp);
                self.events.push(WorldEvent::BossDown(b.kind));
                self.projectiles.clear();
                self.darts.clear();
                self.enemies.clear(); // its summons go with it
                if !self.boss_queue.is_empty() {
                    // a coffee between wardens keeps it fair
                    self.pickups.push(Pickup { pos: tile_center((ARENA_W / 2, ARENA_H / 2)), phase: 0.0 });
                    self.boss_delay = 1.6;
                }
            }
        }
    }

    fn hurt_player(&mut self, from_dir: Vec2) {
        if self.player.invuln > 0.0 {
            return;
        }
        self.player.invuln = INVULN;
        self.player.knock = from_dir.normalize_or(vec2(1.0, 0.0)) * 420.0;
        self.shake = self.shake.max(0.35);
        let p = self.player.pos;
        self.burst(p, Color::new(0.9, 0.15, 0.15, 1.0), 12, 180.0);
        if self.god {
            return;
        }
        self.float_text(p - vec2(0.0, 24.0), "-1 Health".into(), RISKY);
        self.sounds.push(Sfx::Hurt);
        self.events.push(WorldEvent::PlayerHit);
    }

    fn update_enemies(&mut self, dt: f32) {
        let ppos = self.player.pos;
        let n = self.enemies.len();
        let mut shots: Vec<Projectile> = Vec::new();
        let mut contact: Option<Vec2> = None;
        for i in 0..n {
            let mut push = Vec2::ZERO;
            let (pi, ri) = (self.enemies[i].pos, self.enemies[i].radius);
            for (j, other) in self.enemies.iter().enumerate() {
                if i != j {
                    let d = pi - other.pos;
                    let min = ri + other.radius;
                    let l2 = d.length_squared();
                    if l2 < min * min && l2 > 0.0001 {
                        let l = l2.sqrt();
                        push += d / l * (min - l);
                    }
                }
            }
            let flow_dir = self.map.flow_dir(pi);
            let sees = self.map.is_visible(pi);
            let clear = sees && self.map.line_clear(pi, ppos);
            let e = &mut self.enemies[i];
            e.flash -= dt;
            e.anim += dt;
            e.attack_cd -= dt;
            let to_p = ppos - e.pos;
            let dist = to_p.length();
            let dir_p = if dist > 0.1 { to_p / dist } else { Vec2::ZERO };
            let perp = vec2(-dir_p.y, dir_p.x) * e.flank;
            let aggro_range = if e.kind.ranged() { 9.0 * TILE } else { 7.0 * TILE };
            if sees && dist < aggro_range {
                e.aggro = true;
                e.last_seen = 0.0;
            } else {
                e.last_seen += dt;
                if e.last_seen > 5.0 {
                    e.aggro = false;
                }
            }
            let mut fire = false;
            let mut desired = Vec2::ZERO;
            let mut speed = e.speed;
            if e.windup > 0.0 {
                e.windup -= dt;
                e.facing = dir_p;
                if e.windup <= 0.0 {
                    fire = clear;
                    e.attack_cd = if e.kind == EnemyKind::ParkingPatrol { 1.9 } else { 2.5 };
                }
            } else if e.aggro {
                if e.kind.ranged() {
                    if clear {
                        if dist < 3.5 * TILE {
                            desired = (-dir_p + perp * 0.6).normalize_or_zero();
                        } else if dist > 6.5 * TILE {
                            desired = (dir_p + perp * 0.3).normalize_or_zero();
                        } else {
                            desired = perp;
                            speed *= 0.5;
                        }
                        if e.attack_cd <= 0.0 && dist < 9.0 * TILE {
                            e.windup = if e.kind == EnemyKind::ParkingPatrol { 0.55 } else { 0.7 };
                        }
                    } else {
                        desired = flow_dir;
                    }
                } else if clear && dist < 4.5 * TILE {
                    // the shuttle doesn't flank: it's late, so it comes straight at you
                    let flank = if e.kind == EnemyKind::ShuttleRusher { 0.0 } else { 0.7 };
                    desired = if dist > 2.0 * TILE { (dir_p + perp * flank).normalize_or_zero() } else { dir_p };
                } else {
                    desired = flow_dir;
                }
                if e.kind.owl() {
                    let s = (e.anim * 7.0).sin() * 0.6;
                    desired = (desired + vec2(-desired.y, desired.x) * s).normalize_or_zero();
                }
                if desired != Vec2::ZERO {
                    e.facing = desired;
                }
            } else {
                e.wander_t -= dt;
                if e.wander_t <= 0.0 {
                    e.wander_t = gen_range(0.8, 2.2);
                    e.wander = if gen_range(0, 3) == 0 {
                        Vec2::ZERO
                    } else {
                        let a = gen_range(0.0, std::f32::consts::TAU);
                        vec2(a.cos(), a.sin())
                    };
                }
                desired = e.wander;
                speed *= 0.35;
            }
            let mut vel = desired * speed;
            if e.kind == EnemyKind::MysteryMeat {
                vel *= 0.55 + 0.75 * (e.anim * 5.0).sin().max(0.0);
            }
            let delta = (vel + e.knock) * dt + push * 0.5;
            e.knock *= 1.0 - 10.0 * dt;
            let r = e.radius;
            let mut pos = e.pos;
            move_and_collide(&self.map, &mut pos, delta, r * 0.8);
            e.pos = pos;
            if fire {
                let (spd, radius, kind) = match e.kind {
                    EnemyKind::ParkingPatrol => (330.0, 6.0, ProjKind::Ticket),
                    _ => (200.0, 9.0, ProjKind::Orb),
                };
                let d = (ppos - e.pos).normalize_or(vec2(1.0, 0.0));
                shots.push(Projectile { pos: e.pos + d * 14.0, vel: d * spd, radius, life: 4.0, kind });
            }
            if !e.kind.ranged() && dist < r + PLAYER_RADIUS + 2.0 {
                contact = Some(to_p);
            }
        }
        if !shots.is_empty() {
            self.sounds.push(Sfx::Shoot);
            self.projectiles.extend(shots.into_iter().map(slow_shot));
        }
        if let Some(d) = contact {
            if self.player.invuln <= 0.0 {
                // whoever bit you bounces off, so one monster can't chain hits
                for e in &mut self.enemies {
                    if !e.kind.ranged() && e.pos.distance(ppos) < e.radius + PLAYER_RADIUS + 4.0 {
                        e.knock = (e.pos - ppos).normalize_or(vec2(1.0, 0.0)) * 320.0;
                    }
                }
            }
            #[cfg(test)]
            if self.player.invuln <= 0.0 {
                tally(1);
            }
            self.hurt_player(d);
        }
    }

    fn spawn_next_boss(&mut self, dt: f32) {
        if self.boss.is_some() || self.boss_queue.is_empty() {
            return;
        }
        self.boss_delay -= dt;
        if self.boss_delay > 0.0 {
            return;
        }
        let Some(kind) = self.boss_queue.pop() else { return };
        let pos = tile_center((ARENA_W - 6, ARENA_H / 2));
        self.boss = Some(Boss::new(kind, pos, self.tier));
        self.burst(pos, kind.color(), 50, 300.0);
        self.shake = 0.4;
        self.sounds.push(Sfx::Stairs);
        self.events.push(WorldEvent::BossEntered(kind));
    }

    fn update_boss(&mut self, dt: f32) {
        self.spawn_next_boss(dt);
        let ppos = self.player.pos;
        let minions = self.enemies.len();
        let Some(b) = &mut self.boss else { return };
        let mut shots: Vec<Projectile> = Vec::new();
        let mut summon = 0;
        let mut shook = 0.0f32;
        b.flash -= dt;
        b.anim += dt;
        if b.entering > 0.0 {
            b.entering -= dt;
            return;
        }
        let to_p = ppos - b.pos;
        let dist = to_p.length();
        let dir_p = to_p.normalize_or(vec2(-1.0, 0.0));
        let rage = b.enraged();
        let mut vel = Vec2::ZERO;
        b.t -= dt;
        let ring = |shots: &mut Vec<Projectile>, at: Vec2, n: usize, off: f32, spd: f32, kind: ProjKind| {
            for k in 0..n {
                let a = off + k as f32 / n as f32 * std::f32::consts::TAU;
                shots.push(Projectile { pos: at, vel: vec2(a.cos(), a.sin()) * spd, radius: 8.0, life: 5.0, kind });
            }
        };
        let fan = |shots: &mut Vec<Projectile>, at: Vec2, dir: Vec2, n: usize, spread: f32, spd: f32, kind: ProjKind| {
            let base = dir.y.atan2(dir.x);
            for k in 0..n {
                let a = base + (k as f32 - (n as f32 - 1.0) * 0.5) * spread;
                shots.push(Projectile { pos: at + dir * 30.0, vel: vec2(a.cos(), a.sin()) * spd, radius: 8.0, life: 4.0, kind });
            }
        };
        match (b.kind, b.state) {
            // ---- Phish Lord: red line tell, then a charge; enraged: hooks after each charge
            (BossKind::TowTyrant, BossState::Walk) => {
                vel = dir_p * b.kind.speed();
                if b.t <= 0.0 {
                    b.state = BossState::Tell;
                    b.t = if rage { 0.8 } else { 1.0 };
                }
            }
            (BossKind::TowTyrant, BossState::Tell) => {
                b.dir = dir_p; // the red line tracks you until it locks
                if b.t <= 0.0 {
                    b.state = BossState::Act;
                    b.t = 0.5;
                }
            }
            (BossKind::TowTyrant, BossState::Act) => {
                vel = b.dir * 650.0 * BOSS_SPEED_SCALE;
                if b.t <= 0.0 {
                    if rage {
                        fan(&mut shots, b.pos, dir_p, 5, 0.28, 300.0, ProjKind::Hook);
                    }
                    b.state = BossState::Walk;
                    b.t = if rage { 1.5 } else { 2.2 };
                }
            }
            // ---- Overdue Golem: growing ring tell, then a ring of pages; enraged: two rings
            (BossKind::FinalsGolem, BossState::Walk) => {
                vel = dir_p * b.kind.speed();
                if b.t <= 0.0 {
                    b.state = BossState::Tell;
                    b.t = 0.8;
                }
            }
            (BossKind::FinalsGolem, BossState::Tell) => {
                if b.t <= 0.0 {
                    b.state = BossState::Act;
                    b.volleys = if rage { 2 } else { 1 };
                    b.shot_t = 0.0;
                    b.t = 0.6;
                }
            }
            (BossKind::FinalsGolem, BossState::Act) => {
                b.shot_t -= dt;
                if b.volleys > 0 && b.shot_t <= 0.0 {
                    let n = if rage { 20 } else { 14 };
                    ring(&mut shots, b.pos, n, b.spin, 210.0, ProjKind::Page);
                    b.spin += 0.5 / n as f32 * std::f32::consts::TAU;
                    b.volleys -= 1;
                    b.shot_t = 0.35;
                    shook = 0.3;
                }
                if b.t <= 0.0 && b.volleys == 0 {
                    b.state = BossState::Walk;
                    b.t = if rage { 1.8 } else { 2.4 };
                }
            }
            // ---- Spam Queen: hovers at range, flashes, sprays envelopes, calls bats
            (BossKind::OwlQueen, BossState::Walk) => {
                let perp = vec2(-dir_p.y, dir_p.x);
                // right after a volley she preens for a moment: the window to close in
                let resting = b.volleys > 0 && b.t > if rage { 0.6 } else { 0.8 };
                let want = if resting {
                    Vec2::ZERO
                } else if dist < 3.5 * TILE {
                    -dir_p
                } else if dist > 6.0 * TILE {
                    dir_p
                } else {
                    Vec2::ZERO
                };
                vel = if resting { Vec2::ZERO } else { (want + perp * 0.8).normalize_or_zero() * b.kind.speed() };
                if b.t <= 0.0 {
                    b.state = BossState::Tell;
                    b.t = 0.6;
                }
            }
            (BossKind::OwlQueen, BossState::Tell) => {
                if b.t <= 0.0 {
                    b.state = BossState::Act;
                    b.t = 0.2;
                }
            }
            (BossKind::OwlQueen, BossState::Act) => {
                if b.t <= 0.0 {
                    fan(&mut shots, b.pos, dir_p, if rage { 6 } else { 5 }, 0.3, 200.0, ProjKind::Envelope);
                    b.volleys += 1;
                    if b.volleys % 4 == 0 && minions < 2 {
                        summon = 2;
                    }
                    b.state = BossState::Walk;
                    b.t = if rage { 1.3 } else { 1.8 };
                }
            }
            // ---- Ransomware Lich: fades out, reappears far away, spins padlock bolts
            (BossKind::LibraryLich, BossState::Walk) => {
                b.alpha = (b.alpha + dt * 2.5).min(1.0);
                vel = dir_p * b.kind.speed();
                if b.t <= 0.0 {
                    b.state = BossState::Tell;
                    b.t = 0.9;
                    b.teleport_to = None;
                }
            }
            (BossKind::LibraryLich, BossState::Tell) => {
                if b.t > 0.45 {
                    b.alpha = (b.alpha - dt * 2.4).max(0.0);
                } else {
                    if b.teleport_to.is_none() {
                        let mut to = b.pos;
                        for _ in 0..30 {
                            let c = tile_center((gen_range(3, ARENA_W - 3), gen_range(2, ARENA_H - 2)));
                            if !self.map.solid_at(c) && c.distance(ppos) > 6.0 * TILE {
                                to = c;
                                break;
                            }
                        }
                        b.teleport_to = Some(to);
                        b.pos = to;
                    }
                    b.alpha = (b.alpha + dt * 2.4).min(1.0);
                }
                if b.t <= 0.0 {
                    b.alpha = 1.0;
                    b.state = BossState::Act;
                    b.t = 1.0;
                    b.shot_t = 0.35;
                    // reappears with an aimed volley, then spins
                    fan(&mut shots, b.pos, dir_p, 3, 0.18, 240.0, ProjKind::Padlock);
                }
            }
            (BossKind::LibraryLich, BossState::Act) => {
                b.shot_t -= dt;
                if b.shot_t <= 0.0 {
                    ring(&mut shots, b.pos, if rage { 3 } else { 2 }, b.spin, 170.0, ProjKind::Padlock);
                    b.spin += 0.3;
                    b.shot_t = 0.16;
                }
                if b.t <= 0.0 {
                    b.state = BossState::Walk;
                    b.t = if rage { 1.4 } else { 2.0 };
                }
            }
        }
        let want = b.pos + vel * dt;
        let mut pos = b.pos;
        move_and_collide(&self.map, &mut pos, vel * dt, b.radius * 0.7);
        if b.kind == BossKind::TowTyrant && b.state == BossState::Act && pos.distance(want) > 1.0 {
            // charged into a wall or pillar: dazed for a moment (a free hit window)
            b.state = BossState::Walk;
            b.t = 1.2;
            shook = 0.35;
        }
        b.pos = pos;
        let touching = b.hittable() && b.pos.distance(ppos) < b.radius + PLAYER_RADIUS;
        // only a charge hurts on contact; otherwise a warden just shoves you back,
        // so standing in sword range is a choice, not a penalty
        let ramming = b.kind == BossKind::TowTyrant && b.state == BossState::Act;
        let bpos = b.pos;
        if !shots.is_empty() {
            self.sounds.push(Sfx::Shoot);
            self.projectiles.extend(shots.into_iter().map(slow_shot));
        }
        if shook > 0.0 {
            self.shake = self.shake.max(shook);
        }
        for k in 0..summon {
            let off = vec2(if k == 0 { -50.0 } else { 50.0 }, 30.0);
            let at = if self.map.solid_at(bpos + off) { bpos } else { bpos + off };
            let mut e = Enemy::new(EnemyKind::SpamOwl, at, self.tier);
            e.aggro = true;
            self.enemies.push(e);
        }
        if touching && !ramming {
            self.player.knock = (ppos - bpos).normalize_or(vec2(-1.0, 0.0)) * 360.0;
        } else if touching {
            #[cfg(test)]
            if self.player.invuln <= 0.0 {
                tally(0);
            }
            self.hurt_player(ppos - bpos);
        }
    }

    fn update_projectiles(&mut self, dt: f32) {
        let ppos = self.player.pos;
        let mut hit: Option<Vec2> = None;
        let mut pops = Vec::new();
        let map = &self.map;
        self.projectiles.retain_mut(|p| {
            p.pos += p.vel * dt;
            p.life -= dt;
            let hit_player = p.pos.distance(ppos) < p.radius + PLAYER_RADIUS - 7.0; // a forgiving hitbox: grazes don't count
            if hit_player {
                hit = Some(p.vel);
            }
            let gone = hit_player || p.life <= 0.0 || map.solid_at(p.pos);
            if gone {
                pops.push(p.pos);
            }
            !gone
        });
        for at in pops {
            self.burst(at, Color::new(0.9, 0.85, 0.75, 1.0), 5, 110.0);
        }
        if let Some(v) = hit {
            #[cfg(test)]
            if self.player.invuln <= 0.0 {
                tally(2);
            }
            self.hurt_player(v);
        }
    }

    fn update_pickups(&mut self, dt: f32, power: Power) {
        let ppos = self.player.pos;
        let mut drank = None;
        for (i, it) in self.pickups.iter_mut().enumerate() {
            it.phase += dt * 4.0;
            if power.can_heal && drank.is_none() && it.pos.distance(ppos) < PICKUP_RANGE {
                drank = Some(i);
            }
        }
        if let Some(i) = drank {
            let it = self.pickups.swap_remove(i);
            self.burst(it.pos, KSU_GOLD, 16, 160.0);
            self.float_text(it.pos - vec2(0.0, 20.0), "+1 Health (coffee)".into(), SAFE);
            self.sounds.push(Sfx::Potion);
            self.events.push(WorldEvent::Healed);
        }
    }

    // ------------------------------------------------------------ automation

    /// Hands-free control for CC_AUTO play-tests and screenshots: walk to
    /// `target` along a BFS path, shoot anything in range, circle the warden at
    /// blaster range.
    pub fn auto_control(&mut self, target: Option<Vec2>) -> Control {
        let ppos = self.player.pos;
        let near_enemy = self
            .enemies
            .iter()
            .filter(|e| self.map.line_clear(ppos, e.pos))
            .map(|e| e.pos)
            .min_by(|a, b| a.distance(ppos).total_cmp(&b.distance(ppos)));
        let mut ctl = Control::default();
        if let Some(b) = &self.boss {
            let to = b.pos - ppos;
            let d = to.length();
            let perp = vec2(-to.y, to.x).normalize_or_zero();
            // keep at blaster range and circle
            let dir = to.normalize_or_zero();
            ctl.mv = if d > 5.5 * TILE {
                dir
            } else if d < 3.0 * TILE {
                (-dir + perp * 0.5).normalize_or_zero()
            } else {
                perp * 0.9
            };
            ctl.attack = d < DART_RANGE - 10.0 && self.map.line_clear(ppos, b.pos);
            ctl.aim = Some(b.pos);
            // dodge what a player would: the Phish Lord's red line, and bolts about to land
            if b.kind == BossKind::TowTyrant && b.state != BossState::Walk {
                // back off while the line tracks you, then step out of it when it locks
                let side = vec2(-b.dir.y, b.dir.x);
                let s = if side.dot(ppos - b.pos) >= 0.0 { side } else { -side };
                ctl.mv = if b.state == BossState::Tell && b.t > 0.25 { (-to.normalize_or_zero() + s * 0.3).normalize_or_zero() } else { s };
                ctl.attack = false;
            } else if b.kind == BossKind::FinalsGolem && b.state == BossState::Tell {
                // the growing ring means pages are coming: get room to slip between them
                ctl.mv = -to.normalize_or_zero();
                ctl.attack = false;
            } else if let Some(pr) = self
                .projectiles
                .iter()
                .filter(|p| {
                    let rel = ppos - p.pos;
                    let v = p.vel.normalize_or_zero();
                    let along = rel.dot(v);
                    // anything that lands within ~0.7 s and would clip you
                    along > 0.0 && along < p.vel.length() * 0.7 && (rel - v * along).length() < 26.0
                })
                .min_by(|a, b| a.pos.distance(ppos).total_cmp(&b.pos.distance(ppos)))
            {
                let v = pr.vel.normalize_or_zero();
                let side = vec2(-v.y, v.x);
                let s = if side.dot(ppos - pr.pos) >= 0.0 { side } else { -side };
                ctl.mv = s;
            }
            if let Some(e) = near_enemy {
                if e.distance(ppos) < 3.0 * TILE {
                    ctl.aim = Some(e);
                    ctl.attack = true;
                }
            }
            return ctl;
        }
        if let Some(e) = near_enemy {
            let d = e.distance(ppos);
            if d < 5.0 * TILE {
                // back off from anything close, otherwise stand and shoot
                ctl.mv = if d < 1.6 * TILE { (ppos - e).normalize_or_zero() * 0.6 } else { Vec2::ZERO };
                ctl.attack = true;
                ctl.aim = Some(e);
                return ctl;
            }
        }
        if let Some(t) = target {
            let tt = tile_of(t);
            if tt != self.auto_target {
                self.auto_target = tt;
                let mut f = std::mem::take(&mut self.auto_field);
                let mut q = std::mem::take(&mut self.auto_queue);
                self.map.bfs(tt, u16::MAX - 1, &mut f, &mut q);
                self.auto_field = f;
                self.auto_queue = q;
            }
            let d = self.map.downhill(&self.auto_field, ppos);
            ctl.mv = if d == Vec2::ZERO { (t - ppos).normalize_or_zero() } else { d };
        }
        ctl
    }

    // ------------------------------------------------------------ queries

    /// Which warden is up (1-based), for the "WARDEN 1 OF 2" bar.
    pub fn boss_number(&self) -> usize {
        self.bosses_total - self.boss_queue.len()
    }

    #[cfg(test)]
    pub fn arena_cleared(&self) -> bool {
        self.arena && self.boss.is_none() && self.boss_queue.is_empty()
    }

    /// Screen offset: world point + offset = virtual-canvas point.
    pub fn offset(&self) -> Vec2 {
        let shake = if self.shake > 0.0 { vec2(gen_range(-1.0, 1.0), gen_range(-1.0, 1.0)) * self.shake * 10.0 } else { Vec2::ZERO };
        (vec2(VW * 0.5, 56.0 + (VH - 56.0) * 0.5) - self.cam + shake).round()
    }

    // ------------------------------------------------------------ drawing

    /// Raster the map texture now (it's the slow part of a new floor), so the
    /// caller can do it while the screen is black instead of on the first draw.
    pub fn prewarm(&mut self) {
        if self.tex.is_none() {
            let m = &self.map;
            self.tex = Some(dungeon_texture(m.w, m.h, TILE as u16, self.seed, |x, y| u8::from(m.get(x, y) != Tile::Wall)));
        }
    }

    /// Draw the floor (or arena). `npc` is the floor character's sprite and
    /// whether it's been met (dimmed then); the Keeper stands on the stairs
    /// until `stairs_open`.
    pub fn draw(&mut self, t: f32, npc: Option<(&'static [&'static str; 16], bool)>, stairs_open: bool) {
        let off = self.offset();
        clear_background(Color::from_rgba(6, 5, 9, 255));
        self.prewarm();
        if let Some(tex) = &self.tex {
            draw_texture(tex, off.x, off.y, WHITE);
        }
        if !self.arena && self.map.is_seen(self.stairs) {
            let s = self.stairs + off;
            for k in 0..4 {
                let kk = k as f32;
                draw_rectangle(s.x - 16.0 + kk * 3.0, s.y - 14.0 + kk * 8.0, 32.0 - kk * 6.0, 6.0, Color::new(0.85 - kk * 0.18, 0.7 - kk * 0.15, 0.35, 1.0));
            }
            if stairs_open {
                let g = 0.5 + 0.5 * (t * 3.0).sin();
                glow(s.x, s.y, 60.0, KSU_GOLD, 1.0 + g);
                draw_rectangle_lines(s.x - 19.0, s.y - 19.0, 38.0, 38.0, 3.0, Color::new(1.0, 0.85, 0.3, 0.4 + 0.5 * g));
            }
        }
        for it in &self.pickups {
            if self.map.is_visible(it.pos) {
                let p = it.pos + off;
                glow(p.x, p.y, 40.0, Color::new(1.0, 0.8, 0.4, 1.0), 1.0);
                draw_sprite_centered(&COFFEE, p.x, p.y + it.phase.sin() * 3.0, 2.5, false, WHITE);
            }
        }
        // a side prop (main.rs mini-games)
        if let Some(pr) = &self.prop {
            if !self.arena && self.map.is_seen(pr.pos) {
                let p = pr.pos + off;
                if pr.done {
                    draw_sprite_centered(pr.sprite, p.x, p.y, 2.5, false, Color::new(0.5, 0.5, 0.55, 0.7));
                } else {
                    let g = 0.5 + 0.5 * (t * 4.0).sin();
                    glow(p.x, p.y, 54.0, pr.glow, 1.0 + g);
                    draw_sprite_centered(pr.sprite, p.x, p.y + (t * 3.0).sin() * 3.0, 3.0, false, WHITE);
                }
            }
        }
        // the floor's character
        if let Some((sprite, met)) = npc {
            if self.map.is_seen(self.npc) {
                let p = self.npc + off;
                let bob = (t * 2.2).sin() * 4.0;
                if !met {
                    glow(p.x, p.y, 90.0, Color::new(0.6, 0.4, 1.0, 1.0), 1.4);
                }
                draw_ellipse(p.x, p.y + 30.0, 30.0, 8.0, 0.0, Color::new(0.0, 0.0, 0.0, 0.35));
                let tint = if met { Color::new(0.55, 0.55, 0.6, 0.8) } else { WHITE };
                draw_sprite_centered(sprite, p.x, p.y + bob, 4.0, true, tint);
            }
        }
        // the Floor Keeper
        if !self.arena && self.map.is_seen(self.keeper) {
            let k = if stairs_open { self.keeper_aside } else { self.keeper } + off;
            let bob = (t * 1.7).sin() * 3.0;
            glow(k.x + 22.0, k.y, 80.0, Color::new(1.0, 0.75, 0.3, 1.0), 1.3);
            draw_ellipse(k.x, k.y + 30.0, 28.0, 8.0, 0.0, Color::new(0.0, 0.0, 0.0, 0.35));
            draw_sprite_centered(&KEEPER, k.x, k.y - 6.0 + bob, 4.5, false, WHITE);
        }
        for e in &self.enemies {
            if !self.map.is_visible(e.pos) {
                continue;
            }
            let p = e.pos + off;
            let bob = match e.kind {
                EnemyKind::SpamOwl | EnemyKind::NightOwl => (e.anim * 9.0).sin() * 5.0,
                EnemyKind::MysteryMeat => -(e.anim * 5.0).sin().max(0.0) * 6.0,
                _ => (e.anim * 6.0).sin() * 1.5,
            };
            draw_ellipse(p.x, p.y + e.radius, e.radius, 5.0, 0.0, Color::new(0.0, 0.0, 0.0, 0.35));
            if e.windup > 0.0 {
                glow(p.x, p.y - 4.0, 44.0, e.kind.color(), 3.5); // ranged tell: charging glow
            }
            let tint = if e.flash > 0.0 { Color::new(1.0, 0.4, 0.4, 1.0) } else { WHITE };
            draw_sprite_centered(e.kind.sprite(), p.x, p.y + bob - 4.0, e.radius / 5.0, e.facing.x < 0.0, tint);
        }
        if let Some(b) = &self.boss {
            self.draw_boss(b, off, t);
        }
        // the student
        let pl = &self.player;
        let p = pl.pos + off;
        glow(p.x, p.y, 120.0, Color::new(1.0, 0.7, 0.3, 1.0), 1.3);
        draw_ellipse(p.x, p.y + 20.0, 16.0, 5.0, 0.0, Color::new(0.0, 0.0, 0.0, 0.35));
        let blink = pl.invuln > 0.0 && pl.dodge_t <= 0.0 && !self.god && (t * 18.0).sin() > 0.0;
        if pl.dodge_t > 0.0 {
            // mid-roll: a quick hop with a dust trail
            let f = 1.0 - pl.dodge_t / DODGE_TIME;
            let hop = (f * std::f32::consts::PI).sin() * 10.0;
            draw_circle(p.x - pl.dodge_dir.x * 14.0, p.y + 16.0, 6.0 * (1.0 - f), Color::new(0.85, 0.8, 0.7, 0.4));
            draw_sprite_centered(&STUDENT, p.x, p.y - 6.0 - hop, 2.75, pl.dodge_dir.x < 0.0, Color::new(1.0, 1.0, 1.0, 0.75));
        } else if !blink {
            let step = if pl.walk > 0.0 { ((pl.walk * 12.0).sin() * 2.0).round() } else { 0.0 };
            draw_sprite_centered(&STUDENT, p.x, p.y - 6.0 + step, 2.75, pl.facing.x < 0.0, WHITE);
        }
        // the foam-dart blaster: an orange toy with a blue barrel, pointing where you aim
        if pl.dodge_t <= 0.0 {
            let d = pl.swing_dir;
            let n = vec2(-d.y, d.x);
            let grip = p + vec2(0.0, 2.0) + d * 10.0;
            let muzzle = grip + d * 18.0;
            draw_line(grip.x, grip.y, muzzle.x, muzzle.y, 7.0, Color::from_rgba(255, 140, 40, 255));
            let tip = muzzle + d * 5.0;
            draw_line(muzzle.x, muzzle.y, tip.x, tip.y, 5.0, Color::from_rgba(70, 140, 255, 255));
            let h = grip + n * 4.0 * if d.x < 0.0 { -1.0 } else { 1.0 };
            draw_line(grip.x, grip.y, h.x + d.x * 2.0, h.y + 5.0, 4.0, Color::from_rgba(230, 230, 235, 255));
            if pl.swing_t > 0.0 {
                draw_circle(tip.x, tip.y, 5.0 * (pl.swing_t / MUZZLE_TIME) + 2.0, Color::new(1.0, 0.95, 0.8, 0.7));
            }
        }
        for dart in &self.darts {
            let q = dart.pos + off;
            let v = dart.vel.normalize_or_zero();
            let tail = q - v * 9.0;
            draw_line(tail.x, tail.y, q.x, q.y, 4.0, Color::from_rgba(255, 140, 40, 255));
            draw_circle(q.x, q.y, 3.0, Color::from_rgba(70, 140, 255, 255));
        }
        for pr in &self.projectiles {
            let q = pr.pos + off;
            let r = pr.radius;
            match pr.kind {
                ProjKind::Envelope | ProjKind::Page => {
                    let (c1, c2) = if pr.kind == ProjKind::Page { (PAPER, RISKY) } else { (WHITE, KSU_GOLD) };
                    draw_rectangle(q.x - r, q.y - r * 0.7, r * 2.0, r * 1.4, c1);
                    draw_line(q.x - r, q.y - r * 0.7, q.x, q.y + 1.0, 2.0, c2);
                    draw_line(q.x + r, q.y - r * 0.7, q.x, q.y + 1.0, 2.0, c2);
                }
                ProjKind::Padlock => {
                    draw_circle_lines(q.x, q.y - r * 0.5, r * 0.6, 2.5, KSU_GOLD);
                    draw_rectangle(q.x - r, q.y - r * 0.3, r * 2.0, r * 1.3, KSU_GOLD);
                    draw_rectangle(q.x - 1.5, q.y + 0.5, 3.0, 4.0, INK);
                }
                ProjKind::Hook => {
                    draw_circle(q.x, q.y, r + 2.0, Color::new(1.0, 0.77, 0.16, 0.45));
                    draw_circle_lines(q.x, q.y, r, 2.5, Color::from_rgba(200, 200, 210, 255));
                    draw_circle(q.x, q.y, r * 0.4, INK);
                }
                ProjKind::Ticket => {
                    // a little yellow citation slip with a black stripe
                    draw_rectangle(q.x - r - 2.0, q.y - r * 0.8 - 1.0, r * 2.0 + 4.0, r * 1.6 + 2.0, INK);
                    draw_rectangle(q.x - r - 1.0, q.y - r * 0.8, r * 2.0 + 2.0, r * 1.6, KSU_GOLD);
                    draw_rectangle(q.x - r - 1.0, q.y - 1.0, r * 2.0 + 2.0, 2.0, INK);
                }
                ProjKind::Orb => {
                    draw_circle(q.x, q.y, r, Color::from_rgba(90, 220, 255, 255));
                    draw_circle(q.x, q.y, r * 0.45, WHITE);
                }
            }
        }
        for pa in &self.particles {
            let a = (pa.life / pa.max).clamp(0.0, 1.0);
            let q = pa.pos + off;
            let s = pa.size * a;
            draw_rectangle(q.x - s * 0.5, q.y - s * 0.5, s, s, Color::new(pa.color.r, pa.color.g, pa.color.b, a));
        }
        // fog of war: unseen = black, remembered = dim (floors only)
        if !self.arena {
            let x0 = ((-off.x) / TILE).floor().max(0.0) as i32;
            let y0 = ((56.0 - off.y) / TILE).floor().max(0.0) as i32;
            let x1 = (((VW - off.x) / TILE).ceil() as i32 + 1).min(self.map.w);
            let y1 = (((VH - off.y) / TILE).ceil() as i32 + 1).min(self.map.h);
            for y in y0..y1 {
                for x in x0..x1 {
                    let i = self.map.idx(x, y);
                    let a = if !self.map.seen[i] {
                        1.0
                    } else if !self.map.visible[i] {
                        0.62
                    } else {
                        continue;
                    };
                    draw_rectangle(x as f32 * TILE + off.x, y as f32 * TILE + off.y, TILE, TILE, Color::new(0.02, 0.015, 0.03, a));
                }
            }
        }
        for ft in &self.texts {
            let a = (ft.life / 0.9).clamp(0.0, 1.0);
            let q = ft.pos + off;
            centered(&ft.text, q.x + 1.0, q.y + 1.0, 22.0, Color::new(0.0, 0.0, 0.0, a * 0.8));
            centered(&ft.text, q.x, q.y, 22.0, Color::new(ft.color.r, ft.color.g, ft.color.b, a));
        }
    }

    fn draw_boss(&self, b: &Boss, off: Vec2, t: f32) {
        let p = b.pos + off;
        match (b.kind, b.state) {
            (BossKind::TowTyrant, BossState::Tell) => {
                let end = p + b.dir * 560.0;
                draw_line(p.x, p.y, end.x, end.y, 10.0, Color::new(1.0, 0.2, 0.2, 0.15 + 0.25 * (t * 20.0).sin().abs()));
                draw_line(p.x, p.y, end.x, end.y, 2.0, RISKY);
            }
            (BossKind::FinalsGolem, BossState::Tell) => {
                let f = (1.0 - b.t / 0.8).clamp(0.0, 1.0);
                draw_circle_lines(p.x, p.y, 30.0 + 150.0 * f, 4.0, Color::new(1.0, 0.3, 0.25, 0.3 + 0.6 * f));
            }
            (BossKind::OwlQueen, BossState::Tell) => glow(p.x, p.y, 100.0, b.kind.color(), 5.0),
            _ => {}
        }
        if b.enraged() {
            glow(p.x, p.y, 120.0, Color::new(1.0, 0.2, 0.2, 1.0), 1.6 + (t * 6.0).sin());
        } else {
            glow(p.x, p.y, 120.0, b.kind.color(), 1.3);
        }
        let enter = (1.0 - b.entering.max(0.0)).clamp(0.0, 1.0);
        let alpha = b.alpha * enter;
        draw_ellipse(p.x, p.y + 52.0, 46.0, 10.0, 0.0, Color::new(0.0, 0.0, 0.0, 0.4 * alpha));
        let tint = if b.flash > 0.0 { Color::new(1.0, 0.45, 0.45, alpha) } else { Color::new(1.0, 1.0, 1.0, alpha) };
        let bob = (b.anim * 2.5).sin() * 4.0;
        draw_sprite_centered(b.kind.sprite(), p.x, p.y + bob, 7.0, b.pos.x > self.player.pos.x, tint);
    }
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    fn power() -> Power {
        Power { knowledge: 4, focus: 3, can_heal: true }
    }

    /// Floor `f` of the 10-floor run.
    fn tier(f: u32) -> Tier {
        Tier::of(f, 10)
    }

    /// macroquad's RNG is one global: tests that seed it hold this lock so
    /// parallel tests can't interleave draws and change each other's numbers.
    fn rng_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn floors_are_connected_and_place_everything_reachably() {
        let _rng = rng_lock();
        for seed in 1..40u64 {
            macroquad::rand::srand(seed);
            for f in 0..10 {
                let w = World::floor(tier(f));
                let mut field = Vec::new();
                let mut q = VecDeque::new();
                w.map.bfs(tile_of(w.player.pos), u16::MAX - 1, &mut field, &mut q);
                let reach = |p: Vec2| field[w.map.idx(tile_of(p).0, tile_of(p).1)] != UNREACHED;
                assert!(reach(w.npc), "seed {seed} floor {f}: character unreachable");
                assert!(reach(w.stairs), "seed {seed} floor {f}: stairs unreachable");
                assert!(reach(w.keeper_aside), "seed {seed} floor {f}: keeper's step-aside tile unreachable");
                assert!(w.npc.distance(w.stairs) > TILE, "seed {seed} floor {f}: character is on the stairs");
                assert_eq!(w.enemies.len(), tier(f).enemy_count(), "seed {seed} floor {f}");
                assert_eq!((w.map.w, w.map.h), tier(f).map_size());
                assert!(w.enemies.iter().all(|e| !w.map.solid_at(e.pos)));
            }
        }
    }

    #[test]
    fn wardens_are_two_different_bosses_on_every_floor() {
        let _rng = rng_lock();
        let mut pairs = std::collections::HashSet::new();
        for f in 0..20 {
            let [a, b] = wardens_for(f);
            assert_ne!(a, b);
            pairs.insert(if (a as u8) < (b as u8) { (a as u8, b as u8) } else { (b as u8, a as u8) });
        }
        assert_eq!(pairs.len(), 6, "a long run meets every pairing");
        // floor 1 keeps the gentlest pair, floor 10 ends on the two ranged wardens
        assert_eq!(wardens_for(0), [BossKind::TowTyrant, BossKind::FinalsGolem]);
        assert_eq!(wardens_for(9), [BossKind::OwlQueen, BossKind::LibraryLich]);
    }

    /// Simulate the warden fight with the autopilot (invulnerable): both wardens must fall, in order.
    #[test]
    fn autopilot_beats_both_wardens() {
        let _rng = rng_lock();
        macroquad::rand::srand(7);
        for f in 0..10u32 {
            let mut w = World::arena(tier(f), &wardens_for(f as usize));
            w.god = true;
            let mut downs = Vec::new();
            let mut entered = 0;
            for _ in 0..(60 * 240) {
                let ctl = w.auto_control(None);
                w.update(1.0 / 60.0, ctl, power());
                for e in w.events.drain(..) {
                    match e {
                        WorldEvent::BossDown(k) => downs.push(k),
                        WorldEvent::BossEntered(_) => entered += 1,
                        WorldEvent::PlayerHit => panic!("god mode can't be hit"),
                        _ => {}
                    }
                }
                w.sounds.clear();
                if w.arena_cleared() {
                    break;
                }
            }
            assert_eq!(entered, 2, "floor {f}");
            assert_eq!(downs, wardens_for(f as usize).to_vec(), "floor {f}: both wardens fall in order");
            assert!(w.arena_cleared());
        }
    }

    /// Without god mode every warden lands hits on a player who stands still.
    #[test]
    fn wardens_can_hurt_you() {
        let _rng = rng_lock();
        macroquad::rand::srand(3);
        for kind in BossKind::ALL {
            let mut w = World::arena(tier(0), &[kind]);
            let mut hits = 0;
            for _ in 0..(60 * 20) {
                w.update(1.0 / 60.0, Control::default(), power());
                hits += w.events.drain(..).filter(|e| *e == WorldEvent::PlayerHit).count();
                w.sounds.clear();
            }
            assert!(hits > 0, "{kind:?} never hit a player standing still");
        }
    }

    #[test]
    fn coffee_heals_only_when_hurt() {
        let _rng = rng_lock();
        macroquad::rand::srand(5);
        let mut w = World::arena(tier(0), &[]);
        w.pickups.push(Pickup { pos: w.player.pos, phase: 0.0 });
        w.update(0.05, Control::default(), Power { can_heal: false, ..power() });
        assert!(!w.events.contains(&WorldEvent::Healed));
        assert_eq!(w.pickups.len(), 1);
        w.update(0.05, Control::default(), power());
        assert_eq!(w.events.iter().filter(|e| **e == WorldEvent::Healed).count(), 1);
        assert!(w.pickups.is_empty());
    }

    #[test]
    fn darts_kill_a_monster_in_front() {
        let _rng = rng_lock();
        macroquad::rand::srand(9);
        let mut w = World::arena(tier(0), &[]);
        let p = w.player.pos;
        w.enemies.push(Enemy::new(EnemyKind::SpamOwl, p + vec2(120.0, 0.0), tier(0)));
        for _ in 0..60 {
            w.update(1.0 / 60.0, Control { mv: Vec2::ZERO, attack: true, aim: Some(p + vec2(200.0, 0.0)), dodge: false }, power());
        }
        assert!(w.enemies.is_empty());
        assert!(w.events.contains(&WorldEvent::EnemyKilled));
    }

    /// Aim assist: with no mouse aim, darts find the nearest monster in sight.
    #[test]
    fn aim_assist_finds_the_nearest_monster() {
        let _rng = rng_lock();
        macroquad::rand::srand(4);
        let mut w = World::arena(tier(0), &[]);
        let p = w.player.pos;
        w.enemies.push(Enemy::new(EnemyKind::MysteryMeat, p + vec2(0.0, 100.0), tier(0)));
        for _ in 0..90 {
            w.update(1.0 / 60.0, Control { attack: true, ..Control::default() }, power());
        }
        assert!(w.enemies.is_empty(), "the dart should have found the slime below");
    }

    /// A dodge roll moves you fast and a shot can't land at its start.
    #[test]
    fn dodge_roll_is_fast_and_safe() {
        let _rng = rng_lock();
        macroquad::rand::srand(6);
        let mut w = World::arena(tier(0), &[]);
        let start = w.player.pos;
        w.projectiles.push(Projectile { pos: start, vel: vec2(1.0, 0.0), radius: 8.0, life: 1.0, kind: ProjKind::Orb });
        w.update(1.0 / 60.0, Control { mv: vec2(0.0, 1.0), dodge: true, ..Control::default() }, power());
        assert!(!w.events.contains(&WorldEvent::PlayerHit), "rolling through a shot");
        for _ in 0..18 {
            w.update(1.0 / 60.0, Control { mv: vec2(0.0, 1.0), ..Control::default() }, power());
        }
        let rolled = w.player.pos.distance(start);
        assert!(rolled > PLAYER_SPEED * 0.3 * 1.4, "rolled only {rolled}");
        // no second roll until the cooldown is over
        let before = w.player.dodge_cd;
        w.update(1.0 / 60.0, Control { dodge: true, ..Control::default() }, power());
        assert!(w.player.dodge_t <= 0.0 && before > 0.0);
    }

    #[test]
    fn walls_block_line_of_sight() {
        let _rng = rng_lock();
        macroquad::rand::srand(11);
        let mut m = Map::arena();
        // pillar at (7,4): standing at (5,4), the tile at (9,4) is hidden behind it
        m.visible.fill(false);
        m.fov_origin = (-1, -1);
        m.compute_fov(5, 4, 8);
        assert!(m.visible[m.idx(7, 4)], "the pillar itself is visible");
        assert!(!m.visible[m.idx(9, 4)], "the tile behind the pillar is not");
        assert!(m.visible[m.idx(9, 6)]);
    }

    /// One warden-arena fight with real health: start at `health`, a hit costs
    /// 1, the coffee between wardens heals 1 when hurt. `lag` = frames of
    /// reaction delay (the bot acts on what it decided `lag` frames ago).
    /// Returns (survived, hits taken, seconds).
    /// `knowledge` defaults to the least a player can have on that floor (one point per lesson).
    fn fight(t: Tier, health: i32, lag: usize) -> (bool, i32, f32) {
        fight_vs(t, &wardens_for(t.floor as usize), health, lag, t.floor as i32 + 1)
    }

    fn fight_vs(t: Tier, kinds: &[BossKind], health: i32, lag: usize, knowledge: i32) -> (bool, i32, f32) {
        let mut w = World::arena(t, kinds);
        let mut hp = health;
        let mut hits = 0;
        let mut queue: VecDeque<Control> = VecDeque::new();
        for frame in 0..(60 * 240) {
            let c = w.auto_control(None);
            queue.push_back(c);
            let ctl = if queue.len() > lag { queue.pop_front().unwrap_or_default() } else { Control::default() };
            w.update(1.0 / 60.0, ctl, Power { knowledge, focus: 2, can_heal: hp < 5 });
            for e in w.events.drain(..) {
                match e {
                    WorldEvent::PlayerHit => {
                        hp -= 1;
                        hits += 1;
                    }
                    WorldEvent::Healed => hp = (hp + 1).min(5),
                    _ => {}
                }
            }
            w.sounds.clear();
            if hp <= 0 {
                return (false, hits, frame as f32 / 60.0);
            }
            if w.arena_cleared() {
                return (true, hits, frame as f32 / 60.0);
            }
        }
        (false, hits, 240.0)
    }

    fn median(v: &mut [i32]) -> i32 {
        v.sort_unstable();
        v[v.len() / 2]
    }

    /// The fight from full health (the arena refills it) is beatable but not free.
    /// A sharp bot (perfect aim, dodges telegraphs and bolts as they appear) takes
    /// a median of at most 4 hits over both wardens and nearly always survives; the
    /// same bot reacting 0.25 s late survives at least 8 times in 20 on every floor.
    #[test]
    fn two_wardens_are_beatable_but_not_trivial() {
        let _rng = rng_lock();
        let mut slow_medians = Vec::new();
        for f in 0..10u32 {
            for (lag, label) in [(0usize, "sharp"), (15, "slow")] {
                let mut hits = Vec::new();
                let mut wins = 0;
                for seed in 0..20u64 {
                    macroquad::rand::srand(1000 + seed);
                    let (won, h, _) = fight(tier(f), 5, lag);
                    wins += usize::from(won);
                    hits.push(h);
                }
                let m = median(&mut hits);
                if lag == 0 {
                    assert!(m <= 4, "floor {f} {label}: median {m} hits");
                    assert!(wins >= 17, "floor {f} {label}: only {wins}/20 survived");
                } else {
                    assert!(wins >= 8, "floor {f} {label}: only {wins}/20 survived");
                    slow_medians.push(m);
                }
            }
        }
        // gentler, not free: the late bot still gets hit somewhere on floors 6-10
        assert!(slow_medians[5..].iter().any(|m| *m >= 1), "slow bot never got hit late in the run: {slow_medians:?}");
    }

    /// Prints the numbers used to tune WARDEN_HP:
    /// `cargo test --release warden_difficulty -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn warden_difficulty() {
        let _rng = rng_lock();
        HIT_SOURCES.with(|c| c.set([0; 3]));
        for lag in [0usize, 15] {
            for kind in BossKind::ALL {
                let mut hits = Vec::new();
                for seed in 0..40u64 {
                    macroquad::rand::srand(seed);
                    hits.push(fight_vs(Tier::of(1, 4), &[kind], 50, lag, 2).1);
                }
                let m = median(&mut hits);
                let [charge, mob, proj] = HIT_SOURCES.with(|c| c.replace([0; 3]));
                println!(
                    "lag {:.2}s one {:?} (floor 2 HP): median hits {m}, mean {:.1} (charge {charge}, monster {mob}, bolt {proj})",
                    lag as f32 / 60.0,
                    kind,
                    hits.iter().sum::<i32>() as f32 / 40.0
                );
            }
        }
        // the old 4-floor run (its pairs and curve: the balance the 10-floor run
        // is measured against), then the 10-floor run
        use BossKind::*;
        let old_pairs = [[TowTyrant, FinalsGolem], [FinalsGolem, OwlQueen], [OwlQueen, LibraryLich], [LibraryLich, TowTyrant]];
        for floors in [4u32, 10] {
            for lag in [0usize, 15] {
                for f in 0..floors {
                    let t = Tier::of(f, floors);
                    let [a, b] = if floors == 4 { old_pairs[f as usize] } else { wardens_for(f as usize) };
                    let (mut hits, mut wins, mut secs) = (Vec::new(), 0, 0.0);
                    for seed in 0..40u64 {
                        macroquad::rand::srand(seed);
                        let (won, h, s) = fight_vs(t, &[a, b], 5, lag, f as i32 + 1);
                        wins += usize::from(won);
                        hits.push(h);
                        secs += s;
                    }
                    let m = median(&mut hits);
                    let [charge, mob, proj] = HIT_SOURCES.with(|c| c.replace([0; 3]));
                    println!(
                        "{floors}-floor run, lag {:.2}s floor {:>2} (depth {:.2}, warden HP {:.0}, knowledge {}, {:?}+{:?}): survived {}/40, median hits {}, max hits {}, mean fight {:.0}s (hits by charge {charge}, monster {mob}, bolt {proj})",
                        lag as f32 / 60.0,
                        f + 1,
                        t.depth,
                        t.warden_hp(),
                        f + 1,
                        a,
                        b,
                        wins,
                        m,
                        hits.last().copied().unwrap_or(0),
                        secs / 40.0
                    );
                }
            }
        }
    }

    fn tier_pace(t: crate::ai_features::SkillTier) -> Pace {
        let p = t.params();
        Pace::capped(p.monster_speed_pct, p.monster_count_delta, p.warden_hp_pct)
    }

    const SKILL_TIERS: [crate::ai_features::SkillTier; 4] = {
        use crate::ai_features::SkillTier::*;
        [Gentle, Standard, Sharp, Expert]
    };

    /// Adaptive pacing (the skill tier) must keep the warden fight fair: on
    /// floor 10 the slow bot (0.25 s late) survives at least 45% of the time
    /// at every tier, with the pace applied to the arena (warden health and the
    /// Night Owl Queen's summoned owls). Same seeds as `warden_difficulty`.
    #[test]
    fn skill_tier_pacing_keeps_the_last_wardens_fair() {
        let _rng = rng_lock();
        for st in SKILL_TIERS {
            let t = tier(9).with_pace(tier_pace(st));
            let mut wins = 0;
            for seed in 0..40u64 {
                macroquad::rand::srand(seed);
                wins += usize::from(fight(t, 5, 15).0);
            }
            assert!(wins * 100 >= 45 * 40, "{st:?}: slow bot survived {wins}/40 on floor 10");
        }
    }

    /// Paced floors still place a sensible number of monsters.
    #[test]
    fn skill_tier_pacing_changes_monster_count_and_speed() {
        let _rng = rng_lock();
        for st in SKILL_TIERS {
            let pace = tier_pace(st);
            assert!(pace.warden_hp_pct <= 100 && pace.monster_speed_pct <= Pace::MAX_SPEED_PCT);
            for f in [0u32, 9] {
                macroquad::rand::srand(3);
                let t = tier(f).with_pace(pace);
                let w = World::floor(t);
                assert_eq!(w.enemies.len(), t.paced_enemy_count(), "{st:?} floor {f}");
                assert!(w.enemies.len() >= 3);
            }
        }
        let fast = tier(0).with_pace(Pace::capped(150, 9, 150));
        assert_eq!(fast.pace, Pace { monster_speed_pct: 110, monster_count_delta: 2, warden_hp_pct: 100 });
    }

    /// Survival per skill tier, capped (what ships) and uncapped (the backend's
    /// suggested knobs): `cargo test --release skill_tier_difficulty -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn skill_tier_difficulty() {
        let _rng = rng_lock();
        // SIM_SEEDS=n: more seeds per floor (default 40, as `warden_difficulty`)
        let n: u64 = std::env::var("SIM_SEEDS").ok().and_then(|v| v.parse().ok()).unwrap_or(40);
        for st in SKILL_TIERS {
            let p = st.params();
            let raw = Pace { monster_speed_pct: p.monster_speed_pct, monster_count_delta: p.monster_count_delta, warden_hp_pct: p.warden_hp_pct };
            for (label, pace) in [("capped", tier_pace(st)), ("uncapped", raw)] {
                let mut row = String::new();
                for f in 0..10u32 {
                    let t = tier(f).with_pace(pace);
                    let mut wins = 0;
                    for seed in 0..n {
                        macroquad::rand::srand(seed);
                        wins += usize::from(fight(t, 5, 15).0);
                    }
                    row.push_str(&format!(" {wins:>2}"));
                }
                println!("{st:?} {label:>8} (warden HP {}%, owl speed {}%): slow bot survived /{n} on floors 1-10:{row}", pace.warden_hp_pct, pace.monster_speed_pct);
            }
        }
    }

    #[test]
    fn ten_floors_are_ten_difficulty_levels() {
        let _rng = rng_lock();
        assert_eq!((0..10).map(|f| tier(f).level()).collect::<Vec<_>>(), (1..=10).collect::<Vec<_>>());
        assert_eq!(tier(0).depth, 0.0);
        assert_eq!(tier(9).depth, MAX_DEPTH);
        assert_eq!(Tier::of(42, 10), tier(9));
        // every floor is a step harder than the one before
        let mut prev = tier(0);
        for f in 1..10 {
            let t = tier(f);
            assert!(t.depth > prev.depth, "floor {f}");
            assert!(t.warden_hp() > prev.warden_hp(), "floor {f}: warden health");
            assert!(t.enemy_speed_scale() > prev.enemy_speed_scale(), "floor {f}: monster speed");
            assert!(t.enemy_count() >= prev.enemy_count(), "floor {f}: monster count");
            assert!(t.curve() > prev.curve());
            prev = t;
        }
        assert_eq!((tier(0).enemy_count(), tier(9).enemy_count()), (4, 9));
        // monsters are slower than their listed speed, even on floor 10
        assert!(tier(0).enemy_speed_scale() < 0.8 && tier(9).enemy_speed_scale() < 0.95);
        // scaled back: floor 10 is gentler than the old curve's last floor
        assert!(tier(9).curve() < 3.0);
        let old_top = (WARDEN_HP + WARDEN_HP_PER_FLOOR * 3.0) * Tier::swing(10.0) / Tier::swing(4.0);
        assert!(tier(9).warden_hp() < old_top * 0.85, "{} vs old {old_top}", tier(9).warden_hp());
        // the monster mix changes on every floor
        for f in 1..10 {
            assert_ne!(enemy_mix(tier(f).depth), enemy_mix(tier(f - 1).depth), "floor {f}");
        }
    }

    #[test]
    fn owls_replace_bats_and_new_monsters_show_up_deeper() {
        let _rng = rng_lock();
        assert!(EnemyKind::ALL.iter().all(|k| !k.name().to_lowercase().contains("bat")));
        assert_eq!(EnemyKind::SpamOwl.name(), "Spam Owl");
        for f in 0..10 {
            let mix = enemy_mix(tier(f).depth);
            assert_eq!(mix.iter().sum::<u32>(), 100, "floor {f}");
            assert!(mix[1] > 0, "floor {f}: owls everywhere");
        }
        assert_eq!(enemy_mix(tier(0).depth)[2], 0, "no Night Owls on floor 1");
        assert!(enemy_mix(tier(4).depth)[2] > 0, "Night Owls by floor 5");
        assert!(enemy_mix(tier(9).depth)[6] > 0, "the Late Shuttle by floor 10");
        let names: std::collections::HashSet<&str> = EnemyKind::ALL.iter().map(|k| k.name()).collect();
        assert_eq!(names.len(), EnemyKind::ALL.len());
    }
}
