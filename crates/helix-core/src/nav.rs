//! Moving the probe around the ship (design doc §4).
//!
//! The probe stands on a node (a line of the node tables) facing N/E/S/W.
//! Moves play a movie and step the line by the exit's offset; turns play a
//! movie and change the facing; ladders and elevators change level. Doors
//! open and close in place when the probe has clearance (§5.2); `00` doors
//! never open, and the battle-damaged doors to the PC release's hidden rooms
//! wait for their phase.

use helixdb::Direction;
use helixdb::nav::{Ladder, NavGraph, NavNode};

use crate::Effect;
use crate::access::Access;

/// Where the probe starts: line 87 (node 4.04) facing North.
pub const START_LINE: usize = 87;

/// Ticks are 1/60 s.
const TICK: f32 = 1.0 / 60.0;

/// The garbage chute: this door (node 5.04, facing West) opens a way down.
const CHUTE_NODE: &str = "5.04";
const CHUTE_DOWN: &str = "3.019.1.6";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavAction {
    Forward,
    Back,
    TurnLeft,
    TurnRight,
    Up,
    Down,
    /// The door panel's button (or Enter).
    Door,
    /// An elevator floor button.
    Elevator(u8),
    /// Tab / double-arrow mode: the next move repeats until something stops it.
    ToggleRepeat,
}

/// Which navigation buttons are available.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct NavButtons {
    pub up: bool,
    pub down: bool,
    pub forward: bool,
    pub back: bool,
    pub left: bool,
    pub right: bool,
}

/// What the control screen shows for this spot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    /// The node screen (no door ahead).
    Node,
    Door { open: bool },
    Elevator { open: bool, level: u8 },
}

#[derive(Clone)]
pub struct Navigator {
    graph: NavGraph,
    line: usize,
    facing: Direction,
    in_ladder: bool,
    door_open: bool,
    repeat: bool,
    chute_open: bool,
    /// Phase and DNA kit, which doors check.
    access: Access,
}

/// The kind of move a button makes (forward/back/up/down).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Step {
    Forward,
    Back,
    Up,
    Down,
}

/// A move worked out from the current spot: the line offset, the movie
/// suffix, the sound, and (for ladders) the facing afterwards.
struct Plan {
    offset: i32,
    movie: String,
    sound: String,
    facing: Option<Direction>,
}

impl Navigator {
    pub fn new(graph: NavGraph) -> Self {
        Self::at(graph, START_LINE, Direction::North)
    }

    pub fn at(graph: NavGraph, line: usize, facing: Direction) -> Self {
        Self { graph, line, facing, in_ladder: false, door_open: false, repeat: false, chute_open: false, access: Access::default() }
    }

    pub fn graph(&self) -> &NavGraph {
        &self.graph
    }

    pub fn node(&self) -> &NavNode {
        self.graph.at_line(self.line).expect("the probe always stands on a node")
    }

    pub fn line(&self) -> usize {
        self.line
    }

    pub fn facing(&self) -> Direction {
        self.facing
    }

    pub fn in_ladder(&self) -> bool {
        self.in_ladder
    }

    pub fn repeat(&self) -> bool {
        self.repeat
    }

    pub fn level(&self) -> u8 {
        self.node().level()
    }

    pub fn phase(&self) -> u8 {
        self.access.phase()
    }

    pub fn set_phase(&mut self, phase: u8) {
        self.access.set_phase(phase);
    }

    pub fn access(&self) -> &Access {
        &self.access
    }

    pub fn access_mut(&mut self) -> &mut Access {
        &mut self.access
    }

    /// Everything that tells one navigation state from another (for
    /// exhaustively exploring the ship in tests and tools).
    pub fn state_key(&self) -> (usize, Direction, bool, bool, bool, u8) {
        (self.line, self.facing, self.in_ladder, self.door_open, self.chute_open, self.phase())
    }

