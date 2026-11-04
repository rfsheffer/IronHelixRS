//! `helixdb`: inspect Iron Helix's game data tables.
//!
//! Reads them straight from the Mac game's Director projector (its resource
//! fork, as written by `macx sit-extract` from the installer).
//!
//! ```text
//! helixdb summary <app.rsrc>                   table sizes, link check
//! helixdb nodes   <app.rsrc> [--level N]       the navigation graph, node by node
//! helixdb videos  <app.rsrc> --nav <folder>    check every movement video exists
//! ```

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use helixdb::{Direction, GameData, Move, NavGraph, NavNode};

#[derive(Parser)]
#[command(about = "Inspect Iron Helix's game data tables (from the Mac projector)")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Table sizes and a check that every exit lands on a node.
    Summary { app: PathBuf },
    /// Print the navigation graph.
    Nodes {
        app: PathBuf,
        #[arg(long)]
        level: Option<u8>,
    },
    /// List the movement videos the graph needs and check they exist.
    Videos {
        app: PathBuf,
        /// The "Navigation" folder from the Mac CD.
        #[arg(long)]
        nav: PathBuf,
    },
    /// Let the defender roam on its own (the probe idle at the start) and
    /// check every step lands on a Def row, reporting where it went.
    Defender {
        app: PathBuf,
        /// Steps to simulate.
        #[arg(long, default_value_t = 2000)]
        steps: usize,
        #[arg(long, default_value_t = 1)]
        seed: u64,
    },
    /// Plan the defender from every Def node to every node and report the
    /// plans that never pass through their target. (Plans end one step past
    /// it, across its door, by design.)
    Paths { app: PathBuf },
    /// Explore every navigation state reachable from the start through the
    /// game rules (helix-core) and check every movie and clip they ask for.
    Walk {
        app: PathBuf,
        /// The "Iron Helix™ Files" folder from the Mac CD.
        #[arg(long)]
        files: PathBuf,
        /// The PC CD's HELIX folder: adds its hidden rooms and movies.
        #[arg(long)]
        pc: Option<PathBuf>,
    },
}

fn load(app: &Path) -> Result<GameData> {
    let fork = fs::read(app).with_context(|| format!("reading {}", app.display()))?;
    let movie = director::Movie::from_resource_fork(&fork)?;
    Ok(GameData::from_movie(&movie)?)
}

fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Summary { app } => summary(&load(&app)?),
        Command::Nodes { app, level } => nodes(&load(&app)?, level),
        Command::Videos { app, nav } => videos(&load(&app)?, &nav),
        Command::Defender { app, steps, seed } => defender(load(&app)?, steps, seed),
        Command::Paths { app } => paths(load(&app)?),
        Command::Walk { app, files, pc } => {
            let mut g = load(&app)?;
            if let Some(pc) = &pc {
                let exe = pc.join("HELIX.EXE");
                let added = g.add_pc_rooms(&fs::read(&exe).with_context(|| format!("reading {}", exe.display()))?)?;
                println!("PC release: added {}", added.join(", "));
            }
            // Installed files ("Misc Files") sit next to the game program.
            let installed = app.parent().map(Path::to_path_buf).unwrap_or_default();
            walk(g, &helixdb::GameFiles::new(files, pc.as_deref()).with_installed(installed))
        }
    }
}

fn defender(g: GameData, steps: usize, seed: u64) -> Result<()> {
    use helix_core::{Access, DefenderData, Effect, Game, Navigator};
    use std::collections::BTreeMap;

    let mut nav = Navigator::new(g.nav.clone());
    *nav.access_mut() = Access::new(g.clearance.clone());
    let mut game = Game::new(nav, 1, seed).with_defender(DefenderData::new(&g.defender, &g.deaths));
    let (mut taken, mut caught, mut bad, mut longest) = (0, 0, 0, 0);
    let mut decks: BTreeMap<u8, usize> = BTreeMap::new();
    let mut last = game.defender().id().to_string();
    let mut ticks = 0u64;
    while taken < steps && ticks < 50_000_000 {
        ticks += 1;
        let fx = game.tick();
        if fx.iter().any(|e| matches!(e, Effect::Segue { name, .. } if name.starts_with("DefDeath"))) {
            caught += 1;
        }
        if fx.iter().any(|e| matches!(e, Effect::GameOver)) {
            game = Game::new(Navigator::new(g.nav.clone()), 1, seed + ticks).with_defender(DefenderData::new(&g.defender, &g.deaths));
        }
        let id = game.defender().id().to_string();
        if id != last {
            taken += 1;
            if id.is_empty() {
                bad += 1;
            }
            *decks.entry(game.defender().level()).or_default() += 1;
            longest = longest.max(game.defender().plan().len());
            last = id;
        }
    }
    println!("{taken} steps in {:.0} minutes of play, {bad} off the Def table, longest plan {longest}", ticks as f64 / 30.0 / 60.0);
    println!("steps by deck: {decks:?}");
    println!("caught the idle probe {caught} times");
    Ok(())
}

