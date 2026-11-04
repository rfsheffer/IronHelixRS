//! Saving and loading a game (design doc §12.5).
//!
//! A plain `key=value` text, one per line, holding what the original's save
//! file did: the probe's place and number, the phase, energy, time played and
//! the next time warning, the skill, the DNA kit, this game's video clues and
//! those seen, the codes, the notes, and the defender's place and state. A
//! defender return timer that was running restarts at four minutes and queued
//! tasks answer a minute after loading, as in the original.

use helixdb::Direction;

use crate::defender::Sensors;
use crate::game::Game;

const HEADER: &str = "Iron Helix save 1";

impl Game {
    /// The game as save-file text.
    pub fn save(&self) -> String {
        let d = &self.defender;
        let sensors = match d.sensors() {
            Sensors::Active => "active",
            Sensors::Off => "off",
            Sensors::Done => "done",
        };
        let lines = [
            HEADER.to_string(),
            format!("skill={}", self.skill),
            format!("probe={}", self.probe_number()),
            format!("line={}", self.nav.line()),
            format!("facing={}", self.nav.facing().index()),
            format!("phase={}", self.phase()),
            format!("energy={}", self.energy()),
            format!("elapsed={}", self.elapsed),
            format!("next_warning={}", self.next_warning()),
            format!("auto_scan={}", self.auto_scan() as u8),
            format!("kit={}", join(self.kit().iter().map(u8::to_string))),
            format!("clues={}", self.clues.join("|")),
            format!("seen={}", self.clues_seen.join("|")),
            format!("notes={}", self.notes.join("|")),
            format!("codes={}", join(std::iter::once(self.auth_code.clone()).chain(self.mission_codes.iter().cloned()))),
            format!("defender={}", d.line()),
            format!("defender_off={}", d.off() as u8),
            format!("trapped={}", d.trapped as u8),
            format!("sensors={sensors}"),
            format!("tasks={}", d.tasks.join(" ")),
            format!("returning={}", d.returning() as u8),
        ];
        lines.join("\n") + "\n"
    }

    /// Restore a saved game onto this (freshly set up) one.
    pub fn load(&mut self, text: &str) -> Result<(), String> {
        let mut lines = text.lines();
        if lines.next().map(str::trim) != Some(HEADER) {
            return Err("not an Iron Helix save".into());
        }
        let fields: std::collections::HashMap<&str, &str> = lines.filter_map(|l| l.split_once('=')).collect();
        let get = |k: &str| fields.get(k).copied().unwrap_or("");
        let num = |k: &str| get(k).trim().parse::<f32>().map_err(|_| format!("bad {k} in the save"));
        let list = |k: &str, sep: char| -> Vec<String> { get(k).split(sep).filter(|s| !s.is_empty()).map(str::to_string).collect() };

        self.skill = (num("skill")? as u8).clamp(1, 5);
        self.set_probe_number(num("probe")? as u8);
        let facing = Direction::from_index(num("facing")? as u32).unwrap_or(Direction::North);
        self.nav.place(num("line")? as usize, facing);
        self.nav.set_phase(num("phase")? as u8);
        self.set_energy(num("energy")? as i32);
        self.elapsed = num("elapsed")?;
        self.set_next_warning(num("next_warning")?);
        self.set_auto_scan(get("auto_scan") == "1");
        *self.nav.access_mut().kit_mut() = list("kit", ',').iter().filter_map(|d| d.parse().ok()).collect();
        self.clues = list("clues", '|');
        self.clues_seen = list("seen", '|');
        self.notes = list("notes", '|');
        let codes = list("codes", ',');
        if let Some((auth, missions)) = codes.split_first() {
            self.auth_code = auth.clone();
            self.mission_codes = missions.to_vec();
        }
        let sensors = match get("sensors") {
            "off" => Sensors::Off,
            "done" => Sensors::Done,
            _ => Sensors::Active,
        };
        let now = self.elapsed;
        self.defender.restore(
            num("defender")? as usize,
            get("defender_off") == "1",
            get("trapped") == "1",
            sensors,
            list("tasks", ' '),
            get("returning") == "1",
            now,
        );
        self.after_load();
        Ok(())
    }
}

fn join(items: impl Iterator<Item = String>) -> String {
    items.collect::<Vec<_>>().join(",")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::Access;
    use crate::nav::Navigator;
    use helixdb::{NavGraph, Table};

    fn game() -> Game {
        let node = Table::parse("** Node\n1.01,1,0,0,0,0,0,#,#,#,#,\n1.02,0,0,-1,0,0,0,#,#,#,#,");
        let empty = Table::parse("** x");
        let mut nav = Navigator::at(NavGraph::build(&node, &empty, &empty, &[]), 2, Direction::North);
        *nav.access_mut() = Access::new(vec![vec![1]]);
        Game::new(nav, 3, 1)
    }

    #[test]
    fn round_trip() {
        let mut g = game();
        g.nav.place(3, Direction::West);
        g.nav.set_phase(2);
        g.nav.access_mut().kit_mut().extend([10, 2, 1]);
        g.clues = vec!["1.05-1-02+".into()];
        g.clues_seen = vec!["02+".into()];
        g.notes = vec!["Weapons: Abort 7777....Execute on the Bridge".into()];
        g.auth_code = "4069".into();
        g.mission_codes = vec!["6986".into(), "4340".into(), "6315".into()];
        g.elapsed = 600.0;
        let text = g.save();

        let mut h = game();
        h.load(&text).unwrap();
        assert_eq!(h.save(), text);
        assert_eq!(h.nav().node().id, "1.02");
        assert_eq!(h.nav().facing(), Direction::West);
        assert_eq!(h.kit(), [10, 2, 1]);
        assert_eq!(h.skill(), 3);
        assert!(game().load("not a save").is_err());
    }
}
