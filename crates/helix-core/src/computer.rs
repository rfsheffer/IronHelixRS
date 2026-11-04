//! The ship's computers (design doc §8, Lingo `Comp` and `Auth`).
//!
//! The Cmp table is read by line number, as the scripts do. A record starts
//! with a header line (`Menu|TravMenu|Action,<screen>,<Return|NoErase>`) and
//! its option line; an `Action` record also has lines for clue-gated codes,
//! win actions, movie actions and tasks. Options are `Hier(NNN).CC` (open the
//! record after line NNN, clearance CC), `Movi(NAME)`, `Auth.CC`, `XCmp` and
//! `Trav`; items 1-4 are the menu rows, 5 Prev, 6 Exit, 7 Back, 8 Forward.

use crate::Effect;
use crate::defender::Sensors;
use crate::game::Game;

/// Where computer clips play: over the control screen.
const PANEL: &str = "Computer";
/// Keypad digits allowed.
const CODE_LENGTH: usize = 7;

/// What the computers need from the game data.
#[derive(Debug, Clone, Default)]
pub struct ComputerData {
    /// The Cmp table, line 1 first.
    pub cmp: Vec<String>,
    /// `node.facing` spots where the arm can't deploy.
    pub no_arm: Vec<String>,
    /// The two video clue lists; line `skill * 2` of one is used.
    pub clue_lists: Vec<String>,
    /// Note line for each video clue (line N = clue N).
    pub clue_notes: Vec<String>,
    /// The notes before any clue ("No Video Clues Recorded.").
    pub notes_default: String,
}

/// The computer's state while the probe is docked.
#[derive(Debug, Clone)]
pub(crate) struct Computer {
    /// Header line of the current record.
    record: usize,
    /// Option line shown (paged by Trav).
    options: usize,
    /// Base for the location lines (LocationPos).
    location: usize,
    /// Screen cast offset from the record's named cast (paged by Trav).
    page: i32,
    /// A NoErase clip is up: only Prev and Exit work.
    clip_up: bool,
    video_up: bool,
    /// The video clue at this port, e.g. "02+".
    message: Option<String>,
    /// Keypad digits while the Auth screen shows.
    auth: Option<String>,
    /// The last item chosen (CompItemSelect).
    item: usize,
    /// The ETA ship's position, after the ETA clip.
    eta: Option<(i32, i32)>,
}

/// What the computer screen shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ComputerView {
    /// Screen cast by name and page offset; None while a video plays
    /// (`EmptyScreen`).
    pub screen: Option<(String, i32)>,
    /// The Message button is lit (a video clue is here).
    pub message: bool,
    /// Keypad digits, on the Auth screen.
    pub digits: Option<String>,
    /// Text shown on the screen: the codes on the Mission pages.
    pub texts: Vec<(String, (i32, i32))>,
    /// The ship icon on the ETA clip.
    pub eta: Option<(i32, i32)>,
    pub video_up: bool,
    /// A menu record (its rows light square) rather than an action screen.
    pub menu: bool,
}

/// A button on the computer screen (see the module docs for items).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComputerButton {
    /// Items 1-8, 9 Message, 13 play the video again, 14 continue.
    Item(usize),
    /// Keypad: digits 0-9.
    Key(u8),
    Clear,
    Enter,
    Cancel,
}

/// Lingo `item n of s` (1-based, comma-separated).
fn item(s: &str, n: usize) -> &str {
    s.split(',').nth(n.wrapping_sub(1)).unwrap_or("")
}

/// Lingo `chars(s, a, b)` (1-based, inclusive).
fn chars(s: &str, a: usize, b: usize) -> String {
    s.chars().skip(a.saturating_sub(1)).take((b + 1).saturating_sub(a)).collect()
}

impl ComputerData {
    fn line(&self, n: usize) -> &str {
        self.cmp.get(n.wrapping_sub(1)).map_or("", String::as_str)
    }

    /// Codes the scripts treat as taken (they appear in the game's text), so
    /// generated codes avoid them.
    fn used(&self, code: &str) -> bool {
        self.cmp.iter().chain(&self.clue_notes).any(|l| l.contains(code))
    }
}

