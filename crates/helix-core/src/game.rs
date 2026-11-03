//! One game in progress: navigation plus the probe's energy, scanner, DNA
//! kit, probe controls, arm and computers, and the phase (design doc §5-8,
//! §10.1, §13).

use std::sync::Arc;

use helixdb::NavGraph;
use helixdb::nav::ScanPoint;

use crate::access::KIT_SIZE;
use crate::arm::{Arm, ArmEvent};
use crate::computer::{Computer, ComputerButton, ComputerData};
use crate::nav::{NavAction, Navigator, Panel};
use crate::rng::Rng;
use crate::scan::{Scan, ScanEvent, ScanView};
use crate::Effect;

/// Energy units between empty and full (the bar's 399-473).
pub const ENERGY_FULL: i32 = 74;
/// Auto scan needs this much energy and costs 2.
const AUTO_SCAN_MIN: i32 = 3;
const AUTO_SCAN_COST: i32 = 2;
/// Scan indicator frames (node screen casts 107-111).
const BAR_FRAMES: u8 = 5;
/// The scan indicators' x positions, by direction relative to the facing.
const BAR_X: [i32; 4] = [399, 418, 436, 455];
/// Game length in minutes (§12.3).
pub const GAME_MINUTES: f32 = 90.0;
/// The defender's start (line 5); it stays there until its rules arrive.
const DEFENDER_START: usize = 5;

/// What the control screen shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    /// Node screen, door or elevator panel (see [`Navigator::panel`]).
    Nav(Panel),
    /// After enhancing: Acquire sample / Exit scanner.
    Scan,
    Controls(ControlsPage),
    /// Docked at a computer port (see [`Game::computer_view`]).
    Computer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlsPage {
    Menu,
    /// The DNA kit, with the samples selected for deletion.
    Inventory { selected: [bool; KIT_SIZE] },
}

/// A function button lit up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    Scan,
    Arm,
    Controls,
}

/// Buttons on the control screen's scan and probe-control interfaces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Acquire,
    ExitScanner,
    Inventory,
    AutoScan,
    Reboot,
    ExitControls,
    Previous,
    Delete,
    /// A sample in the inventory (0-2).
    Sample(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Nav(NavAction),
    /// The Scan function button.
    Scan,
    /// The probe controls (CTRL) function button.
    Controls,
    /// The Arm function button.
    Arm,
    /// A button on the computer screen.
    Computer(ComputerButton),
    /// Mouse down, drag and up in the view window.
    ViewPress(i32, i32),
    ViewDrag(i32, i32),
    ViewRelease,
    Press(Button),
}

/// What the computers know of the defender, until its own rules arrive.
#[derive(Debug, Clone)]
pub(crate) struct Defender {
    pub(crate) line: usize,
    pub(crate) off: bool,
    pub(crate) trapped: bool,
    pub(crate) door_sensors: bool,
    /// Tasks queued for it by computer actions (section 8.5).
    pub(crate) tasks: Vec<String>,
}

#[derive(Clone)]
pub struct Game {
    pub(crate) nav: Navigator,
    energy: i32,
    pub(crate) skill: u8,
    auto_scan: bool,
    scan: Option<Scan>,
    controls: Option<ControlsPage>,
    bars: [Option<u8>; 4],
    pub(crate) rng: Rng,
    arm: Option<Arm>,
    pub(crate) computer: Option<Computer>,
    pub(crate) data: Arc<ComputerData>,
    /// This game's video clues ("node-facing-NN+").
    pub(crate) clues: Vec<String>,
    /// Clues whose video has been seen ("NN+").
    pub(crate) clues_seen: Vec<String>,
    /// The player's notes, newest first.
    pub(crate) notes: Vec<String>,
    pub(crate) auth_code: String,
    pub(crate) mission_codes: Vec<String>,
    pub(crate) defender: Defender,
    /// Seconds played.
    pub(crate) elapsed: f32,
    pub(crate) won: bool,
}

