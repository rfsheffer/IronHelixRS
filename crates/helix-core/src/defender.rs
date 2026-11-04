//! The defender (design doc §9, Lingo `Def`, `Move`, `Gen`).
//!
//! It walks its own graph, the Def table, which shares line numbers with the
//! node tables. Rows are a binary search tree over node numbers within each
//! deck; a plan is a list of relative line jumps found by searching the tree,
//! each with the line-of-sight list of the row it lands on. Those lists decide
//! the warning level and whether the probe is caught.
//!
//! It steps when the player has been idle for a while (Director's `timeout`:
//! input resets it) and every few player moves. Jamming delays it; the
//! bulkheads trap it; computer actions turn it off. Tasks queued by computer
//! actions get a response a minute later; a disabled defender (and the door
//! sensors) comes back after five minutes.

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use helixdb::death::{DeathRow, Token};
use helixdb::defender::DefNode;

use crate::rng::Rng;

/// Where the defender starts (and respawns).
pub const START_LINE: usize = 5;
/// Ticks are 1/60 s.
const TICK: f32 = 1.0 / 60.0;
/// A plan longer than this is dropped for a random target.
const MAX_PLAN: usize = 40;
/// Nodes per deck, for picking a random target (Lingo's NodeIndex).
const NODES_PER_DECK: [u32; 6] = [23, 41, 5, 7, 16, 10];
/// Deck 2's tree is split: these nodes are one section, joined to the rest
/// between 2.13 and 2.23.
const DECK_2_SECTION: std::ops::RangeInclusive<i32> = 3..=16;
/// How long a disabled defender (or the door sensors) stays down.
pub const DOWN_SECONDS: f32 = 5.0 * 60.0;
/// When a queued task gets its response.
const TASK_SECONDS: f32 = 60.0;
/// Warning lamp timing, in ticks.
const WARNING_ONE: f32 = 20.0;
const WARNING_TWO: f32 = 40.0;
const AMBIENT: f32 = 90.0;

/// The Def and DeathDB tables, by line.
#[derive(Debug, Default)]
pub struct DefenderData {
    rows: BTreeMap<usize, Vec<String>>,
    deaths: BTreeMap<usize, Vec<Vec<Token>>>,
}

impl DefenderData {
    pub fn new(def: &[DefNode], deaths: &[DeathRow]) -> Self {
        Self {
            rows: def
                .iter()
                .map(|n| (n.line, n.fields.iter().map(|f| f.clone().unwrap_or_else(|| "#".into())).collect()))
                .collect(),
            deaths: deaths.iter().map(|d| (d.line, d.groups.clone())).collect(),
        }
    }

    /// Lingo `item n` of a Def row (1-based).
    fn item(&self, line: usize, n: usize) -> &str {
        self.rows.get(&line).and_then(|r| r.get(n - 1)).map_or("", String::as_str)
    }

    fn int(&self, line: usize, n: usize) -> i32 {
        let s = self.item(line, n).trim();
        s.parse().unwrap_or_else(|_| leading_number(s))
    }

    fn id(&self, line: usize) -> &str {
        self.item(line, 1)
    }
}

/// Lingo's `value` of a string: the leading number ("1." is 1).
fn leading_number(s: &str) -> i32 {
    let digits: String = s.trim().chars().take_while(|c| c.is_ascii_digit() || *c == '-').collect();
    digits.parse().unwrap_or(0)
}

/// The node number in an id: chars 3-4 ("1.05" -> 5).
fn node_number(id: &str) -> i32 {
    leading_number(&id.chars().skip(2).take(2).collect::<String>())
}

fn deck(id: &str) -> u8 {
    id.chars().next().and_then(|c| c.to_digit(10)).unwrap_or(0) as u8
}

/// The door sensors' state (§8.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sensors {
    Active,
    /// Turned off from a computer, for five minutes.
    Off,
    /// The defender has answered the switch-off; they alert it again.
    Done,
}

/// The warning lamp under the view (Lingo's CB light).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lamp {
    Off,
    /// The ambient blip, at one of ten positions.
    Ambient(u8),
    One,
    Two,
}

/// Where the probe is, as the defender sees it.
#[derive(Debug, Clone)]
pub struct Probe {
    pub line: usize,
    pub id: String,
    pub level: u8,
    pub facing: usize,
    pub door_open: bool,
}