impl Computer {
    /// Lingo's InstallComputer: the Main menu.
    fn main(message: Option<String>) -> Self {
        Self { record: 2, options: 3, location: 3, page: 0, clip_up: false, video_up: false, message, auth: None, item: 0, eta: None }
    }
}

impl Game {
    /// Pick this game's video clues and codes (Lingo's CreateCodes and
    /// the clue list choice).
    pub(crate) fn set_up_computers(&mut self) {
        let lists = self.data.clue_lists.len();
        if lists > 0 {
            let list = &self.data.clue_lists[self.rng.random(lists as u32) as usize - 1];
            let line = list.lines().nth(self.skill as usize * 2 - 1).unwrap_or("");
            self.clues = line.split(',').map(str::trim).filter(|c| !c.is_empty()).map(str::to_string).collect();
        }
        let mut codes: Vec<String> = Vec::new();
        while codes.len() < 4 {
            let code = (self.rng.random(9000) + 999).to_string();
            if !self.data.used(&code) && !codes.contains(&code) {
                codes.push(code);
            }
        }
        self.auth_code = codes.remove(0);
        self.mission_codes = codes;
    }

    fn header(&self) -> &str {
        self.computer.as_ref().map_or("", |c| self.data.line(c.record))
    }

    fn menu_type(&self) -> String {
        item(self.header(), 1).to_string()
    }

    fn menu_name(&self) -> String {
        item(self.header(), 2).to_string()
    }

    /// The video clue at this port, when phase > 1.
    fn message_here(&self) -> Option<String> {
        if self.phase() < 2 {
            return None;
        }
        let here = format!("{}-{}", self.nav.node().id, self.nav.facing().index());
        let clue = self.clues.iter().find(|c| c.contains(&here))?;
        // "1.05-1-02+": the number and mark after the second dash.
        Some(chars(clue, here.len() + 2, here.len() + 4))
    }

    pub(crate) fn open_computer(&mut self) -> Vec<Effect> {
        self.computer = Some(Computer::main(self.message_here()));
        Vec::new()
    }

    /// Lingo's RejectComputer: the "access denied" clip, then the node screen.
    pub(crate) fn reject_computer(&mut self) -> Vec<Effect> {
        vec![
            Effect::Panel { folder: PANEL.into(), name: "AccessDeniedToCmp".into(), keep: false },
            Effect::Wait(1.0),
            self.nav.info_clip(),
        ]
    }

    pub fn computer_view(&self) -> Option<ComputerView> {
        let c = self.computer.as_ref()?;
        let name = self.menu_name();
        let screen = if c.video_up {
            None
        } else if c.auth.is_some() {
            Some(("Auth".to_string(), 0))
        } else {
            Some((name.clone(), c.page))
        };
        // The Mission pages show the generated codes.
        let page_name = match (name.as_str(), c.page) {
            ("Mission", 1) => "Mission 1",
            ("Mission", 2) => "Mission 2",
            _ => "",
        };
        let texts = match (page_name, c.auth.is_some() || c.video_up) {
            ("Mission 1", false) => vec![(self.auth_code.clone(), (351, 108))],
            // One code in each of the page's three boxes.
            ("Mission 2", false) => self.mission_codes.iter().zip([350, 400, 451]).map(|(code, x)| (code.clone(), (x, 74))).collect(),
            _ => Vec::new(),
        };
        Some(ComputerView {
            message: c.message.is_some() && name == "Main" && !c.video_up && c.auth.is_none(),
            screen,
            digits: c.auth.clone(),
            texts,
            eta: c.eta,
            video_up: c.video_up,
            menu: self.menu_type().contains("Menu"),
        })
    }