impl Game {
    /// A new game: the probe at the start, auto scan on, energy full.
    /// `skill` (1-5) picks the scan table.
    pub fn new(nav: Navigator, skill: u8, seed: u64) -> Self {
        let mut game = Self {
            nav,
            energy: ENERGY_FULL,
            skill: skill.clamp(1, 5),
            auto_scan: true,
            scan: None,
            controls: None,
            bars: [None; 4],
            rng: Rng::new(seed),
            arm: None,
            computer: None,
            data: Arc::default(),
            clues: Vec::new(),
            clues_seen: Vec::new(),
            notes: Vec::new(),
            auth_code: String::new(),
            mission_codes: Vec::new(),
            defender: Defender { line: DEFENDER_START, off: false, trapped: false, door_sensors: true, tasks: Vec::new() },
            elapsed: 0.0,
            won: false,
        };
        game.show_bars();
        game
    }

    /// Add the computers: picks this game's video clues and codes.
    pub fn with_computers(mut self, data: ComputerData) -> Self {
        self.data = Arc::new(data);
        self.set_up_computers();
        self
    }

    /// The authorization code and the three mission codes.
    pub fn codes(&self) -> (&str, &[String]) {
        (&self.auth_code, &self.mission_codes)
    }

    /// The player's notes, newest first (video clue notes).
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// This game's video clues and those seen.
    pub fn clues(&self) -> (&[String], &[String]) {
        (&self.clues, &self.clues_seen)
    }

    pub fn won(&self) -> bool {
        self.won
    }

    /// The arm's cast and position, while it's out or moving.
    pub fn arm_view(&self) -> Option<(u16, (i32, i32))> {
        self.arm.as_ref().map(Arm::view)
    }

    /// Tasks queued for the defender (section 8.5).
    pub fn defender_tasks(&self) -> &[String] {
        &self.defender.tasks
    }

    pub fn nav(&self) -> &Navigator {
        &self.nav
    }

    pub fn graph(&self) -> &NavGraph {
        self.nav.graph()
    }

    pub fn energy(&self) -> i32 {
        self.energy
    }

    pub fn phase(&self) -> u8 {
        self.nav.phase()
    }

    pub fn skill(&self) -> u8 {
        self.skill
    }

    pub fn kit(&self) -> &[u8] {
        self.nav.access().kit()
    }

    pub fn auto_scan(&self) -> bool {
        self.auto_scan
    }

    pub fn interface(&self) -> Interface {
        if self.computer.is_some() {
            return Interface::Computer;
        }
        match (&self.scan, self.controls) {
            (_, Some(page)) => Interface::Controls(page),
            (Some(Scan::Enhanced(_)), _) => Interface::Scan,
            _ => Interface::Nav(self.nav.panel()),
        }
    }

    pub fn function(&self) -> Option<Function> {
        if self.scan.is_some() {
            Some(Function::Scan)
        } else if self.arm.as_ref().is_some_and(Arm::out) {
            Some(Function::Arm)
        } else {
            self.controls.map(|_| Function::Controls)
        }
    }

    /// The manual scan box is moving or locking on (for tools and tests
    /// that wait for it).
    pub fn scan_busy(&self) -> bool {
        self.scan.as_ref().is_some_and(Scan::busy) || self.arm.as_ref().is_some_and(Arm::busy)
    }

    pub fn scan_view(&self) -> ScanView {
        self.scan.as_ref().map(Scan::view).unwrap_or_default()
    }

    /// Scan indicators: (frame 0-4, x) for each direction with a scan
    /// point, hidden under the probe controls.
    pub fn scan_bars(&self) -> [Option<(u8, i32)>; 4] {
        let facing = self.nav.facing().index();
        std::array::from_fn(|d| {
            let frame = self.bars[d].filter(|_| self.controls.is_none() && self.computer.is_none())?;
            Some((frame, BAR_X[(4 + d - facing) % 4]))
        })
    }

    /// Testing aid: jump to a phase as if it had been reached.
    pub fn set_phase(&mut self, phase: u8) {
        self.nav.set_phase(phase);
    }

    /// The original's debug shortcut: grant DNA 01, 02 and 10 and skip to
    /// phase 2 (design doc §4.8).
    pub fn debug_phase_two(&mut self) -> Vec<Effect> {
        if self.phase() != 1 {
            return Vec::new();
        }
        *self.nav.access_mut().kit_mut() = vec![10, 2, 1];
        self.advance_phase(2)
    }

    /// The scan point when facing `d` here, from this skill's table.
    fn scan_point(&self, d: helixdb::Direction) -> Option<ScanPoint> {
        if self.nav.in_ladder() {
            return None;
        }
        self.nav.node().scan.get(self.skill as usize - 1)?[d.index()]
    }