/// Something the defender does that the game must act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefenderEvent {
    /// It caught the probe: play this death video (folder, name), or the
    /// static if None.
    Caught(Option<(String, String)>),
    Sound(&'static str),
    /// It responds to a queued task (`<task>BOL` message).
    Responds(String),
    /// Back on line after being disabled (the map switches to its view).
    Back,
}

#[derive(Debug, Clone)]
pub struct Defender {
    data: Arc<DefenderData>,
    pub(crate) line: usize,
    prev_line: usize,
    path: VecDeque<i32>,
    /// Line of sight of each planned step.
    los: VecDeque<String>,
    /// Its current line of sight.
    sight: String,
    /// Steps left before the delay is re-rolled, and the delay in ticks.
    run: u32,
    delay: f32,
    /// Seconds since the last input or step.
    idle: f32,
    pub(crate) off: bool,
    pub(crate) trapped: bool,
    pub(crate) jammed: bool,
    pub(crate) sensors: Sensors,
    pub(crate) tasks: Vec<String>,
    task_at: Option<f32>,
    /// When a disabled defender (or the sensors) comes back.
    pub(crate) back_at: Option<f32>,
    user_moves: u32,
    pub(crate) warning: u8,
    lamp: Lamp,
    lamp_timer: f32,
    blink: bool,
}

/// Plan state while searching the tree (Lingo's QS* globals).
struct Search<'a> {
    data: &'a DefenderData,
    pos: usize,
    node: i32,
    path: Vec<i32>,
    los: Vec<String>,
    /// Gave up (too long): plan a random target instead.
    too_long: bool,
}

impl Search<'_> {
    fn jump(&mut self, by: i32) {
        self.pos = (self.pos as i64 + by as i64).max(0) as usize;
        self.path.push(by);
        self.los.push(self.data.item(self.pos, 7).to_string());
    }

    /// Lingo's QuickSearch: walk the tree toward a node number on this deck.
    fn quick_search(&mut self, target: i32) {
        for _ in 0..200 {
            let (left, left_max) = (self.data.int(self.pos, 3), self.data.int(self.pos, 5));
            let (right, right_max) = (self.data.int(self.pos, 4), self.data.int(self.pos, 6));
            let jump = if target <= left_max && target >= self.node + left {
                left
            } else if target <= right_max && target >= self.node + right && right != 0 {
                right
            } else {
                self.data.int(self.pos, 2)
            };
            self.jump(jump);
            self.node = node_number(self.data.id(self.pos));
            if self.path.len() > MAX_PLAN {
                self.too_long = true;
                return;
            }
            if self.node == target {
                return;
            }
        }
        self.too_long = true;
    }
}

impl Defender {
    pub fn new(data: Arc<DefenderData>) -> Self {
        Self {
            data,
            line: START_LINE,
            prev_line: START_LINE,
            path: VecDeque::new(),
            los: VecDeque::new(),
            sight: String::new(),
            run: 0,
            delay: 0.0,
            idle: 0.0,
            off: false,
            trapped: false,
            jammed: false,
            sensors: Sensors::Active,
            tasks: Vec::new(),
            task_at: None,
            back_at: None,
            user_moves: 0,
            warning: 3,
            lamp: Lamp::Ambient(0),
            lamp_timer: 0.0,
            blink: false,
        }
        .with_sight()
    }

    fn with_sight(mut self) -> Self {
        self.sight = self.data.item(self.line, 7).to_string();
        self
    }

    pub fn id(&self) -> &str {
        self.data.id(self.line)
    }

    /// Its line in the node tables (for its map position).
    pub fn line(&self) -> usize {
        self.line
    }

    pub fn warning(&self) -> u8 {
        self.warning
    }

    pub fn off(&self) -> bool {
        self.off
    }

    pub fn jammed(&self) -> bool {
        self.jammed
    }

    pub fn level(&self) -> u8 {
        deck(self.id())
    }

    pub fn in_ladder(&self) -> bool {
        self.id().contains('L')
    }

    pub fn lamp(&self) -> Lamp {
        self.lamp
    }

    /// Its map icon is shown (it blinks while jammed, and is hidden when off).
    pub fn visible(&self) -> bool {
        !self.off && !(self.jammed && self.blink)
    }