    /// The computer button at a control-screen point (Lingo's
    /// MenuItemSelected and the keypad areas of EnterAuth).
    pub fn computer_button_at(&self, x: i32, y: i32) -> Option<ComputerButton> {
        let c = self.computer.as_ref()?;
        if c.auth.is_some() {
            if (352..396).contains(&x) && (70..125).contains(&y) {
                let n = (y - 69) / 14 * 3 + (x - 351) / 16 + 1;
                return Some(match n {
                    1..=9 => ComputerButton::Key(n as u8),
                    10 => ComputerButton::Key(0),
                    _ => ComputerButton::Clear,
                });
            }
            if (415..471).contains(&x) && (91..126).contains(&y) {
                return Some(if y > 108 { ComputerButton::Cancel } else { ComputerButton::Enter });
            }
            return None;
        }
        let selected = if (129..142).contains(&y) {
            if c.video_up {
                match x {
                    358..=423 => 13,
                    427..=491 => 14,
                    _ => return None,
                }
            } else if x > 459 {
                6
            } else if x > 427 {
                5
            } else if (345..401).contains(&x) && self.menu_name() == "Main" && c.message.is_some() {
                9
            } else if (345..377).contains(&x) {
                7
            } else if (378..411).contains(&x) {
                8
            } else {
                return None;
            }
        } else if (72..124).contains(&y) {
            ((y - 71) / 14 + 1) as usize
        } else {
            return None;
        };
        if c.clip_up {
            return matches!(selected, 5 | 6).then_some(ComputerButton::Item(selected));
        }
        if selected == 9 || c.video_up {
            return Some(ComputerButton::Item(selected));
        }
        let option = item(self.data.line(c.options), selected);
        (option != "#").then_some(ComputerButton::Item(selected))
    }

    /// Lingo's OperateComputer and EnterAuth.
    pub(crate) fn computer_press(&mut self, button: ComputerButton) -> Vec<Effect> {
        let Some(c) = &mut self.computer else { return Vec::new() };
        if c.auth.is_some() {
            return self.keypad(button);
        }
        let ComputerButton::Item(selected) = button else { return Vec::new() };
        c.item = selected;
        let option = item(self.data.line(c.options), selected).to_string();
        let clip_up = c.clip_up;
        match selected {
            5 if clip_up => self.erase(),
            9 | 13 => self.play_video(selected == 9),
            14 => {
                let c = self.computer.as_mut().unwrap();
                c.video_up = false;
                Vec::new()
            }
            _ if selected >= 5 || self.menu_type() != "Menu" => self.run_option(&option),
            _ => {
                let code = chars(&option, 11, 12);
                if !self.nav.access().grants(&code) {
                    let mut fx = self.action("AccessDeniedDNA", Task::None);
                    fx.extend(self.erase());
                    return fx;
                }
                if !self.in_location(selected) {
                    let mut fx = self.action("AccessDeniedLoc", Task::None);
                    fx.extend(self.erase());
                    return fx;
                }
                self.run_option(&option)
            }
        }
    }

    /// Lingo's InLocation: rows 1-4 may be limited to certain ports.
    fn in_location(&self, selected: usize) -> bool {
        let Some(c) = &self.computer else { return false };
        let allowed = item(self.data.line(c.location + selected), 1);
        let here = format!("{}.{}", self.nav.node().id, self.nav.facing().index());
        allowed == "#" || allowed.contains(&here)
    }

    /// Lingo's PacoErase: clear a clip off the screen.
    fn erase(&mut self) -> Vec<Effect> {
        if let Some(c) = &mut self.computer {
            c.clip_up = false;
            c.eta = None;
            if c.auth.is_some() {
                c.auth = Some(String::new());
            }
        }
        vec![Effect::ErasePanel]
    }

    /// Run an option's function: Hier, Movi, Auth, XCmp or Trav.
    fn run_option(&mut self, option: &str) -> Vec<Effect> {
        let no_erase = item(self.header(), 3) == "NoErase";
        let c = self.computer.as_mut().unwrap();
        match chars(option, 1, 4).as_str() {
            "Hier" => {
                let target: usize = chars(option, 6, 8).trim().parse().unwrap_or(0);
                c.record = target + 1;
                c.options = target + 2;
                c.location = target + 2;
                c.page = 0;
                c.eta = None;
                vec![Effect::ErasePanel, Effect::Sound("Menu Sound".into())]
            }
            "Movi" => {
                let clip = chars(option, 6, 11);
                let task = if clip == "CPUETA" { Task::Funct } else { Task::None };
                let fx = self.action(&clip, task);
                if no_erase {
                    self.computer.as_mut().unwrap().clip_up = true;
                }
                fx
            }
            "Auth" => {
                c.auth = Some(String::new());
                vec![Effect::ErasePanel, Effect::Sound("Menu Sound".into())]
            }
            "XCmp" => self.close_arm(),
            "Trav" => {
                let back = c.item == 7;
                c.options = if back { c.options - 1 } else { c.options + 1 };
                c.page += if back { -1 } else { 1 };
                vec![Effect::ErasePanel, Effect::Sound("Menu Sound".into())]
            }
            _ => Vec::new(),
        }
    }