fn paths(g: GameData) -> Result<()> {
    use helix_core::{DefenderData, Game, Navigator};
    use std::collections::BTreeMap;

    let game = Game::new(Navigator::new(g.nav.clone()), 1, 1).with_defender(DefenderData::new(&g.defender, &g.deaths));
    let d = game.defender();
    // Targets: every node the probe can stand on that the Def table has (not ladders).
    let targets: Vec<&str> = g.defender.iter().map(|n| n.id.as_str()).filter(|id| !id.contains('L')).collect();
    let (mut ok, mut gave_up) = (0, 0);
    let mut wrong: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for from in &g.defender {
        for &to in &targets {
            match d.route(from.line, to) {
                Some(visits) if from.id == to || visits.iter().any(|v| v == to) => ok += 1,
                Some(visits) => wrong.entry(to.to_string()).or_default().push(format!("{} -> {}", from.id, visits.last().map_or("(stays)", String::as_str))),
                None => gave_up += 1,
            }
        }
    }
    println!("{ok} plans reach their target, {gave_up} give up as too long");
    for (to, misses) in &wrong {
        println!("to {to}: {} miss, e.g. {}", misses.len(), misses.iter().take(3).cloned().collect::<Vec<_>>().join(", "));
    }
    Ok(())
}

fn walk(g: GameData, files: &helixdb::GameFiles) -> Result<()> {
    use helix_core::{Effect, NavAction, Navigator};
    use std::collections::{BTreeMap, BTreeSet, VecDeque};

    let actions = [
        NavAction::Forward,
        NavAction::Back,
        NavAction::TurnLeft,
        NavAction::TurnRight,
        NavAction::Up,
        NavAction::Down,
        NavAction::Door,
    ];
    // Every game phase, since some doors open only in later ones.
    let starts: Vec<Navigator> = (1..=4)
        .map(|phase| {
            let mut n = Navigator::new(g.nav.clone());
            n.set_phase(phase);
            n
        })
        .collect();
    let mut seen: BTreeSet<_> = starts.iter().map(Navigator::state_key).collect();
    let mut queue = VecDeque::from(starts);
    let mut nodes = BTreeSet::new();
    let mut movies: BTreeMap<String, bool> = BTreeMap::new();
    let mut clips: BTreeMap<String, bool> = BTreeMap::new();

    let exists = |folder: &str, name: &str| files.find(folder, name).is_some();

    while let Some(nav) = queue.pop_front() {
        nodes.insert(nav.node().id.clone());
        let mut tries: Vec<NavAction> = actions.to_vec();
        if nav.node().is_elevator() {
            tries.extend((1..=6).map(NavAction::Elevator));
        }
        for repeat in [false, true] {
            for &a in &tries {
                let mut next = nav.clone();
                if repeat {
                    next.act(NavAction::ToggleRepeat);
                }
                for e in next.act(a) {
                    match e {
                        Effect::Video { folder, name, .. } => {
                            let ok = exists(&folder, &name);
                            movies.insert(format!("{folder}/{name}"), ok);
                        }
                        Effect::InfoClip { folder, name } => {
                            let ok = exists(&folder, &name);
                            clips.insert(format!("{folder}/{name}"), ok);
                        }
                        _ => {}
                    }
                }
                if seen.insert(next.state_key()) {
                    queue.push_back(next);
                }
            }
        }
    }

    let missing: Vec<_> = movies.iter().filter(|(_, ok)| !**ok).map(|(m, _)| m).collect();
    let missing_clips = clips.values().filter(|ok| !**ok).count();
    println!("{} states explored, {} of {} nodes reached", seen.len(), nodes.len(), g.nav.len());
    println!("{} distinct movies requested, {} missing", movies.len(), missing.len());
    for m in missing.iter().take(30) {
        println!("  missing {m}");
    }
    println!("{} info-strip clips requested, {} missing (the game skips those)", clips.len(), missing_clips);

    // Every scan point's movies, in all five skill tables.
    let mut scan_movies = BTreeSet::new();
    for n in g.nav.nodes() {
        for p in n.scan.iter().flatten().flatten() {
            scan_movies.insert(("Scan", format!("{:02}.ScanEnhance", p.item)));
            scan_movies.insert(("Scan", format!("{:02}.ScanAcquire", p.item)));
            scan_movies.insert(("Scan/ScanText", format!("{:02}.ScanTxt", p.dna)));
        }
    }
    let missing: Vec<_> = scan_movies.iter().filter(|(f, n)| !exists(f, n)).collect();
    println!("{} scan movies and texts for the scan points, {} missing", scan_movies.len(), missing.len());
    for (f, n) in missing.iter().take(30) {
        println!("  missing {f}/{n}");
    }

    // Everything the computers can play: menu clips, win and movie actions,
    // the fixed messages, video clues and the segues.
    let mut computer: BTreeSet<(&str, String)> = BTreeSet::new();
    let lines = &g.cmp_lines;
    for (i, line) in lines.iter().enumerate() {
        for part in line.split(',') {
            if let Some(name) = part.strip_prefix("Movi(").and_then(|r| r.strip_suffix(')')) {
                computer.insert(("Computer", name.to_string()));
            }
        }
        if line.starts_with("Action,") {
            // Win and movie actions belong to the keypad (Auth) rows.
            let item = |offset: usize, n: usize| lines.get(i + offset).and_then(|l| l.split(',').nth(n)).unwrap_or("").trim();
            for n in 0..4 {
                if !item(1, n).starts_with("Auth") {
                    continue;
                }
                for name in [item(3, n), item(4, n)] {
                    let name = name.trim_end_matches('$');
                    if !name.is_empty() && name != "#" && !name.starts_with('[') {
                        computer.insert(("Computer", name.to_string()));
                    }
                }
            }
        }
    }
    let fixed = [
        "AccessDeniedDNA", "AccessDeniedLoc", "AccessDeniedCode", "AccessDeniedToCmp", "Win.Reject", "DefAlreadyOFF.cmp",
        "DoorSensorsBOL.Cmp", "RightLoc.Eject", "WrongLoc.BulkHeads", "WrongLoc.Incin", "WrongLoc.Eject", "Target1",
        "Target2", "Target3",
    ];
    computer.extend(fixed.iter().map(|n| ("Computer", n.to_string())));
    for list in &g.video_clue_lists {
        for clue in list.split([',', '\n']).map(str::trim).filter(|c| c.len() > 4) {
            let number = &clue[clue.len() - 3..clue.len() - 1];
            computer.insert(("Video", format!("Video {number}")));
        }
    }
    computer.extend(["Win 1", "Win 2"].map(|n| ("Video", n.to_string())));
    computer.extend(["BulkHeads", "TurnDefOff", "Eject"].map(|n| ("Segues", n.to_string())));
    computer.extend(["Incin", "EndofShip", "UnableDeploy"].map(|n| ("Misc Files", n.to_string())));
    let missing: Vec<_> = computer.iter().filter(|(f, n)| !exists(f, n)).collect();
    println!("{} computer clips, video messages and segues, {} missing", computer.len(), missing.len());
    for (f, n) in missing.iter().take(30) {
        println!("  missing {f}/{n}");
    }

    // Every death video the DeathDB can pick: row 4 * (node line - 1) +
    // facing + 1, group n -> <node>.<facing letter>.<n>.
    let mut deaths = BTreeSet::new();
    for row in &g.deaths {
        let facing = (row.line - 1) % 4;
        let letter = ['N', 'E', 'S', 'W'][facing];
        let level = row.node.chars().next().unwrap_or('0');
        for n in 1..=row.groups.len() {
            deaths.insert((format!("Navigation/L{level}"), format!("{}.{letter}.{n}", row.node)));
        }
    }
    let missing: Vec<_> = deaths.iter().filter(|(f, n)| !exists(f, n)).collect();
    println!("{} death videos, {} missing", deaths.len(), missing.len());
    for (f, n) in missing.iter().take(30) {
        println!("  missing {f}/{n}");
    }
    Ok(())
}