    /// A battle-damaged door (PC release) opens on deck 2 from phase 2 and on
    /// deck 4 in phase 3 only, as `HELIX.EXE` checks it (design doc §14.4).
    fn damage_cleared(&self) -> bool {
        match self.level() {
            2 => self.phase() >= 2,
            4 => self.phase() == 3,
            _ => false,
        }
    }

    fn door_ahead(&self) -> Option<&str> {
        self.node().door(self.facing)
    }

    fn door_behind(&self) -> Option<&str> {
        self.node().door(self.facing.opposite())
    }

    fn up(&self) -> Option<Ladder> {
        self.node().up
    }

    /// The down ladder, or the garbage chute while its door is open.
    fn down(&self) -> Option<Ladder> {
        if self.chute_open { Ladder::parse(CHUTE_DOWN) } else { self.node().down }
    }

    pub fn buttons(&self) -> NavButtons {
        let n = self.node();
        let f = self.facing;
        NavButtons {
            up: self.up().is_some_and(|l| l.from == f),
            down: self.down().is_some_and(|l| l.from == f),
            forward: (self.door_ahead().is_none() || self.door_open) && n.exit(f).is_some(),
            back: self.door_behind().is_none() && n.exit(f.opposite()).is_some(),
            left: !self.in_ladder,
            right: !self.in_ladder,
        }
    }

    pub fn panel(&self) -> Panel {
        if self.door_ahead().is_none() {
            Panel::Node
        } else if self.node().is_elevator() {
            Panel::Elevator { open: self.door_open, level: self.level() }
        } else {
            Panel::Door { open: self.door_open }
        }
    }

    fn folder(level: impl std::fmt::Display) -> String {
        format!("Navigation/L{level}")
    }

    /// The info strip clip for this spot (location label).
    pub fn info_clip(&self) -> Effect {
        Effect::InfoClip {
            folder: Self::folder(self.level()),
            name: format!("{}.T.{}", self.node().id, self.facing.index()),
        }
    }

    /// Work out a move from the current spot (Lingo's WhichNavMove).
    fn plan(&self, step: Step) -> Option<Plan> {
        let n = self.node();
        let dir = self.facing.letter();
        let exit_move = |e: Option<helixdb::nav::Exit>, letter: char| {
            e.map(|e| Plan {
                offset: e.offset,
                movie: if self.in_ladder { letter.to_string() } else { format!("{letter}{dir}") },
                sound: format!("ProbeMove{}", e.sound),
                facing: None,
            })
        };
        let ladder_move = |l: Option<Ladder>, letter: char, sound: &str| {
            l.map(|l| Plan { offset: l.offset, movie: letter.to_string(), sound: sound.into(), facing: Some(l.facing) })
        };
        match step {
            Step::Forward => exit_move(n.exit(self.facing), 'F'),
            Step::Back => exit_move(n.exit(self.facing.opposite()), 'B'),
            Step::Up => ladder_move(self.up(), 'U', "ProbeMoveUp"),
            Step::Down => ladder_move(self.down(), 'D', "ProbeMoveDown"),
        }
    }

    /// The science lab's curved corridor turns the probe as it moves.
    fn science_lab(&mut self, step: Step) {
        let id = self.node().id.as_str();
        let f = self.facing;
        use Direction::*;
        self.facing = match (id, step, f) {
            ("2.14", Step::Forward, East) => South,
            ("2.14", Step::Back, West) => North,
            ("2.15", Step::Back, South) => East,
            ("2.15", Step::Forward, North) => West,
            _ => f,
        };
    }

    fn step_line(&mut self, offset: i32) {
        if let Some(next) = self.graph.step(self.node(), offset) {
            self.line = next.line;
        }
    }

    /// Effects that draw the current spot from scratch (entering play or
    /// loading a game): the original plays the turn that ends on the
    /// current facing, then the location clip.
    pub fn redraw(&self) -> Vec<Effect> {
        let node = if self.node().is_elevator() { "0".to_string() } else { self.node().id.clone() };
        let folder = Self::folder(node.chars().next().unwrap_or('0'));
        vec![
            Effect::video(folder, format!("{node}.R{}", self.facing.left()), None),
            self.info_clip(),
        ]
    }