    /// A new probe: back to the start with no plan (Lingo's InitVars and
    /// InitDBs); tasks and return timers carry over.
    pub(crate) fn restart(&mut self) {
        self.line = START_LINE;
        self.prev_line = START_LINE;
        self.path.clear();
        self.los.clear();
        self.sight = self.data.item(self.line, 7).to_string();
        self.off = false;
        self.trapped = false;
        self.jammed = false;
        self.sensors = Sensors::Active;
        self.run = 0;
        self.user_moves = 0;
        self.warning = 3;
    }

    /// Lingo's SetDefenderRun: the step delay, re-rolled every few steps.
    fn set_run(&mut self, rng: &mut Rng, skill: u8) {
        self.delay = ((6 - skill as i32) * 50 + rng.random(150) as i32 + 100) as f32;
        self.run = rng.random(10) + 5;
    }

    /// The player did something: the idle timer starts again.
    pub(crate) fn input(&mut self) {
        self.idle = 0.0;
    }

    /// Lingo's SetWarningLevel.
    pub(crate) fn set_warning(&mut self, probe: &Probe) {
        if self.off {
            self.warning = 4;
            return;
        }
        let door = probe.door_open.then(|| format!("*{}-{}*", probe.facing, probe.id));
        let node = format!("*{}*", probe.id);
        let seen = |s: &str| s.contains(&node) || door.as_ref().is_some_and(|d| s.contains(d.as_str()));
        if seen(&self.sight) {
            self.warning = 0;
            return;
        }
        let Some(next) = self.los.front() else { return };
        if seen(next) {
            self.warning = 1;
            return;
        }
        let mut later = self.los.get(1).cloned().unwrap_or_default();
        if self.level() == probe.level {
            later.push_str(self.los.get(2).map_or("", String::as_str));
        }
        self.warning = if seen(&later) { 2 } else { 3 };
    }

    /// Lingo's DefPathToTarget, toward "deck.node".
    pub(crate) fn plan_to(&mut self, target: &str, rng: &mut Rng, probe: &Probe) {
        for _ in 0..8 {
            let (path, los, too_long) = self.search(target);
            if !too_long {
                self.path = path.into();
                self.los = los.into();
                self.sight.clear();
                self.set_warning(probe);
                return;
            }
            // Lingo's RandomNode, when the plan grew too long.
            let target = self.random_target(rng);
            let (path, los, too_long) = self.search(&target);
            if !too_long {
                self.path = path.into();
                self.los = los.into();
                self.sight.clear();
                self.set_warning(probe);
                return;
            }
        }
        self.path.clear();
        self.los.clear();
    }

    /// Lingo's RandomNode: somewhere on another deck.
    fn random_target(&self, rng: &mut Rng) -> String {
        let mut level = rng.random(6) as u8;
        while level == self.level() {
            level = rng.random(6) as u8;
        }
        format!("{level}.{}", rng.random(NODES_PER_DECK[level as usize - 1]))
    }

    fn plan_random(&mut self, rng: &mut Rng, probe: &Probe) {
        let target = self.random_target(rng);
        self.plan_to(&target, rng, probe);
    }

    /// The tree search behind a plan: jumps, their sight lists, too long.
    fn search(&self, target: &str) -> (Vec<i32>, Vec<String>, bool) {
        let data = &*self.data;
        let target_node = node_number(target);
        let target_level = deck(target);
        let mut s = Search { data, pos: self.line, node: node_number(self.id()), path: Vec::new(), los: Vec::new(), too_long: false };
        let level = self.level();

        if target_level != level {
            if !self.in_ladder() {
                let here = node_number(self.id());
                let ladder = if target_level > 2 && level <= 2 {
                    [17, 20][level as usize - 1]
                } else if target_level == 6 && level == 5 {
                    4
                } else {
                    data.int(s.pos, 9)
                };
                if here != ladder {
                    s.node = node_number(data.id(s.pos));
                    s.quick_search(ladder);
                }
            }
            let mut search_level = level;
            for _ in 0..12 {
                if target_level == search_level || s.too_long {
                    break;
                }
                let jump = if target_level < search_level { data.int(s.pos, 10) } else { data.int(s.pos, 11) };
                s.jump(jump);
                search_level = deck(data.id(s.pos));
            }
        }
        if data.id(s.pos).contains('L') {
            let out = data.int(s.pos, 12);
            s.jump(out);
        }
        s.node = node_number(data.id(s.pos));
        if target_level == 2 && !s.too_long {
            // Lingo's Dck2ShortCut: cross between the two halves of the tree.
            let (from, to) = (DECK_2_SECTION.contains(&s.node), DECK_2_SECTION.contains(&target_node));
            if from && !to {
                s.quick_search(13);
                s.jump(10);
                s.node = node_number(data.id(s.pos));
            } else if !from && to {
                s.quick_search(23);
                s.jump(-10);
                s.node = node_number(data.id(s.pos));
            }
        }
        if !s.too_long && s.node != target_node {
            s.quick_search(target_node);
        }
        // The other side of a door.
        let across = data.item(s.pos, 8).to_string();
        if !s.too_long && across != "##" && !across.is_empty() && across != "#" {
            let by = leading_number(&across) - node_number(data.id(s.pos));
            s.jump(by);
        }
        (s.path, s.los, s.too_long)
    }