    fn keypad(&mut self, button: ComputerButton) -> Vec<Effect> {
        let c = self.computer.as_mut().unwrap();
        let digits = c.auth.as_mut().unwrap();
        match button {
            ComputerButton::Key(d) if digits.len() < CODE_LENGTH => {
                digits.push((b'0' + d) as char);
                Vec::new()
            }
            ComputerButton::Key(_) => vec![Effect::Sound("SysBeeb".into())],
            ComputerButton::Clear => {
                digits.clear();
                Vec::new()
            }
            ComputerButton::Cancel => {
                c.auth = None;
                vec![Effect::ErasePanel]
            }
            ComputerButton::Enter => {
                let code = digits.clone();
                let (fx, refresh) = self.check_code(&code);
                if refresh && let Some(c) = &mut self.computer {
                    c.auth = None;
                }
                fx
            }
            ComputerButton::Item(_) => Vec::new(),
        }
    }

    /// Lingo's CheckAuth; also says whether to leave the keypad.
    fn check_code(&mut self, code: &str) -> (Vec<Effect>, bool) {
        let c = self.computer.as_ref().unwrap();
        let (record, selected) = (c.record, c.item);
        let codes = item(self.data.line(record + 2), selected).to_string();
        let seen = |clue: &str| self.clues_seen.iter().any(|s| s.starts_with(clue));
        for (clue, value) in [(chars(&codes, 1, 2), chars(&codes, 4, 7)), (chars(&codes, 9, 10), chars(&codes, 12, 15))] {
            if !clue.contains('#') && seen(&clue) && code == value {
                let win = item(self.data.line(record + 3), selected).to_string();
                return (self.action(&win, Task::Funct), false);
            }
        }
        if self.menu_name() == "Weapon" && selected == 3 {
            return match self.mission_codes.iter().position(|m| m == code) {
                Some(i) => {
                    let mut fx = self.action(&format!("Target{}", i + 1), Task::Variable);
                    fx.extend(self.erase());
                    (fx, false)
                }
                None => (self.action("AccessDeniedCode", Task::None), false),
            };
        }
        if code == self.auth_code {
            let movie = item(self.data.line(record + 4), selected).to_string();
            return (self.action(&movie, Task::Variable), true);
        }
        (self.action("AccessDeniedCode", Task::None), true)
    }

    /// Lingo's DoCmpAction: play a computer clip and run or queue its task.
    fn action(&mut self, clip: &str, task: Task) -> Vec<Effect> {
        let (record, selected) = self.computer.as_ref().map_or((0, 0), |c| (c.record, c.item));
        let vars = self.data.line(record + 5).to_string();
        if let Some(clip) = clip.strip_suffix('$') {
            // An ending: only with the defender down in phase 4 (§13).
            let mut fx = vec![Effect::Panel { folder: PANEL.into(), name: clip.into(), keep: true }];
            if self.phase() == 4 && (self.defender.off || self.defender.trapped) {
                fx.extend(self.win());
            } else {
                fx.push(Effect::Panel { folder: PANEL.into(), name: "Win.Reject".into(), keep: true });
                if let Some(c) = &mut self.computer {
                    c.auth = None;
                }
            }
            return fx;
        }
        let mut fx = vec![Effect::Panel { folder: PANEL.into(), name: clip.into(), keep: true }];
        let n = match task {
            Task::None => return fx,
            Task::Funct => selected + 4,
            Task::Variable => selected,
        };
        let job = item(&vars, n).to_string();
        if job == "#" || job.is_empty() {
            return fx;
        }
        if let Some(function) = job.strip_suffix('!') {
            fx.extend(self.run_function(function));
        } else {
            self.defender.queue_task(&job, self.elapsed);
        }
        fx
    }