    /// Apply a player action; returns what to show and play, in order.
    /// Unavailable actions do nothing.
    pub fn act(&mut self, action: NavAction) -> Vec<Effect> {
        let b = self.buttons();
        match action {
            NavAction::Forward if b.forward => self.travel(Step::Forward),
            NavAction::Back if b.back => self.travel(Step::Back),
            NavAction::Up if b.up => self.travel(Step::Up),
            NavAction::Down if b.down => self.travel(Step::Down),
            NavAction::TurnLeft if b.left => self.turn(false),
            NavAction::TurnRight if b.right => self.turn(true),
            NavAction::Door if self.door_ahead().is_some() => self.toggle_door(),
            // The floor buttons are on the elevator panel, shown when facing its door.
            NavAction::Elevator(level) if self.door_ahead().is_some() => self.ride(level),
            NavAction::Elevator(_) => Vec::new(),
            NavAction::ToggleRepeat => {
                self.repeat = !self.repeat;
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Forward, back, up or down (Lingo's Navigation).
    fn travel(&mut self, step: Step) -> Vec<Effect> {
        let mut fx = Vec::new();
        let Some(mut plan) = self.plan(step) else { return fx };
        if let Some(f) = plan.facing {
            self.facing = f;
        }
        self.science_lab(step);

        // Backing away from an open door ahead closes it (with its movie).
        if step == Step::Back && self.door_open {
            fx.extend(self.toggle_door());
        }
        if matches!(step, Step::Up | Step::Down) && !self.in_ladder {
            self.in_ladder = true;
        }

        if self.repeat {
            self.door_open = false;
            let mut previous_facing;
            loop {
                fx.push(Effect::video(Self::folder(self.level()), format!("{}.{}", self.node().id, plan.movie), Some(&plan.sound)));
                self.step_line(plan.offset);
                previous_facing = self.facing;
                // Keep going the same way while the next spot allows it.
                let Some(next) = self.plan(step) else { break };
                if let Some(f) = next.facing {
                    self.facing = f;
                }
                self.science_lab(step);
                let blocked = match step {
                    Step::Forward => self.door_ahead().is_some(),
                    Step::Back => self.door_behind().is_some(),
                    Step::Up | Step::Down => !self.node().is_ladder() || self.node().id == "5.09.L",
                };
                if blocked {
                    break;
                }
                plan = next;
            }
            self.repeat = false;
            self.facing = previous_facing;
        } else {
            fx.push(Effect::video(Self::folder(self.level()), format!("{}.{}", self.node().id, plan.movie), Some(&plan.sound)));
            if self.door_open {
                // Moving through (or away from) an open door closes it behind.
                self.door_open = false;
                fx.push(Effect::Sound("Air Latch Close".into()));
            }
            self.step_line(plan.offset);
        }

        if !self.node().is_ladder() {
            self.in_ladder = false;
        }
        self.chute_open = false;
        fx.push(self.info_clip());
        fx
    }

    /// Turn left or right (Lingo's Rotate).
    fn turn(&mut self, right: bool) -> Vec<Effect> {
        let mut fx = Vec::new();
        if self.door_open {
            fx.extend(self.toggle_door());
        }
        // Elevator interiors share one set of turn movies under node "0".
        let node = if self.node().is_elevator() { "0".to_string() } else { self.node().id.clone() };
        let folder = Self::folder(node.chars().next().unwrap_or('0'));
        let turns = if self.repeat { 2 } else { 1 };
        for _ in 0..turns {
            let name = format!("{node}.{}{}", if right { 'R' } else { 'L' }, self.facing);
            fx.push(Effect::video(&folder, name, Some("Probe Rotate")));
            self.facing = if right { self.facing.right() } else { self.facing.left() };
        }
        self.repeat = false;
        fx.push(self.info_clip());
        fx
    }

    /// Open or close the door ahead (Lingo's DoorAction).
    fn toggle_door(&mut self) -> Vec<Effect> {
        let Some(code) = self.door_ahead().map(str::to_string) else { return Vec::new() };
        let mut fx = Vec::new();
        let is_chute = self.node().id == CHUTE_NODE && self.facing == Direction::West;
        let (movie, sound) = if !self.door_open {
            let damaged = self.node().damaged[self.facing.index()];
            if code == "00" || (damaged && !self.damage_cleared()) {
                fx.push(Effect::Sound("SysBeeb".into()));
                fx.push(Effect::InfoClip { folder: "Messages".into(), name: "LockedDoorsTxt".into() });
                return fx;
            }
            if !self.access.grants(&code) {
                fx.push(Effect::Sound("Access Denied".into()));
                fx.push(Effect::InfoClip { folder: "Messages".into(), name: "AccessDeniedDNA.Mssg".into() });
                return fx;
            }
            self.door_open = true;
            ('O', if is_chute { "GarbageChute Open" } else { "Air Latch Open" })
        } else {
            self.door_open = false;
            ('C', "Air Latch Close")
        };
        if is_chute {
            self.chute_open = self.door_open;
        }
        fx.push(Effect::video(
            Self::folder(self.level()),
            format!("{}.{movie}{}", self.node().id, self.facing),
            Some(sound),
        ));
        fx
    }

    /// Ride the elevator to another level (Lingo's OperateElevator).
    fn ride(&mut self, level: u8) -> Vec<Effect> {
        let mut fx = Vec::new();
        if !self.node().is_elevator() || level == self.level() || !(1..=6).contains(&level) {
            return fx;
        }
        if self.door_open {
            fx.extend(self.toggle_door());
        }
        fx.push(Effect::Sound("Elevator2".into()));
        while self.level() != level {
            fx.push(Effect::Wait(100.0 * TICK));
            let offset = if level > self.level() { self.node().elevator_up } else { self.node().elevator_down };
            let Some(offset) = offset else { break };
            let before = self.line;
            self.step_line(offset);
            if self.line == before {
                break;
            }
        }
        fx.push(Effect::Wait(40.0 * TICK));
        fx.extend(self.toggle_door()); // arrived: the door opens
        fx.push(self.info_clip());
        fx
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use helixdb::Table;

    /// A made-up little ship (not game data):
    ///
    /// line 2 `1.01` --N--> line 3 `1.02` (door "01" north of 1.02) --N--> line 4 `1.03`
    /// 1.02 has a ladder down (facing West) to line 6 `2.1.1.L`, which leads down to line 7 `2.01`.
    fn ship() -> NavGraph {
        let node = Table::parse(
            "** Node\n\
             1.01,1,0,0,0,0,0,#,#,#,#,\n\
             1.02,1,0,-1,0,0,3.003.2.2,01,#,#,#,\n\
             1.03,0,0,-1,0,0,0,#,#,01,#,\n\
             -- ladders --\n\
             2.1.1.L,0,0,0,0,0,2.001.2.2,#,#,#,#,\n\
             2.01,0,0,0,0,0,0,#,#,#,#,",
        );
        let empty = Table::parse("** x");
        NavGraph::build(&node, &empty, &empty, &[])
    }

    #[test]
    fn walking_and_buttons() {
        let mut n = Navigator::at(ship(), 2, Direction::North);
        let b = n.buttons();
        assert!(b.forward && !b.back && !b.up && b.left);
        let fx = n.act(NavAction::Forward);
        assert_eq!(fx[0], Effect::video("Navigation/L1", "1.01.FN", Some("ProbeMove1")));
        assert_eq!(n.node().id, "1.02");
        assert!(matches!(fx.last(), Some(Effect::InfoClip { name, .. }) if name == "1.02.T.0"));
        // A closed door ahead blocks forward and shows the door panel.
        assert!(!n.buttons().forward);
        assert_eq!(n.panel(), Panel::Door { open: false });
        assert!(n.act(NavAction::Forward).is_empty());
    }

    #[test]
    fn doors_open_and_close_behind() {
        let mut n = Navigator::at(ship(), 3, Direction::North);
        let fx = n.act(NavAction::Door);
        assert_eq!(fx[0], Effect::video("Navigation/L1", "1.02.ON", Some("Air Latch Open")));
        assert!(n.buttons().forward);
        let fx = n.act(NavAction::Forward);
        assert!(fx.contains(&Effect::Sound("Air Latch Close".into())));
        assert_eq!(n.node().id, "1.03");
        // Now the door is behind: no backing through it.
        assert!(!n.buttons().back);
    }

    #[test]
    fn turning_and_ladders() {
        let mut n = Navigator::at(ship(), 3, Direction::North);
        let fx = n.act(NavAction::TurnLeft);
        assert_eq!(fx[0], Effect::video("Navigation/L1", "1.02.LN", Some("Probe Rotate")));
        assert_eq!(n.facing(), Direction::West);
        assert!(n.buttons().down);
        let fx = n.act(NavAction::Down);
        assert_eq!(fx[0], Effect::video("Navigation/L1", "1.02.D", Some("ProbeMoveDown")));
        assert_eq!(n.node().id, "2.1.1.L");
        assert!(n.in_ladder());
        assert!(!n.buttons().left, "no turning on a ladder");
        n.act(NavAction::Down);
        assert_eq!(n.node().id, "2.01");
        assert!(!n.in_ladder());
    }

    #[test]
    fn repeat_moves_until_blocked() {
        let mut n = Navigator::at(ship(), 2, Direction::North);
        n.act(NavAction::ToggleRepeat);
        let fx = n.act(NavAction::Forward);
        // Stops at 1.02 because of the closed door ahead.
        assert_eq!(n.node().id, "1.02");
        assert_eq!(fx.iter().filter(|e| matches!(e, Effect::Video { .. })).count(), 1);
        assert!(!n.repeat());
    }

    #[test]
    fn locked_doors_stay_shut() {
        let node = Table::parse("** Node\n1.01,0,0,0,0,0,0,00,#,#,#,");
        let empty = Table::parse("** x");
        let mut n = Navigator::at(NavGraph::build(&node, &empty, &empty, &[]), 2, Direction::North);
        let fx = n.act(NavAction::Door);
        assert_eq!(fx[0], Effect::Sound("SysBeeb".into()));
        assert_eq!(n.panel(), Panel::Door { open: false });
    }

    #[test]
    fn doors_need_clearance() {
        let node = Table::parse("** Node
1.01,0,0,0,0,0,0,03,#,#,#,");
        let empty = Table::parse("** x");
        let mut n = Navigator::at(NavGraph::build(&node, &empty, &empty, &[]), 2, Direction::North);
        *n.access_mut() = Access::new(vec![vec![1], vec![1, 7]]);
        let fx = n.act(NavAction::Door);
        assert_eq!(fx[0], Effect::Sound("Access Denied".into()));
        assert_eq!(n.panel(), Panel::Door { open: false });
        n.access_mut().kit_mut().push(7);
        n.act(NavAction::Door);
        assert_eq!(n.panel(), Panel::Door { open: true });
    }

    #[test]
    fn damaged_doors_wait_for_their_phase() {
        let node = Table::parse("** Node
2.01,0,0,0,0,0,0,06,#,#,#,
4.01,0,0,0,0,0,0,06,#,#,#,");
        let empty = Table::parse("** x");
        let mut g = NavGraph::build(&node, &empty, &empty, &[]);
        for line in [2, 3] {
            g.node_mut(line).unwrap().damaged[0] = true;
        }
        let opens = |line: usize, phase: u8| {
            let mut n = Navigator::at(g.clone(), line, Direction::North);
            n.set_phase(phase);
            n.act(NavAction::Door);
            n.panel() == Panel::Door { open: true }
        };
        assert_eq!([1, 2, 3, 4].map(|p| opens(2, p)), [false, true, true, true]);
        assert_eq!([1, 2, 3, 4].map(|p| opens(3, p)), [false, false, true, false]);
    }
}