    /// Lingo's DisplayScanIndicators: a random frame for each direction with
    /// a scan point.
    fn show_bars(&mut self) {
        for d in helixdb::Direction::ALL {
            self.bars[d.index()] = self.scan_point(d).map(|_| self.rng.random(BAR_FRAMES as u32) as u8 - 1);
        }
    }

    /// Effects that draw the current spot from scratch.
    pub fn redraw(&self) -> Vec<Effect> {
        self.nav.redraw()
    }

    pub fn act(&mut self, action: Action) -> Vec<Effect> {
        match action {
            Action::Nav(a) => self.navigate(a),
            Action::Scan if self.scan.is_some() => self.close_scan(),
            Action::Scan => self.open_scan(),
            Action::Controls if self.controls.is_some() => self.close_controls(),
            Action::Controls => self.open_controls(),
            Action::Arm if self.arm.as_ref().is_some_and(Arm::out) => self.close_arm(),
            Action::Arm => self.open_arm(),
            Action::Computer(b) => self.computer_press(b),
            Action::ViewPress(x, y) => {
                if let Some(arm) = self.arm.as_mut().filter(|a| a.out()) {
                    // Pressing again moves the arm, and leaves the computer.
                    arm.press(x, y);
                    let had_computer = self.computer.take().is_some();
                    return if had_computer { vec![Effect::ErasePanel, self.nav.info_clip()] } else { Vec::new() };
                }
                let event = self.scan.as_mut().and_then(|s| s.press(x, y));
                self.scan_events(event.into_iter().collect())
            }
            Action::ViewDrag(x, y) => {
                if let Some(s) = &mut self.scan {
                    s.drag(x, y);
                }
                if let Some(a) = &mut self.arm {
                    a.drag(x, y);
                }
                Vec::new()
            }
            Action::ViewRelease => {
                if let Some(s) = &mut self.scan {
                    s.release();
                }
                if let Some(a) = &mut self.arm {
                    a.release();
                }
                Vec::new()
            }
            Action::Press(b) => self.press(b),
        }
    }

    /// Advance animations by one tick ([`crate::scan::TICK`]).
    pub fn tick(&mut self) -> Vec<Effect> {
        // Lingo's AnimateScanBars: each shown bar drifts by -1, 0 or +1.
        for bar in self.bars.iter_mut().flatten() {
            let drift = self.rng.random(3) as i32 - 2;
            *bar = (*bar as i32 + drift).clamp(0, BAR_FRAMES as i32 - 1) as u8;
        }
        self.elapsed += crate::scan::TICK;
        let events = match &mut self.scan {
            Some(s) => s.tick(&mut self.rng),
            None => Vec::new(),
        };
        let mut fx = self.scan_events(events);
        let arm_events = self.arm.as_mut().map(Arm::tick).unwrap_or_default();
        for e in arm_events {
            match e {
                ArmEvent::Motion => fx.push(Effect::Sound("Arm Motion".into())),
                ArmEvent::Stop => fx.push(Effect::Sound("Arm Stop".into())),
                ArmEvent::HotSpot => fx.push(Effect::Sound("Arm HotSpot".into())),
                ArmEvent::Retracted => self.arm = None,
                // Docked at the port: clearance 02 opens the computer.
                ArmEvent::Docked if self.nav.access().grants("02") => fx.extend(self.open_computer()),
                ArmEvent::Docked => fx.extend(self.reject_computer()),
            }
        }
        fx
    }

    /// Lingo's InstallArm.
    fn open_arm(&mut self) -> Vec<Effect> {
        let here = format!("{}.{}", self.nav.node().id, self.nav.facing().index());
        if self.nav.in_ladder() || self.data.no_arm.contains(&here) {
            return vec![
                Effect::Sound("SysBeeb".into()),
                Effect::InfoClip { folder: "Misc Files".into(), name: "UnableDeploy".into() },
            ];
        }
        let mut fx = Vec::new();
        if self.scan.is_some() {
            fx.extend(self.close_scan());
        } else if self.controls.is_some() {
            fx.extend(self.close_controls());
        }
        let port = self.nav.node().ports[self.nav.facing().index()].map(|(h, v)| (h as i32, v as i32));
        self.arm = Some(Arm::deploy(port));
        fx
    }