    /// Lingo's AdvDefender: one step of the plan (or plan anew).
    fn advance(&mut self, rng: &mut Rng, skill: u8, probe: &Probe) -> Vec<DefenderEvent> {
        self.trapped = false;
        self.user_moves = 0;
        if self.jammed {
            self.jammed = false;
            self.lamp = Lamp::Ambient(0);
            self.set_run(rng, skill);
        }
        if self.run == 0 {
            self.set_run(rng, skill);
        } else {
            self.run -= 1;
        }
        let Some(jump) = self.path.pop_front() else {
            self.plan_random(rng, probe);
            return Vec::new();
        };
        self.prev_line = self.line;
        self.line = (self.line as i64 + jump as i64).max(0) as usize;
        self.sight = self.data.item(self.line, 7).to_string();
        self.los.pop_front();
        self.set_warning(probe);
        if self.warning == 0 { vec![self.catch(probe, None)] } else { Vec::new() }
    }

    /// Lingo's KillUser: the death video for how it came into view.
    pub(crate) fn catch(&self, probe: &Probe, door: Option<&str>) -> DefenderEvent {
        let from = self.data.id(self.prev_line).to_string();
        let to = self.id().to_string();
        let line = 4 * (probe.line.saturating_sub(1)) + probe.facing + 1;
        let groups = self.data.deaths.get(&line).cloned().unwrap_or_default();
        for (i, group) in groups.iter().enumerate() {
            let hit = group.iter().any(|t| match t {
                Token::Pair(a, b) => *a == from && *b == to,
                Token::Door(d) => door == Some(d.as_str()),
                Token::Other(_) => false,
            });
            if hit {
                let letter = ['N', 'E', 'S', 'W'][probe.facing];
                return DefenderEvent::Caught(Some((
                    format!("Navigation/L{}", probe.level),
                    format!("{}.{letter}.{}", probe.id, i + 1),
                )));
            }
        }
        DefenderEvent::Caught(None)
    }

    /// After a player move: caught, or a step every few moves (§9.1).
    pub(crate) fn after_move(&mut self, rng: &mut Rng, skill: u8, probe: &Probe) -> Vec<DefenderEvent> {
        self.set_warning(probe);
        if self.warning == 0 {
            return vec![self.catch(probe, None)];
        }
        let threshold = 4i32 - skill as i32;
        if !self.off && !self.jammed && !self.trapped && self.user_moves as i32 > threshold && self.user_moves >= 1 {
            self.advance(rng, skill, probe)
        } else {
            self.user_moves += 1;
            Vec::new()
        }
    }

    /// After the probe opens or closes a door: caught if it opens into view
    /// (a door death), else the sensors may call the defender over.
    pub(crate) fn after_door(&mut self, rng: &mut Rng, probe: &Probe, beyond: Option<&str>) -> Vec<DefenderEvent> {
        self.set_warning(probe);
        if self.warning == 0 {
            let door = self.id().to_string();
            return vec![self.catch(probe, Some(&door))];
        }
        let here = format!("*{}*", probe.id);
        let there = beyond.map(|b| format!("*{b}*"));
        let watched = self.los.iter().any(|l| l.contains(&here)) && there.as_ref().is_some_and(|t| self.los.iter().any(|l| l.contains(t.as_str())));
        if !watched && !(self.off || self.sensors == Sensors::Off || self.trapped) {
            self.plan_to(&probe.id, rng, probe);
        }
        Vec::new()
    }