fn summary(g: &GameData) -> Result<()> {
    let levels: BTreeSet<u8> = g.nav.nodes().map(NavNode::level).collect();
    let exits: usize = g.nav.nodes().map(|n| n.exits.iter().flatten().count()).sum();
    let doors: usize = g.nav.nodes().map(|n| n.doors.iter().flatten().count()).sum();
    let ladders = g.nav.nodes().filter(|n| n.up.is_some() || n.down.is_some()).count();
    let scan_points: usize = g.nav.nodes().map(|n| n.scan.iter().flatten().flatten().count()).sum();
    println!("navigation: {} nodes on levels {levels:?}", g.nav.len());
    println!("  {exits} exits, {doors} doors, {ladders} nodes with ladders, {scan_points} scan points (all skill levels)");
    println!("defender: {} nodes", g.defender.len());
    println!("deaths: {} rows", g.deaths.len());
    println!("computer: {} screens: {}", g.screens.len(), g.screens.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(", "));
    println!("probe failure spots: {}, no-arm spots: {}", g.probe_failures.len(), g.no_arm.len());

    let problems = g.nav.check();
    if problems.is_empty() {
        println!("link check: every exit and ladder lands on a node");
    } else {
        println!("link check: {} problems", problems.len());
        for p in &problems {
            println!("  {}: {}", p.from, p.what);
        }
    }
    Ok(())
}