    /// Lingo's DeInstallArm (and leaving the computer).
    pub(crate) fn close_arm(&mut self) -> Vec<Effect> {
        let had_computer = self.computer.take().is_some();
        if let Some(a) = &mut self.arm {
            a.retract();
        }
        self.show_bars();
        if had_computer { vec![Effect::ErasePanel, self.nav.info_clip()] } else { Vec::new() }
    }

    fn navigate(&mut self, action: NavAction) -> Vec<Effect> {
        let fx = self.nav.act(action);
        // Any movement movie closes the scanner, probe controls, arm and computer.
        if fx.iter().any(|e| matches!(e, Effect::Video { .. })) {
            self.scan = None;
            self.controls = None;
            self.arm = None;
            self.computer = None;
            self.show_bars();
        }
        fx
    }

    fn scan_events(&mut self, events: Vec<ScanEvent>) -> Vec<Effect> {
        let mut fx = Vec::new();
        for e in events {
            match e {
                ScanEvent::Blip => fx.push(Effect::Sound("Scan Blip".into())),
                ScanEvent::Enhance(p) => {
                    // Lingo's EnhanceScan.
                    self.scan = Some(Scan::Enhanced(p));
                    fx.push(Effect::video("Scan", format!("{:02}.ScanEnhance", p.item), Some("Enhance")));
                    fx.push(scan_text(&format!("{:02}.ScanTxt", p.dna)));
                }
            }
        }
        fx
    }

    /// Lingo's InstallScan.
    fn open_scan(&mut self) -> Vec<Effect> {
        let mut fx = if self.controls.is_some() { self.close_controls() } else { Vec::new() };
        if self.arm.is_some() {
            fx.extend(self.close_arm());
            self.arm = None;
        }
        let point = self.scan_point(self.nav.facing());
        if !self.auto_scan {
            self.scan = Some(Scan::manual(point));
            fx.push(Effect::Sound("ScanInit".into()));
        } else if self.energy < AUTO_SCAN_MIN {
            fx.push(Effect::Sound("SysBeeb".into()));
        } else {
            self.energy -= AUTO_SCAN_COST;
            self.scan = Some(Scan::auto(point));
            fx.push(Effect::Sound("AutoScan".into()));
            match point {
                Some(_) => fx.push(scan_text("PreScanPos")),
                None => fx.extend([Effect::Sound("SysBeeb".into()), scan_text("PreScanNeg")]),
            }
        }
        fx
    }

    /// Lingo's DeInstallScan: back to the navigation view and node screen.
    fn close_scan(&mut self) -> Vec<Effect> {
        let enhanced = matches!(self.scan, Some(Scan::Enhanced(_)));
        self.scan = None;
        self.show_bars();
        let mut fx = Vec::new();
        if enhanced {
            fx.push(Effect::RestoreView);
        }
        fx.push(self.nav.info_clip());
        fx
    }

    fn open_controls(&mut self) -> Vec<Effect> {
        let mut fx = if self.scan.is_some() { self.close_scan() } else { Vec::new() };
        if self.arm.is_some() {
            fx.extend(self.close_arm());
            self.arm = None;
        }
        self.controls = Some(ControlsPage::Menu);
        fx
    }

    fn close_controls(&mut self) -> Vec<Effect> {
        self.controls = None;
        self.show_bars();
        vec![self.nav.info_clip()]
    }

    /// The scan or probe-control button at a control-screen point, if any
    /// (Lingo's ScanInterface and CntrlClickOn).
    pub fn button_at(&self, x: i32, y: i32) -> Option<Button> {
        match self.interface() {
            Interface::Scan if (353..460).contains(&x) && (63..93).contains(&y) => {
                Some(if y < 75 { Button::Acquire } else { Button::ExitScanner })
            }
            Interface::Controls(page) => {
                let row = if (356..457).contains(&x) && y < 98 {
                    [(57..67), (73..83), (89..99)].iter().position(|r| r.contains(&y))
                } else {
                    None
                };
                if (121..131).contains(&y) && (452..485).contains(&x) {
                    return Some(Button::ExitControls);
                }
                match page {
                    ControlsPage::Menu => row.map(|r| [Button::Inventory, Button::AutoScan, Button::Reboot][r]),
                    ControlsPage::Inventory { .. } => {
                        if (121..131).contains(&y) && (414..447).contains(&x) {
                            Some(Button::Previous)
                        } else if (104..115).contains(&y) && (356..475).contains(&x) {
                            Some(Button::Delete)
                        } else {
                            row.map(Button::Sample)
                        }
                    }
                }
            }
            _ => None,
        }
    }