    /// Jam it (§9.5): delays its next step.
    pub(crate) fn jam(&mut self, skill: u8) {
        if !self.off {
            self.delay = if skill == 1 { 400.0 } else { 250.0 };
            self.jammed = true;
            self.lamp = Lamp::Off;
        }
    }

    /// Queue a task from a computer action; it responds a minute later.
    pub(crate) fn queue_task(&mut self, task: &str, now: f32) {
        if self.tasks.is_empty() {
            self.task_at = Some(now + TASK_SECONDS);
        }
        if !self.tasks.iter().any(|t| t == task) {
            self.tasks.push(task.to_string());
        }
    }

    /// Down for five minutes (disabled or trapped); also the door sensors.
    pub(crate) fn down_for_a_while(&mut self, now: f32) {
        self.back_at = Some(now + DOWN_SECONDS);
    }

    /// Lingo's DefOffLine: its plan and warnings clear.
    pub(crate) fn off_line(&mut self, how: &str, now: f32) {
        self.path.clear();
        self.los.clear();
        self.jammed = false;
        self.warning = 4;
        self.lamp = Lamp::Off;
        if !self.tasks.iter().any(|t| t == how) {
            self.tasks.push(how.to_string());
        }
        self.down_for_a_while(now);
    }

    /// Lingo's DefenderAtWork: answer the queued tasks.
    fn respond(&mut self) -> Vec<DefenderEvent> {
        let tasks = std::mem::take(&mut self.tasks);
        self.task_at = None;
        tasks
            .into_iter()
            .map(|t| {
                if t == "DoorSensors" {
                    self.sensors = Sensors::Done;
                }
                DefenderEvent::Responds(t)
            })
            .collect()
    }

    /// Advance by `dt` seconds of play (Lingo's IdleMacro and the timeout).
    pub(crate) fn tick(&mut self, dt: f32, now: f32, rng: &mut Rng, skill: u8, probe: &Probe) -> Vec<DefenderEvent> {
        let mut events = Vec::new();
        self.blink = !self.blink;

        // The lamp: warnings, or the ambient blip.
        self.lamp_timer += dt / TICK;
        if !self.jammed && !self.off {
            let signal = match self.warning {
                2 if self.lamp_timer > WARNING_TWO => Some((Lamp::Two, "Warning Two")),
                1 if self.lamp_timer > WARNING_ONE => Some((Lamp::One, "Warning One")),
                _ => None,
            };
            if let Some((lamp, sound)) = signal {
                // Lingo's SignalWarnings: the lamp flashes on (with the sound) and off.
                if matches!(self.lamp, Lamp::Off | Lamp::Ambient(_)) {
                    self.lamp = lamp;
                    events.push(DefenderEvent::Sound(sound));
                } else {
                    self.lamp = Lamp::Off;
                }
                self.lamp_timer = 0.0;
            } else if self.warning >= 3 && self.lamp_timer > AMBIENT {
                self.lamp = Lamp::Ambient(rng.random(10) as u8 - 1);
                self.lamp_timer = 0.0;
            }
        }

        // Back after five minutes, or answering tasks after one.
        if let Some(at) = self.back_at {
            if now >= at {
                self.back_at = None;
                if self.off {
                    self.line = START_LINE;
                    self.prev_line = START_LINE;
                    self.sight = self.data.item(self.line, 7).to_string();
                }
                self.off = false;
                if self.sensors == Sensors::Off {
                    self.sensors = Sensors::Done;
                }
                self.set_run(rng, skill);
                self.set_warning(probe);
                events.push(DefenderEvent::Back);
                if self.warning == 0 {
                    events.push(self.catch(probe, None));
                    return events;
                }
                events.extend(self.respond());
            }
        } else if !self.off && !self.tasks.is_empty() && self.task_at.is_some_and(|t| now >= t) {
            events.extend(self.respond());
        }

        // The idle timer: a step after `delay` ticks without input (5
        // minutes while trapped).
        if !self.off {
            self.idle += dt;
            let delay = if self.trapped { DOWN_SECONDS } else { self.delay * TICK };
            if self.idle >= delay {
                self.idle = 0.0;
                events.extend(self.advance(rng, skill, probe));
            }
        }
        events
    }

