//! The probe arm (design doc §7, Lingo `Arm`).
//!
//! The arm is one of 27 cast members, from retracted ([`RETRACTED`]) to fully
//! extended ([`EXTENDED`]); each has its own horizontal position, and the
//! vertical position moves freely. It eases toward the mouse while the button
//! is held, and snaps onto the facing's computer port when it comes close.
//! Everything advances in ticks, like the original's loop.

/// Cast numbers: fully extended, the deploy target, and retracted.
pub const EXTENDED: u16 = 62;
const DEPLOYED: u16 = 82;
pub const RETRACTED: u16 = 88;
/// Where the arm sprite sits when retracted (its registration point).
const HOME: (i32, i32) = (70, 152);
/// Vertical position after deploying.
const DEPLOYED_V: i32 = 30;
/// Range the mouse can move the arm to vertically.
const V_RANGE: (i32, i32) = (-6, 95);
/// How near the mouse and the arm must be to a port, in pixels.
const PORT_REACH: i32 = 30;
/// Snapping onto a port stops this close.
const SNAP: (i32, i32) = (3, 4);
/// Give up snapping after this many ticks (the steps may straddle the port).
const SNAP_LIMIT: u32 = 40;

/// Horizontal offset of an arm cast from [`HOME`] (Lingo's HorzArmOffset).
fn offset(cast: u16) -> i32 {
    let cast = cast as i32;
    if cast >= 80 {
        10 * (RETRACTED as i32 - cast)
    } else if cast >= 71 {
        81 + 8 * (80 - cast)
    } else {
        155 + 6 * (71 - cast)
    }
}

/// The arm cast for a mouse x (Lingo maps x onto 27 steps of 9 pixels).
fn cast_for(x: i32) -> u16 {
    RETRACTED - ((x - 63) / 9).clamp(0, 26) as u16
}

/// Something the arm wants to happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArmEvent {
    /// Moving: start the motion sound.
    Motion,
    Stop,
    /// Held on a port.
    HotSpot,
    /// Came to rest docked at the port.
    Docked,
    /// Finished retracting.
    Retracted,
}

#[derive(Debug, Clone)]
pub struct Arm {
    cast: u16,
    v: i32,
    target_cast: u16,
    target_v: i32,
    speed: f32,
    /// Deployed (false while retracting).
    out: bool,
    /// The mouse is down; `mouse` is where it last was while down.
    held: bool,
    mouse: (i32, i32),
    /// Ignore the mouse until it's pressed in the view again.
    just_deployed: bool,
    /// The facing's computer port, if any.
    port: Option<(i32, i32)>,
    located: bool,
    locked: bool,
    snapping: Option<u32>,
    moving: bool,
    running: bool,
}

impl Arm {
    /// Lingo's InstallArm: extend from the retracted position.
    pub(crate) fn deploy(port: Option<(i32, i32)>) -> Self {
        Self {
            cast: RETRACTED,
            v: HOME.1,
            target_cast: DEPLOYED,
            target_v: DEPLOYED_V,
            speed: 8.0,
            out: true,
            held: false,
            mouse: (0, 0),
            just_deployed: true,
            port,
            located: false,
            locked: false,
            snapping: None,
            moving: false,
            running: true,
        }
    }

    /// Lingo's DeInstallArm: pull back in.
    pub(crate) fn retract(&mut self) {
        self.target_cast = RETRACTED;
        self.target_v = HOME.1;
        self.speed = 6.0;
        self.out = false;
        self.located = false;
        self.locked = false;
        self.snapping = None;
        self.running = true;
    }

    pub fn out(&self) -> bool {
        self.out
    }

    /// The cast to draw and where (its registration point).
    pub fn view(&self) -> (u16, (i32, i32)) {
        (self.cast, (HOME.0 + offset(self.cast), self.v))
    }

    pub(crate) fn busy(&self) -> bool {
        self.running
    }

    fn aim(&mut self) {
        let (x, y) = self.mouse;
        self.target_cast = cast_for(x);
        self.target_v = (y - 40).clamp(V_RANGE.0, V_RANGE.1);
    }

    pub(crate) fn press(&mut self, x: i32, y: i32) {
        if !self.out {
            return;
        }
        self.held = true;
        self.just_deployed = false;
        self.mouse = (x, y);
        self.located = false;
        self.locked = false;
        self.aim();
        self.running = true;
    }

    pub(crate) fn drag(&mut self, x: i32, y: i32) {
        if self.held {
            self.mouse = (x, y);
            if !self.just_deployed && self.out && self.snapping.is_none() {
                self.aim();
            }
        }
    }

    pub(crate) fn release(&mut self) {
        self.held = false;
    }

    fn arrived(&self) -> bool {
        self.cast == self.target_cast && self.v == self.target_v
    }