    /// Task functions (the `!` tasks).
    fn run_function(&mut self, function: &str) -> Vec<Effect> {
        let arg = function.split_once("(\"").map(|(_, a)| a.trim_end_matches("\")").to_string()).unwrap_or_default();
        match function.split('(').next().unwrap_or("") {
            "DoorSensorsOff" => {
                if self.defender.sensors == Sensors::Active {
                    // Off for five minutes; the defender answers when they're back.
                    self.defender.sensors = Sensors::Off;
                    self.defender.queue_task("DoorSensors", self.elapsed);
                    self.defender.down_for_a_while(self.elapsed);
                    Vec::new()
                } else {
                    self.action("DoorSensorsBOL.Cmp", Task::None)
                }
            }
            "DefShutDown" => self.shut_down(&arg),
            "KillDefender" => self.kill_defender(&arg),
            "PlaceETA" => {
                // The ship's progress along the ETA line, by time used.
                let used = (self.elapsed / (crate::game::GAME_MINUTES * 60.0)).clamp(0.0, 1.0);
                if let Some(c) = &mut self.computer {
                    c.eta = Some(((used * 84.0) as i32 + 382, 135 - (used * 58.0) as i32));
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn defender_node(&self) -> String {
        self.defender.id().to_string()
    }

    /// Lingo's DefShutDown: trap it behind the bulkheads (only in the
    /// corridor 2.05-2.07) or turn it off.
    fn shut_down(&mut self, how: &str) -> Vec<Effect> {
        if self.defender.off {
            return self.action("DefAlreadyOFF.cmp", Task::None);
        }
        let node = self.defender_node();
        if how == "BulkHeads" && ["2.05", "2.06", "2.07"].contains(&node.as_str()) {
            self.defender.trapped = true;
            self.defender.jammed = false;
            self.off_line(how)
        } else if how == "TurnDefOff" {
            self.defender.off = true;
            self.off_line(how)
        } else {
            let mut fx = self.action(&format!("WrongLoc.{how}"), Task::None);
            fx.extend(self.erase());
            fx
        }
    }

    /// Lingo's KillDefender: incinerate it (6.05-6.06) or eject it (6.08-6.09).
    fn kill_defender(&mut self, how: &str) -> Vec<Effect> {
        let node = self.defender_node();
        let here = match how {
            "Incin" => ["6.05", "6.06"].contains(&node.as_str()),
            "Eject" => ["6.08", "6.09"].contains(&node.as_str()),
            _ => false,
        };
        if here && !self.defender.off {
            let mut fx = if how == "Eject" { self.action("RightLoc.Eject", Task::None) } else { Vec::new() };
            self.defender.off = true;
            fx.extend(self.off_line(how));
            fx
        } else {
            let mut fx = self.action(&format!("WrongLoc.{how}"), Task::None);
            fx.extend(self.erase());
            fx
        }
    }

    /// Lingo's DefOffLine: the segue, then phase 4.
    fn off_line(&mut self, how: &str) -> Vec<Effect> {
        self.defender.off_line(how, self.elapsed);
        let segue = if how == "Incin" { ("Misc Files", how) } else { ("Segues", how) };
        let mut fx = vec![Effect::Segue { folder: segue.0.into(), name: segue.1.into() }];
        match self.phase() {
            3 => fx.extend(self.advance_phase(4)),
            2 | 4 => {
                self.nav.set_phase(4);
                self.computer = None;
                fx.extend([Effect::ErasePanel, self.nav.info_clip()]);
            }
            _ => {}
        }
        fx
    }

    /// Lingo's UserWins: the end of the ship, then a win video.
    fn win(&mut self) -> Vec<Effect> {
        self.won = true;
        let video = format!("Win {}", self.rng.random(2));
        // The end of the ship, then the win video on the WinFrame (no map,
        // buttons or indicators), then back to the start.
        vec![
            Effect::Segue { folder: "Misc Files".into(), name: "EndofShip".into() },
            Effect::ErasePanel,
            Effect::Entering(true),
            Effect::Panel { folder: "Video".into(), name: video, keep: true },
            // On the hardest skill, the Sega CD's extended ending (§14.4).
            if self.skill == 5 { Effect::ExtendedEnding } else { Effect::Wait(0.0) },
            Effect::GameOver,
        ]
    }

    /// Lingo's RunVideoPaco: a video message, which becomes a clue.
    fn play_video(&mut self, first: bool) -> Vec<Effect> {
        let Some(message) = self.computer.as_ref().and_then(|c| c.message.clone()) else { return Vec::new() };
        let number = chars(&message, 1, 2);
        if first {
            self.computer.as_mut().unwrap().video_up = true;
            if !self.clues_seen.iter().any(|s| s.starts_with(&number)) {
                self.clues_seen.push(message.clone());
                if let Some(note) = number.parse::<usize>().ok().and_then(|n| self.data.clue_notes.get(n.wrapping_sub(1))) {
                    self.notes.insert(0, note.clone());
                }
            }
        }
        let mut fx = vec![Effect::Panel { folder: "Video".into(), name: format!("Video {number}"), keep: true }];
        let seen = |mark: char| self.clues_seen.iter().any(|s| s.ends_with(mark));
        if self.phase() == 2 && seen('*') && seen('+') {
            fx.extend(self.advance_phase(3));
        }
        fx
    }
}

/// Which task a clip carries (Lingo's ActionAssociated).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Task {
    None,
    /// The task 4 items on (win actions and the ETA clip).
    Funct,
    /// The item's own task (movie actions).
    Variable,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Access;
    use crate::game::{Action, Interface};
    use crate::nav::Navigator;
    use helixdb::{Direction, NavGraph, Table};

    /// A made-up computer (not the game's table): Main offers Security
    /// (clearance 02), Mission (anywhere) and Mission again (only from a port
    /// that doesn't exist).
    const CMP: &str = "**Cmp
Menu,Main,Return,
Hier(006).02,Hier(012).01,Hier(012).01,#,#,XCmp,#,#,
#,
#,
9.99.0/,
Action,Security,Return,
#,Auth.01,Auth.01,Auth.01,Hier(001).01,XCmp,,#,#,
#,40-1111/41-2222,##-####/##-####,42-3333/43-4444,
#,Secure,#,Internal,
#,SecureReject,DoorSensors,ELO,
#,#,DoorSensorsOff!,#,#,DefShutDown(\"BulkHeads\")!,#,DefShutDown(\"TurnDefOff\")!,
TravMenu,Mission,Return,
#,#,#,#,Hier(001).01,XCmp,#,Trav,
#,#,#,#,Hier(001).01,XCmp,Trav,Trav,";

    fn game() -> Game {
        let node = Table::parse("** Node\n1.01,0,0,0,0,0,0,#,#,#,#,");
        let empty = Table::parse("** x");
        let mut nav = Navigator::at(NavGraph::build(&node, &empty, &empty, &[]), 2, Direction::North);
        *nav.access_mut() = Access::new(vec![vec![1]]);
        nav.access_mut().kit_mut().push(1);
        let data = ComputerData { cmp: CMP.lines().map(str::to_string).collect(), ..Default::default() };
        let mut g = Game::new(nav, 1, 1).with_computers(data);
        g.auth_code = "5678".into();
        g.open_computer();
        g
    }

    fn press(g: &mut Game, b: ComputerButton) -> Vec<Effect> {
        g.act(Action::Computer(b))
    }

    fn enter(g: &mut Game, code: &str) -> Vec<Effect> {
        for d in code.bytes() {
            press(g, ComputerButton::Key(d - b'0'));
        }
        press(g, ComputerButton::Enter)
    }

    fn panel(name: &str) -> Effect {
        Effect::Panel { folder: PANEL.into(), name: name.into(), keep: true }
    }

    #[test]
    fn menus_and_the_keypad() {
        let mut g = game();
        assert_eq!(g.interface(), Interface::Computer);
        assert_eq!(g.computer_view().unwrap().screen, Some(("Main".into(), 0)));
        assert_eq!(g.computer_button_at(400, 75), Some(ComputerButton::Item(1)));
        assert_eq!(g.computer_button_at(470, 135), Some(ComputerButton::Item(6)));

        press(&mut g, ComputerButton::Item(1));
        assert_eq!(g.computer_view().unwrap().screen, Some(("Security".into(), 0)));

        // The generated code runs the movie action and its task.
        press(&mut g, ComputerButton::Item(3));
        assert_eq!(g.computer_view().unwrap().digits.as_deref(), Some(""));
        assert_eq!(g.computer_button_at(358, 75), Some(ComputerButton::Key(1)));
        assert_eq!(g.computer_button_at(420, 115), Some(ComputerButton::Cancel));
        let fx = enter(&mut g, "5678");
        assert_eq!(fx[0], panel("DoorSensors"));
        assert_eq!(g.defender.sensors, Sensors::Off);
        assert_eq!(g.defender_tasks(), ["DoorSensors"]);
        assert_eq!(g.computer_view().unwrap().digits, None, "back to the menu");

        // A wrong code.
        press(&mut g, ComputerButton::Item(4));
        assert_eq!(enter(&mut g, "1")[0], panel("AccessDeniedCode"));

        // A clue code works only once its video has been seen.
        press(&mut g, ComputerButton::Item(4));
        assert_eq!(enter(&mut g, "4444")[0], panel("AccessDeniedCode"));
        g.clues_seen.push("43*".into());
        press(&mut g, ComputerButton::Item(4));
        let fx = enter(&mut g, "4444");
        assert_eq!(fx[0], panel("Internal"));
        assert!(g.defender.off, "Internal turns the defender off");
        assert!(fx.contains(&Effect::Segue { folder: "Segues".into(), name: "TurnDefOff".into() }));
    }

    #[test]
    fn bulkheads_need_the_defender_in_the_corridor() {
        let mut g = game();
        g.clues_seen.push("41+".into());
        press(&mut g, ComputerButton::Item(1));
        press(&mut g, ComputerButton::Item(2));
        let fx = enter(&mut g, "2222");
        assert_eq!(fx[0], panel("Secure"));
        assert_eq!(fx[1], panel("WrongLoc.BulkHeads"));
        assert!(!g.defender.trapped);
    }

    #[test]
    fn clearance_location_and_paging() {
        let mut g = game();
        press(&mut g, ComputerButton::Item(3));
        assert_eq!(press(&mut g, ComputerButton::Item(3))[0], panel("AccessDeniedLoc"));
        g.nav.access_mut().kit_mut().clear();
        assert_eq!(press(&mut g, ComputerButton::Item(1))[0], panel("AccessDeniedDNA"));

        press(&mut g, ComputerButton::Item(2));
        assert_eq!(g.computer_view().unwrap().screen, Some(("Mission".into(), 0)));
        assert_eq!(g.computer_button_at(390, 135), Some(ComputerButton::Item(8)));
        press(&mut g, ComputerButton::Item(8));
        let view = g.computer_view().unwrap();
        assert_eq!(view.screen, Some(("Mission".into(), 1)));
        assert_eq!(view.texts, [("5678".to_string(), (351, 108))], "Mission 1 shows the code");
        press(&mut g, ComputerButton::Item(7));
        assert_eq!(g.computer_view().unwrap().screen, Some(("Mission".into(), 0)));
        press(&mut g, ComputerButton::Item(6));
        assert_eq!(g.interface(), Interface::Nav(crate::Panel::Node));
    }

    #[test]
    fn codes_avoid_the_tables() {
        let g = game();
        let (_, missions) = g.codes();
        assert_eq!(missions.len(), 3);
        assert!(missions.iter().all(|c| c.len() == 4 && !CMP.contains(c.as_str())));
    }

    #[test]
    fn lingo_strings() {
        assert_eq!(item("a,b,,d", 3), "");
        assert_eq!(item("a,b", 5), "");
        assert_eq!(chars("Hier(008).02", 6, 8), "008");
        assert_eq!(chars("Hier(008).02", 11, 12), "02");
        assert_eq!(chars("Auth.01", 11, 12), "");
        assert_eq!(chars("Movi(Crew01)", 6, 11), "Crew01");
    }
}