fn target(g: &NavGraph, n: &NavNode, offset: i32) -> String {
    g.step(n, offset).map_or_else(|| format!("?{offset:+}"), |t| t.id.clone())
}

fn nodes(g: &GameData, level: Option<u8>) -> Result<()> {
    for n in g.nav.nodes().filter(|n| level.is_none_or(|l| n.level() == l)) {
        let mut parts = Vec::new();
        for d in Direction::ALL {
            if let Some(e) = n.exit(d) {
                let door = n.door(d).map(|x| format!(" [door {x}]")).unwrap_or_default();
                parts.push(format!("{d}->{}{door}", target(&g.nav, n, e.offset)));
            } else if let Some(x) = n.door(d) {
                parts.push(format!("{d}: door {x}"));
            }
        }
        for (name, l) in [("up", n.up), ("down", n.down)] {
            if let Some(l) = l {
                parts.push(format!("{name} (facing {})->{} facing {}", l.from, target(&g.nav, n, l.offset), l.facing));
            }
        }
        println!("{:8} line {:3}  {}", n.id, n.line, parts.join("  "));
    }
    Ok(())
}

/// Every movement video name the graph implies, with the node it starts from.
fn needed_videos(g: &NavGraph) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for n in g.nodes() {
        for d in Direction::ALL {
            if n.exit(d).is_some() {
                if n.is_ladder() {
                    out.insert(format!("{}.F", n.id));
                } else {
                    out.insert(n.video(Move::Forward, d));
                }
            }
            // Back is the exit of the opposite facing, played facing `d`. The
            // game disables it when a door is behind the probe.
            if n.exit(d.opposite()).is_some() && n.door(d.opposite()).is_none() && !n.is_ladder() {
                out.insert(n.video(Move::Back, d));
            }
        }
        if n.up.is_some() {
            out.insert(n.video(Move::Up, Direction::North));
        }
        if n.down.is_some() {
            out.insert(n.video(Move::Down, Direction::North));
        }
    }
    out
}

fn videos(g: &GameData, nav: &Path) -> Result<()> {
    let mut present = BTreeSet::new();
    for entry in fs::read_dir(nav)? {
        let dir = entry?.path();
        if dir.is_dir() {
            for f in fs::read_dir(&dir)? {
                let name = f?.file_name().to_string_lossy().into_owned();
                if !name.ends_with(".rsrc") {
                    present.insert(name);
                }
            }
        }
    }
    let needed = needed_videos(&g.nav);
    let missing: Vec<_> = needed.iter().filter(|v| !present.contains(*v)).collect();
    println!("{} movement videos implied by the graph, {} found, {} missing", needed.len(), needed.len() - missing.len(), missing.len());
    for m in missing.iter().take(40) {
        println!("  missing {m}");
    }
    let turns = present.iter().filter(|p| p.contains(".L") || p.contains(".R")).count();
    println!("({} files in the folder; {} are turns, which every facing has)", present.len(), turns);
    Ok(())
}