    fn press(&mut self, button: Button) -> Vec<Effect> {
        let enhanced = match self.scan {
            Some(Scan::Enhanced(p)) => Some(p),
            _ => None,
        };
        match (button, enhanced, self.controls) {
            (Button::Acquire, Some(p), _) => self.acquire(p),
            (Button::ExitScanner, Some(_), _) => self.close_scan(),
            (Button::Inventory, _, Some(ControlsPage::Menu)) => {
                self.controls = Some(ControlsPage::Inventory { selected: [false; KIT_SIZE] });
                Vec::new()
            }
            (Button::AutoScan, _, Some(ControlsPage::Menu)) => {
                self.auto_scan = !self.auto_scan;
                Vec::new()
            }
            (Button::ExitControls, _, Some(_)) => self.close_controls(),
            (Button::Previous, _, Some(ControlsPage::Inventory { .. })) => {
                self.controls = Some(ControlsPage::Menu);
                Vec::new()
            }
            (Button::Sample(i), _, Some(ControlsPage::Inventory { mut selected })) if i < self.kit().len() => {
                selected[i] = !selected[i];
                self.controls = Some(ControlsPage::Inventory { selected });
                Vec::new()
            }
            (Button::Delete, _, Some(ControlsPage::Inventory { selected })) => {
                let mut i = 0;
                self.nav.access_mut().kit_mut().retain(|_| {
                    i += 1;
                    !selected[i - 1]
                });
                self.controls = Some(ControlsPage::Inventory { selected: [false; KIT_SIZE] });
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    /// Lingo's AcquireSample.
    fn acquire(&mut self, p: ScanPoint) -> Vec<Effect> {
        let mut fx = Vec::new();
        let kit = self.nav.access().kit();
        if kit.len() >= KIT_SIZE {
            fx.extend([Effect::Sound("SysBeeb".into()), scan_text("Sample Bank Full")]);
        } else if kit.contains(&p.dna) {
            fx.extend([Effect::Sound("SysBeeb".into()), scan_text("Duplicate DNA")]);
        } else {
            fx.push(Effect::video("Scan", format!("{:02}.ScanAcquire", p.item), Some("Acquire")));
            fx.push(scan_text("Acquire Sucessful"));
            // DNA 12 is kept as 11: crew 12's inventory icon would be cast
            // 168-169, which the scan interface uses.
            self.nav.access_mut().kit_mut().insert(0, if p.dna == 12 { 11 } else { p.dna });
        }
        fx.push(Effect::Wait(1.0));
        fx.extend(self.close_scan());

        let kit = self.kit();
        if self.phase() == 1 && kit.contains(&1) && kit.contains(&2) && (kit.contains(&10) || kit.contains(&4)) {
            fx.extend(self.advance_phase(2));
        }
        fx
    }

    /// Lingo's AdvLevel: the phase movie on the bridge, then back to the
    /// game with full energy.
    pub(crate) fn advance_phase(&mut self, phase: u8) -> Vec<Effect> {
        self.nav.set_phase(phase);
        self.energy = ENERGY_FULL;
        self.scan = None;
        self.controls = None;
        self.arm = None;
        self.computer = None;
        self.show_bars();
        vec![
            Effect::Bridge { folder: "Opening".into(), name: format!("Phase{}", phase - 1) },
            Effect::RestoreView,
            self.nav.info_clip(),
        ]
    }
}

fn scan_text(name: &str) -> Effect {
    Effect::InfoClip { folder: "Scan/ScanText".into(), name: name.into() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Access;
    use helixdb::{Direction, Table};

    /// One node facing North with a scan point (item 7, DNA `dna`) in the
    /// skill 1 table; nothing in the others.
    fn game(dna: u8) -> Game {
        let node = Table::parse("** Node\n1.01,0,0,0,0,0,0,#,#,#,#,");
        let empty = Table::parse("** x");
        let scan = Table::parse(&format!("** Scan\n1.01,120.060.07.{dna:02},#,#,#,#,#,#,#,"));
        let graph = NavGraph::build(&node, &empty, &empty, &[scan]);
        let mut nav = Navigator::at(graph, 2, Direction::North);
        *nav.access_mut() = Access::new(vec![vec![1]]);
        Game::new(nav, 1, 1)
    }

    fn enhance(g: &mut Game) -> Vec<Effect> {
        g.act(Action::Scan);
        g.act(Action::ViewPress(121, 61))
    }

    #[test]
    fn auto_scan_enhance_and_acquire() {
        let mut g = game(2);
        assert_eq!(g.scan_bars()[0], Some((g.bars[0].unwrap(), 399)));
        let fx = g.act(Action::Scan);
        assert_eq!(g.energy(), ENERGY_FULL - 2);
        assert!(fx.contains(&scan_text("PreScanPos")));
        assert_eq!(g.function(), Some(Function::Scan));

        let fx = g.act(Action::ViewPress(121, 61));
        assert_eq!(fx[0], Effect::video("Scan", "07.ScanEnhance", Some("Enhance")));
        assert_eq!(fx[1], scan_text("02.ScanTxt"));
        assert_eq!(g.interface(), Interface::Scan);
        assert_eq!(g.button_at(400, 70), Some(Button::Acquire));

        let fx = g.act(Action::Press(Button::Acquire));
        assert_eq!(fx[0], Effect::video("Scan", "07.ScanAcquire", Some("Acquire")));
        assert!(fx.contains(&Effect::RestoreView));
        assert_eq!(g.kit(), [2]);
        assert_eq!(g.function(), None);

        // Again: a duplicate.
        enhance(&mut g);
        let fx = g.act(Action::Press(Button::Acquire));
        assert!(fx.contains(&scan_text("Duplicate DNA")));
        assert_eq!(g.kit(), [2]);
    }

    #[test]
    fn auto_scan_needs_energy_and_a_point() {
        let mut g = game(2);
        g.energy = 2;
        assert_eq!(g.act(Action::Scan), [Effect::Sound("SysBeeb".into())]);
        assert_eq!(g.function(), None);

        let mut g = game(2);
        g.nav.act(NavAction::TurnRight);
        let fx = g.act(Action::Scan);
        assert!(fx.contains(&scan_text("PreScanNeg")));
        assert_eq!(g.function(), Some(Function::Scan), "the scanner stays on");
        g.act(Action::Scan);
        assert_eq!(g.function(), None);
    }

    #[test]
    fn third_sample_starts_phase_two() {
        let mut g = game(10);
        g.nav.access_mut().kit_mut().extend([2, 1]);
        enhance(&mut g);
        let fx = g.act(Action::Press(Button::Acquire));
        assert_eq!(g.phase(), 2);
        assert!(fx.contains(&Effect::Bridge { folder: "Opening".into(), name: "Phase1".into() }));
        assert_eq!(g.kit(), [10, 2, 1]);
        assert_eq!(g.energy(), ENERGY_FULL);
    }

    #[test]
    fn full_kit_and_deleting_samples() {
        let mut g = game(5);
        g.nav.access_mut().kit_mut().extend([1, 2, 3]);
        enhance(&mut g);
        assert!(g.act(Action::Press(Button::Acquire)).contains(&scan_text("Sample Bank Full")));

        g.act(Action::Controls);
        assert_eq!(g.function(), Some(Function::Controls));
        assert_eq!(g.scan_bars(), [None; 4], "hidden under the controls");
        assert_eq!(g.button_at(400, 60), Some(Button::Inventory));
        g.act(Action::Press(Button::Inventory));
        assert_eq!(g.button_at(400, 78), Some(Button::Sample(1)));
        g.act(Action::Press(Button::Sample(1)));
        assert_eq!(g.interface(), Interface::Controls(ControlsPage::Inventory { selected: [false, true, false] }));
        g.act(Action::Press(Button::Delete));
        assert_eq!(g.kit(), [1, 3]);
        g.act(Action::Press(Button::ExitControls));
        assert_eq!(g.interface(), Interface::Nav(Panel::Node));
    }

    #[test]
    fn auto_scan_can_be_turned_off() {
        let mut g = game(2);
        g.act(Action::Controls);
        g.act(Action::Press(Button::AutoScan));
        assert!(!g.auto_scan());
        g.act(Action::Controls);
        let fx = g.act(Action::Scan);
        assert_eq!(fx.last(), Some(&Effect::Sound("ScanInit".into())));
        assert_eq!(g.energy(), ENERGY_FULL, "manual scanning is free");
        assert!(g.scan_view().scan_box.is_some());
    }
}
