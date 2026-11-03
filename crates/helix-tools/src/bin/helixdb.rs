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
    }
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