    pub(crate) fn tick(&mut self) -> Vec<ArmEvent> {
        let mut events = Vec::new();
        if !self.running {
            return events;
        }
        if let Some(n) = self.snapping {
            return self.snap(n);
        }

        // Ease toward the target, faster when far (Lingo's speed factors).
        if self.cast != self.target_cast {
            let step = ((self.cast as i32 - self.target_cast as i32).abs() as f32 / self.speed) as i32 + 1;
            let step = step.min((self.cast as i32 - self.target_cast as i32).abs());
            self.cast = if self.cast > self.target_cast { self.cast - step as u16 } else { self.cast + step as u16 };
        }
        if self.v != self.target_v {
            let step = ((self.v - self.target_v).abs() as f32 / (self.speed * 0.6)) as i32 + 1;
            let step = step.min((self.v - self.target_v).abs());
            self.v += if self.v > self.target_v { -step } else { step };
        }

        if self.arrived() {
            if self.moving && self.out {
                events.push(ArmEvent::Stop);
            }
            self.moving = false;
        } else if !self.moving {
            self.moving = true;
            events.push(ArmEvent::Motion);
        }

        // Near the port with the mouse: snap on (Lingo's PanelControl).
        if let (Some((ph, pv)), true) = (self.port, self.out) {
            let (mx, my) = self.mouse;
            let (ah, av) = self.view().1;
            if (mx - ph).abs() < PORT_REACH && (my - 40 - pv).abs() < PORT_REACH {
                if !self.locked && (ah - ph).abs() < PORT_REACH && (av - pv).abs() < PORT_REACH {
                    self.snapping = Some(0);
                    return events;
                } else if self.locked && ((ah - ph).abs() > PORT_REACH || (av - pv).abs() > PORT_REACH) {
                    self.locked = false;
                    self.located = false;
                }
            } else {
                self.located = false;
                self.locked = false;
            }
        }

        if self.arrived() && !self.held {
            self.running = false;
            if !self.out {
                events.push(ArmEvent::Retracted);
            } else if self.located {
                events.push(ArmEvent::Docked);
            }
        }
        events
    }

    /// Lingo's LocatePanel: step onto the port, then lock.
    fn snap(&mut self, n: u32) -> Vec<ArmEvent> {
        let (ph, pv) = self.port.unwrap_or_default();
        let (ah, av) = self.view().1;
        let mut done = true;
        if (ah - ph).abs() > SNAP.0 {
            self.cast = if ah > ph { (self.cast + 1).min(RETRACTED) } else { (self.cast - 1).max(EXTENDED) };
            done = false;
        }
        if (av - pv).abs() > SNAP.1 {
            self.v += if av > pv { -2 } else { 2 };
            done = false;
        }
        if !done && n < SNAP_LIMIT {
            self.snapping = Some(n + 1);
            return Vec::new();
        }
        self.snapping = None;
        self.locked = true;
        self.located = true;
        self.target_cast = self.cast;
        self.target_v = self.v;
        self.moving = false;
        let mut events = vec![ArmEvent::Stop];
        if self.held {
            events.push(ArmEvent::HotSpot);
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(arm: &mut Arm) -> Vec<ArmEvent> {
        let mut events = Vec::new();
        for _ in 0..300 {
            events.extend(arm.tick());
            if !arm.busy() {
                break;
            }
        }
        events
    }

    #[test]
    fn deploys_and_retracts() {
        let mut arm = Arm::deploy(None);
        let events = run(&mut arm);
        assert_eq!(arm.view(), (DEPLOYED, (HOME.0 + offset(DEPLOYED), DEPLOYED_V)));
        assert_eq!(events, [ArmEvent::Motion, ArmEvent::Stop]);
        arm.retract();
        assert_eq!(run(&mut arm).last(), Some(&ArmEvent::Retracted));
        assert_eq!(arm.view().0, RETRACTED);
    }

    #[test]
    fn follows_the_mouse_and_docks_at_the_port() {
        // A port where cast 75 at v 57 puts the arm (offset 121).
        let port = (HOME.0 + offset(75), 57);
        let mut arm = Arm::deploy(Some(port));
        run(&mut arm);
        arm.press(port.0, port.1 + 40);
        arm.release();
        let events = run(&mut arm);
        assert!(events.contains(&ArmEvent::Docked), "{events:?}");
        let (_, (h, v)) = arm.view();
        assert!((h - port.0).abs() <= SNAP.0 && (v - port.1).abs() <= SNAP.1);
    }

    #[test]
    fn mouse_far_from_the_port_does_not_dock() {
        let mut arm = Arm::deploy(Some((250, 57)));
        run(&mut arm);
        arm.press(100, 60);
        arm.release();
        assert!(!run(&mut arm).contains(&ArmEvent::Docked));
        assert_eq!(arm.view().0, cast_for(100));
    }

    #[test]
    fn horizontal_steps() {
        assert_eq!(offset(RETRACTED), 0);
        assert_eq!(offset(80), 80);
        assert_eq!(offset(71), 153);
        assert_eq!(offset(EXTENDED), 209);
        assert_eq!(cast_for(0), RETRACTED);
        assert_eq!(cast_for(500), EXTENDED);
    }
}