    /// The first step at game start: pick a target (Lingo's InitGame).
    pub(crate) fn start(&mut self, rng: &mut Rng, skill: u8, probe: &Probe) {
        self.set_run(rng, skill);
        self.advance(rng, skill, probe);
    }

    /// Its planned path's lines, for tools and the debug overlay.
    pub fn plan(&self) -> Vec<String> {
        let mut line = self.line as i64;
        self.path
            .iter()
            .map(|j| {
                line += *j as i64;
                self.data.id(line.max(0) as usize).to_string()
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helixdb::Table;

    /// A made-up deck (not game data): a tree over nodes 1-3 with 2 at the
    /// root, each seeing itself; node 3 also sees node 1.
    fn data() -> Arc<DefenderData> {
        let def = Table::parse(
            "** Def\n\
             1.01,1,0,0,0,0,*1.01*,##,1\n\
             1.02,0,-1,1,1,3,*1.02*,##,1\n\
             1.03,-1,0,0,0,0,*1.03*1.01*,##,1",
        );
        // Node line 2 facing North reads DeathDB line 4 * (2 - 1) + 0 + 1 = 5.
        let deaths = Table::parse("*Death\n1.00,\n1.00,\n1.00,\n1.01,/1.02-1.03/,");
        Arc::new(DefenderData::new(&helixdb::defender::parse(&def), &helixdb::death::parse(&deaths)))
    }

    fn probe(id: &str, line: usize) -> Probe {
        Probe { line, id: id.into(), level: 1, facing: 0, door_open: false }
    }

    #[test]
    fn plans_through_the_tree_and_catches_in_sight() {
        let mut d = Defender::new(data());
        d.line = 2; // 1.01
        let p = probe("1.01", 2);
        let mut rng = Rng::new(1);
        d.plan_to("1.03", &mut rng, &p);
        assert_eq!(d.plan(), ["1.02", "1.03"]);
        // Its next step sees 1.02 only; the step after sees 1.01: warning 2.
        assert_eq!(d.warning, 2);
        assert_eq!(d.advance(&mut rng, 1, &p), []);
        assert_eq!(d.id(), "1.02");
        assert_eq!(d.warning, 1);
        // Into 1.03, which sees 1.01: caught, by the 1.02-1.03 death video.
        let events = d.advance(&mut rng, 1, &p);
        assert_eq!(events, [DefenderEvent::Caught(Some(("Navigation/L1".into(), "1.01.N.1".into())))]);
    }

    #[test]
    fn idle_steps_and_moves() {
        let mut d = Defender::new(data());
        d.line = 2;
        let p = probe("9.99", 0);
        let mut rng = Rng::new(1);
        d.plan_to("1.03", &mut rng, &p);
        d.delay = 60.0; // one second
        // Input keeps it still.
        for _ in 0..90 {
            d.input();
            d.tick(1.0 / 60.0, 0.0, &mut rng, 1, &p);
        }
        assert_eq!(d.id(), "1.01");
        for _ in 0..61 {
            d.tick(1.0 / 60.0, 0.0, &mut rng, 1, &p);
        }
        assert_eq!(d.id(), "1.02", "a step after a second idle");
        // At skill 5 it steps every second move.
        assert!(d.after_move(&mut rng, 5, &p).is_empty());
        assert_eq!(d.id(), "1.02");
        d.after_move(&mut rng, 5, &p);
        assert_eq!(d.id(), "1.03");
    }

    #[test]
    fn jammed_trapped_and_back() {
        let mut d = Defender::new(data());
        let p = probe("9.99", 0);
        let mut rng = Rng::new(1);
        d.jam(1);
        assert!(d.jammed);
        d.off = true;
        d.off_line("TurnDefOff", 0.0);
        assert_eq!(d.warning, 4);
        let events = d.tick(1.0, DOWN_SECONDS, &mut rng, 1, &p);
        assert!(events.contains(&DefenderEvent::Back));
        assert!(events.contains(&DefenderEvent::Responds("TurnDefOff".into())));
        assert!(!d.off);
        assert_eq!(d.line, START_LINE);
    }
}
