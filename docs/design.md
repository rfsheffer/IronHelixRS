# Iron Helix: Game Design Reference

This document describes how *Iron Helix* plays, worked out from the Mac release, which is a
Director 3 game whose Lingo scripts and data tables are readable. It is written in our own words
as the specification for the Rust reimplementation. **No original code or table contents are
reproduced here**; the engine reads the original tables from the player's copy at runtime (see
`crates/helixdb`), and short identifiers (asset names, sound names, node ids) appear only so the
engine can find the right assets.

Conventions:

- **Ticks** are 1/60 s (Director's clock). Times in minutes are real-time minutes.
- **Facing** is 0-3 = North, East, South, West; letters N/E/S/W appear in video names.
- **Node ids** are `<level>.<node>` (e.g. `4.04`); ladder nodes contain `L` (`2.1.2.L`),
  elevator nodes end in `.E` (`1.16.E`).
- **Line** means a line number in the node tables (see §3.1), which is the game's node index.
- Coordinates are on the original 512×342-ish stage; the new engine can scale.

Status markers: ✅ confirmed in the scripts and data, ❓ needs verification (see §14).

---

## 1. The game in one page

The player remotely pilots a **probe** aboard the warship *Jeremiah Obrian*, which is heading to
destroy a planet. The ship is patrolled by a **defender** robot that destroys probes it sees.

1. **Phase 1: collect DNA.** Scan crew remains/traces around the ship and store DNA samples
   (up to 3 at a time). DNA grants clearance to doors and computer functions. The phase ends
   when the kit holds the right crew members' DNA (§6.4).
2. **Phase 2: find the video messages.** At certain computer ports, recorded crew video messages
   play; each reveals a code and where to use it, added to the player's notes. Two kinds of clue
   are needed (§8.3).
3. **Phase 3: disable the defender.** Trap it behind bulkheads, turn it off, incinerate it or eject
   it, using computer commands at the right moment and place (§9.7).
4. **Phase 4: stop the ship.** While the defender is down (it comes back after ~5 minutes), issue
   the final command with a code learned from the videos. Success plays the ending.

The player has **3 probes** and **90 real minutes**. Losing all probes or running out of time
ends the game.

---

## 2. Screen layout

The game runs in a single fixed screen. Main regions (positions approximate, from the scripts):

| Region | What it shows | Notes |
|---|---|---|
| **View window** (left, ~240×128 movie area) | Navigation videos, scan/arm overlays | PACo movies play here. Clicks here drive the scanner and the arm. |
| **Control screen** (right, around x 340-495, y 55-145) | Context panel: node screen, door/elevator panel, computer, probe controls, scan results | One "installed interface" at a time (§2.1). |
| **Info strip** (top right, at about (342,19)) | Short text clips: location names, messages | Small 144×32 PACo clips (`*.T.<facing>`, message clips). |
| **Map** | Level map or whole-ship overview with probe and defender icons | Three map modes (§11). |
| **Navigation buttons** (6) | Up, Down, Forward, Back, Turn left, Turn right | Each shows single or double arrow state (§4.6). |
| **Function buttons** (4) | Scan, Jam, Arm, Probe controls | §6, §9.5, §7, §10. |
| **Mode button** (3 positions) | Map mode 1/2/3 | |
| **Game controls** | Quit, Save, Pause, Notes | §12. |
| **Warning light / energy** | Warning lamp with 3 states; energy bar on the node screen | §9.4, §10.1. |
| **Volume slider** | 8 steps (0 = mute) | |

### 2.1 Installed interfaces (control screen)

`Default` (node screen), `Door`, `Elevator`, `Computer`, `Controls`, `Scan`. Clicks on the
control screen are routed by the current one. Opening the scanner, arm or controls closes the
others.

---

## 3. Game data

### 3.1 The node tables share one index ✅

The Node, Map, Ports, Scan 1-5 and Def tables all have **one row per node at the same line
number**. Line N in every table describes the same place, so the line number is the node's
global index. Exits store **relative line offsets** ("+7 lines"), so header and separator lines
count. `helixdb::NavGraph` keeps original line numbers for this reason.

- Probe start: line 87 (node 4.04), facing North. ✅
- Defender start: line 5 (node 1.04). ✅
- 110 nodes over 6 levels. Level sizes (node numbers per level): 23, 41, 5, 7, 16, 10. ✅

### 3.2 Table summary

| Table | Per row | Used by |
|---|---|---|
| Node | id, exit per facing (offset + sound), ladder up/down, door per facing, elevator up/down (E nodes) | §4 |
| Map | level-map position, ship-overview position, in-ladder position | §11 |
| Ports | probe-arm docking point per facing | §7 |
| Scan 1-5 | per facing: scan point (position, item, DNA) and a second item; one table per skill level | §6 |
| Def | the defender's search tree, line of sight, door/ladder/level jumps | §9 |
| DeathDB | 4 rows per node (one per facing): which defender approaches trigger which death video | §9.6 |
| Cmp | computer menus and action records | §8 |
| Clearance groups (11 small lists) | which DNA ids satisfy clearance 02-12 | §5.2 |
| GameVidList (2 members) | video clue placements per skill level | §8.3 |
| Notes source | one line per video clue: a code and where to execute it | §8.3 |
| ProbeFail | node.facing spots where the probe may fail | §10.2 |
| NoArm | node.facing spots where the arm can't deploy | §7 |
| Reserved codes (list in a script) | codes that must not be generated as authorization codes | §8.4 |

Field layouts are documented in `crates/helixdb` (nav.rs, defender.rs, death.rs, computer.rs).

### 3.3 Skill levels

Five skill levels (1 easiest), chosen on the boot screen. ✅ (five Scan tables and five clue
lists; the boot menu art should be checked ❓). Skill affects: which Scan table is used, which
video clue list is used, defender speed and how often it moves after the player (§9.1), jam
duration (§9.5).

---

## 4. Navigation

### 4.1 Moves

At each node the probe has a **facing**. Available actions:

| Action | Allowed when | Video | Result |
|---|---|---|---|
| Forward | exit for current facing ≠ 0, and no door ahead (or the door is open) | `<node>.F<facing>` (in a ladder: `<node>.F`) | line += exit offset |
| Back | exit for the opposite facing ≠ 0, and **no door behind** | `<node>.B<facing>` (in a ladder: `<node>.B`) | line += that offset; facing unchanged |
| Turn left / right | not in a ladder | `<node>.L<facing>` / `<node>.R<facing>` (facing *before* the turn) | facing −1 / +1 (mod 4) |
| Up / Down | node has a ladder entry whose "from facing" = current facing (and, for repeat moves, only while on ladder nodes) | `<node>.U` / `<node>.D` | line += ladder offset; facing := ladder's "facing after"; enters ladder state |

- Each exit carries a **movement sound** number (default 1): `ProbeMove1-3`. Up/down use
  `ProbeMoveUp`/`ProbeMoveDown`; turns use `Probe Rotate`. The sound starts as the video starts.
- Turn videos at **elevator nodes** (ids containing `E`) are filed under node `0` (shared
  elevator interior). ✅
- **Ladder state** ends when a move lands on a node whose id has no `L`.
- Check: the moves implied by the tables match the 375 movement videos on the Mac CD exactly
  (`helixdb videos`). ✅

### 4.2 Special case: the science lab bend ✅

Nodes **2.14** and **2.15** sit on a curved corridor: moving between them changes the facing
(forward from 2.14 facing E ends facing S; back from 2.14 facing W ends facing N; back from 2.15
facing S ends facing E; forward from 2.15 facing N ends facing W).

### 4.3 Doors ✅

- Each node has an optional **door per facing** with a 2-digit **clearance code**.
- When facing a door, the control screen shows the **door panel** (one open/close button;
  Enter/Return also works). Elevator nodes show the elevator panel instead (§4.4).
- Code **`00`**: permanently locked: beep + "locked doors" message.
- Opening requires clearance (§5.2). Success: video `<node>.O<facing>` + `Air Latch Open`.
  Failure: `Access Denied` sound + "access denied (DNA)" message.
- Closing: video `<node>.C<facing>` + `Air Latch Close`.
- Moving forward through an open door, or backing away from it, **closes it behind** the probe
  (`Air Latch Close`, no video).
- Opening a door **into the defender's line of sight kills the probe** (§9.6, door death).
- With **door sensors active** (default), opening a door alerts the defender, which plans a path
  to the probe's node, unless its current plan already covers both sides of that door. Sensors
  can be switched off from a computer (§8.5).
- **Garbage chute**: the door at node 5.04 facing West. Opening it (sound `GarbageChute Open`)
  adds a temporary Down exit (towards level 6); closing removes it.

### 4.4 Elevators ✅

- Elevator nodes (`*.E`) have a door like any other plus a **floor panel** (one button per level,
  number keys 1-7 also work).
- Choosing another level: closes the door if open, plays `Elevator2`, then rides one floor per
  100 ticks, stepping the line by the node's **elevator up / down offset** per floor (Node items
  12/13), updating the map each floor; on arrival the door opens.

### 4.5 After every move ✅

1. The map icon animates to the new position.
2. The **warning level** is recomputed (§9.4). If it is 0, the defender catches the probe (§9.6).
3. Otherwise the defender may take a step (§9.1); then the node screen is set up (§4.7),
   including the random **probe failure** check (§10.2).

### 4.6 Repeat moves ("double arrow") ✅

Tab (or the double-arrow button state) makes a move **repeat**: the probe keeps going in that
direction, playing each segment's video, until something stops it: a door ahead (forward) or
behind (back), leaving a ladder (up/down), a catch by the defender, or a probe failure at low
energy. A double turn turns twice (180°).

### 4.7 Node screen ✅

On arriving at a node the control screen shows:

- **Energy bar** (§10.1) and **4 scan indicators**: which facings (relative to the current one)
  have a scan point; the bars flicker randomly while shown.
- The **info strip** plays the location clip `<node>.T.<facing>`, or, if this node holds a video
  clue for this game and phase > 1, a "message waiting" clip instead.
- If a door is ahead: the door/elevator panel.

### 4.8 Keyboard ✅

| Key | Action |
|---|---|
| ← / → | turn left / right (not in a ladder) |
| ↑ / ↓ | forward / back (if the button is active) |
| U / D | ladder up / down |
| Tab | toggle repeat moves |
| Space | jam the defender |
| Enter/Return | door button (door panel); confirm save in the quit dialog |
| 0-9, Backspace | authorization keypad (§8.4) |
| 1-7 | elevator floor |
| Cmd-Q | quit |

(The original also had a debug key combination that granted DNA and skipped to phase 2.)

---

## 5. DNA and clearance

### 5.1 The DNA kit ✅

- Holds up to **3** DNA samples (crew member ids `01`-`12`; id 12 is stored as 11 ❓ why).
- Managed from **probe controls → DNA inventory**: shows the samples, select any, delete selected.

### 5.2 Clearance check ✅

A door or computer item asks for a clearance code:

- `01` is always granted.
- Once **phase > 1**, everything is granted.
- Otherwise the code selects one of 11 **clearance groups** (codes 02-12), each a set of DNA ids;
  any sample in the kit from that set grants access.
- Docking at a computer port needs clearance **02** (§7).

---

## 6. Scanning

### 6.1 Scan points ✅

Each node/facing may have a scan point (from the skill level's Scan table): screen position,
**item number** (selects the videos) and **DNA id** found there.

### 6.2 Auto scan (default on) ✅

Toggled in probe controls. Pressing Scan:

- Needs energy above empty + 3, else beep. Costs **2 energy**.
- If the current facing has a point: "positive" text clip, a cycling target marker appears at
  the point; clicking within 20 px of it **enhances** (§6.3).
- If not: "negative" text clip + beep.

### 6.3 Manual scan ✅

With auto scan off, pressing Scan shows a scan box in the view. Clicking/dragging makes the box
glide toward the mouse (max speed 20 px/step) inside the target area (x 51-232, y 37-123), with
random flashes and blips while moving. When the box is within 20 px (h) / 14 px (v) of the point
it locks on; after 4 blip cycles it **enhances**. Moving away cancels the lock.

### 6.4 Enhance and acquire ✅

- Enhance: video `Scan:<item>.ScanEnhance` (sound `Enhance`), then the DNA description clip
  `Scan:ScanText:<dna>.ScanTxt`. The control screen switches to the Scan interface:
  **Acquire sample** or **Exit**.
- Acquire: bank full (3) → "sample bank full" + beep; already held → "duplicate" + beep;
  otherwise video `Scan:<item>.ScanAcquire` (sound `Acquire`), "acquire successful", sample added.
- **Phase 1 → 2** when the kit contains **01 and 02 and (10 or 04)** (§13).

---

## 7. The probe arm ✅

- Not available at **NoArm** spots ("unable to deploy" + beep).
- The arm extends into the view and follows the mouse: horizontal extension in 27 steps
  (mouse x mapped onto them), vertical position clamped; it eases toward the target with a motion
  sound, `Arm Stop` when it arrives.
- If the current facing has a **port** (Ports table) and the arm comes within 30 px, it snaps
  onto the port (`Arm Stop`, `Arm HotSpot` while held).
- Releasing while docked: clearance **02** → open the **computer** (§8); otherwise an
  "access denied" clip.
- Pressing Arm again retracts it.

---

## 8. Computers

### 8.1 Screens and navigation ✅

The computer replaces the node screen. It starts at the **Main** menu. The Cmp table holds
records; a record is a header (type `Menu` or `Action`, a screen name) plus lines for options,
locations and (for actions) codes, win actions, movies and task variables. Screen names match
artwork (Main, Command, Engine, Local, Mission 1-3, Weapon, Auth, Crew, Security, AntiMat,
Reactor, Field, Warp, Incin, Load, Navigation...).

Bottom bar: **Exit**, **Prev** (close a playing clip), **Back/Fwd** (page through multi-page
screens), **Message** (Main only, when a video clue is here). Rows 1-4 are the menu items.

Each option has a function and arguments:

| Function | Meaning |
|---|---|
| Hier | open another record (sub-menu), by line |
| Auth | open the authorization keypad (§8.4) |
| Movi | play a named computer clip (some clips also trigger a function) |
| XCmp | leave the computer (retracts the arm) |
| Trav | page to the next/previous screen of a multi-page record |

### 8.2 Item requirements ✅

Menu rows 1-4 also check:

- **Clearance** (§5.2) from the option, else "access denied (DNA)".
- **Location**: the record lists, per row, which `node.facing` ports may use it (or anywhere);
  else "access denied (location)". So some commands only work from particular terminals.

### 8.3 Video clues and notes ✅

- At the start of a game one of two **clue lists** is picked at random; the skill level picks a
  line of it. Each entry places a video at a `node-facing` port, with a video number and a
  **kind mark** (`+` or `*`).
- At that port (phase > 1) the Main menu offers **Message**: plays `Video:Video NN`. The first
  time, the clue becomes "available" and the clue's note line (a code and where to execute it)
  is added to the top of the player's **Notes** (replacing "No video clues recorded").
- **Phase 2 → 3** once the available clues include at least one `*` and one `+` clue.

### 8.4 Authorization codes ✅

- At game start, **four random 4-digit codes** are generated (1000-9999, never one of a fixed
  list of codes that appear elsewhere as decoys). The first is "the" authorization code; the
  other three are **mission (target) codes**. They are shown on the Mission computer screens.
- The keypad takes up to 7 digits (beep when full), with clear and backspace; Enter checks,
  Cancel returns.
- Checking an Action item, in order:
  1. Each item has up to two **clue-gated codes**: valid only if that video clue has been seen.
     A match runs the item's **win action**.
  2. The Weapon screen's third item accepts the three mission codes (→ Target 1/2/3).
  3. The generated authorization code runs the item's **movie action**.
  4. Anything else: "access denied (code)".

### 8.5 Actions and defender tasks ✅

Running an action plays its computer clip and may carry a **task**:

- A task marked as a **function** runs immediately (e.g. turn door sensors off, shut the
  defender down, trap it, kill it).
- Other tasks are **queued for the defender**: about a minute later it "responds" and a
  `<task>BOL` message plays (on the computer or as node text).
- **Door sensors off** works for 5 minutes: it sets the defender's return timer, and when that
  expires the defender processes its queued "door sensors" task, which switches them back on.
- Tasks marked as **endings** only work in phase 4 with the defender off or trapped (§13);
  otherwise a "rejected" clip plays and the defender reacts.

---

## 9. The defender

### 9.1 When it moves ✅

- **Timer**: every `(6 − skill) × 50 + random(1..150) + 100` ticks (skill 1: ~5.9-8.3 s,
  skill 5: ~2.5-5 s). The delay is re-rolled every `random(1..10) + 5` steps.
- **After player moves**: if it isn't off, jammed or trapped, it also steps once the player has
  made more than `max(4 − skill, 0)` moves since its last step (at skill 4-5, every second move).
- Each step it follows one entry of its **planned path**; with no plan it picks a new random
  target (§9.2).

### 9.2 Choosing where to go ✅

- **Roaming**: a random node on a *different* level (uniform over that level's node numbers).
- **Alerted**: towards the probe's node, when a door is opened with door sensors active (§4.3).

### 9.3 Path planning ✅

The Def table is a **binary search tree over node numbers** within each level (items: parent,
left/right child offsets, left/right subtree maxima). To reach a target:

1. If the target is on another level: go to this level's ladder node (levels 1-2 heading above
   level 2 use node 17 or 20; level 5 → 6 uses node 4; otherwise the row's ladder jump), climb
   (the row's up/down level jumps) until on the target level, then step off the ladder.
2. On deck 2, the tree is split into two sections (nodes 3-16 vs the rest), joined between 2.13
   and 2.23; crossing sections routes through that link.
3. Search the tree for the target (left if within the left subtree's range, right if within the
   right's, else up to the parent).
4. If the target is across a door, add the step through it.
5. A plan longer than 40 steps is abandoned for a random target.

Each planned step also records that node's **line-of-sight** list, used for warnings.

### 9.4 Warning levels ✅

| Level | Condition | Effect |
|---|---|---|
| 0 | the probe's node is in the defender's **current** line of sight (or the probe is at an open door facing into it, written `<facing>-<node>`) | caught (§9.6) |
| 1 | in the line of sight of its **next** planned step | lamp + `Warning One` every 20 ticks |
| 2 | in the sight of the step after (and one more if on the same level) | lamp + `Warning Two` every 40 ticks |
| 3 | otherwise | ambient lamp flicker every 90 ticks |
| 4 | defender off | no warnings |

### 9.5 Jamming ✅

Jam (button or Space): needs energy > empty + 10, costs **10 energy**. If the defender is on, it
is **jammed** until its next step, which is delayed (400 ticks at skill 1, 250 otherwise). Its
map icon blinks while jammed.

### 9.6 Being caught ✅

- The DeathDB row for (probe node, facing) lists groups of conditions. If a group matches the
  defender's last move (from-node → to-node) or a door death at the defender's node, play that
  group's death video `<node>.<facing letter>.<n>` (n = group number), then `FadeToGreen`.
- Otherwise: `Static` sound, `DeathStatic`, `FadeToGreen`.
- Then a new probe (§10.3).

### 9.7 Disabling the defender ✅

All are computer actions (§8.5); doing it in the wrong place plays a "wrong location" clip.

| Method | Works when | Result |
|---|---|---|
| Bulkheads | defender at 2.05, 2.06 or 2.07 | **trapped** for 5 minutes (it can't move) |
| Turn off | always | **off** |
| Incinerate | defender at 6.05 or 6.06 | **off** |
| Eject | defender at 6.08 or 6.09 | **off** ("right location" clip) |

On success: its path and warnings clear, warning level 4, the matching segue plays, and a
**5-minute return timer** starts. When it expires the defender comes back (respawning at line 5
if it was off), map switches to the defender view, and if the probe is in sight it is caught.
Phase 3 → 4 on success (also from phase 2).

Door sensors off (§8.5) uses the same 5-minute timer; the sensors come back on when it expires.

---

## 10. The probe

### 10.1 Energy ✅

- Bar from 473 (full) to 399 (empty): 74 units.
- Costs: auto scan 2, jam 10. Refilled to full on each **phase change**.
- Near empty (within 5 units): moves into ProbeFail spots fail for certain (§10.2), and a failure
  then costs the probe ("power low").

### 10.2 Probe failure ✅

- At **ProbeFail** spots (node.facing), on arrival there is a 1-in-50 chance (certain when energy
  is within 5 of empty) that the probe fails: video `Probe Failure:<node>.<facing>.PC`, nav
  buttons disappear.
- If energy is low: "power low" message, then the probe is lost (§10.3, no-energy segue).
- Otherwise "reboot" message: the player reboots from probe controls: a boot-up light show over
  the interface, then `Probe Failure:<node>.<facing>.RB`, then normal play resumes.

### 10.3 Losing a probe ✅

Segue `Segues:DefDeath<n>` (or `NoEnergy<n>`) for probe n. With probes left: show minutes
remaining, pause 2 s, the next probe enters (`Opening:Entering Ship` + `Entering Ship PPV`) and
play restarts from the start position. After the 3rd: `GameOver`, then a random `Video:Lose 1-3`,
back to the title.

---

## 11. Map ✅

| Mode | Shows |
|---|---|
| 1 | the probe's level; the defender's icon only if it's on that level |
| 2 | the defender's level; the probe's icon only if it's on that level |
| 3 | whole-ship overview (both icons, overview coordinates) |

- Probe icon shows its facing (4 frames). Icons glide to new positions in straight lines
  (probe ~2 px/step, defender ~7). Ladder positions use the in-ladder coordinates.
- Defender icon hidden when it is off; blinks when jammed.

---

## 12. Game flow, controls, save

### 12.1 Startup ✅

`Opening:OpenCreditsAndTitle` → bridge background + `Opening:OPENING` → title screen with the
theme looping. Title: skill buttons, Begin, Load saved game, Replay intro, About
(`Segues:About`), Quit (`Segues:Credits` then exit).

### 12.2 New game ✅

Generate codes and pick the clue list (§8.3-8.4); empty DNA kit; auto scan on; phase 1; notes
reset; door sensors active; defender on; clock starts (90 min); probe 1. Plays `Video:Intro`,
`Opening:Entering Ship`, `Opening:Entering Ship PPV`, then play begins.

### 12.3 Clock ✅

- 90 real minutes. Spoken warnings when 60, 30 and 1 minute remain (`60 Min`, `30 Min`,
  `1 Min`). A `15 min` sound ships but the script never reaches it.
- Pausing (and the quit dialog) stops the clock.
- The ETA screen places the ship icon along a line by fraction of time used.
- Time out: `GameOver` + random `Lose` video.

### 12.4 Game controls ✅

Quit (dialog: Quit / Cancel / Save & Quit), Save, Pause (any key/click resumes), Notes (overlay
of the notes field on the control screen; not while scanning).

### 12.5 Save game contents ✅

Probe number, defender node, probe node, facing, phase, trapped flag; volume, energy, minutes
played, warning-interval state, skill; defender return time, defender tasks, task time,
defender-off flag; DNA kit; probe line and defender line; clue list; clues available; door
sensors and the codes; notes text. Loading restores these and replays a turn video to redraw the
view. The new engine can use its own format.

---

## 13. Winning ✅

In **phase 4**, with the defender **off or trapped**, run an ending action (an Action item
unlocked by a clue-gated code). Plays `Misc Files:EndofShip`, then a random `Video:Win 1-2`, back
to the title. Outside those conditions the ending is rejected and the defender reacts.

---

## 14. Assets

### 14.1 Video folders (Mac names; PC and Sega use 8.3 equivalents)

| Folder | Contents |
|---|---|
| `Navigation/L<level>/` | per node: `F/B<dir>` moves, `L/R<dir>` turns, `U`/`D` ladders, `O/C<dir>` door open/close, `T.<facing>` location strips, `<dir>.<n>` death scenes |
| `Probe Failure/` | `<node>.<facing>.PC` failure, `.RB` reboot |
| `Scan/` | `<item>.ScanEnhance`, `<item>.ScanAcquire`; `ScanText/` text clips |
| `Computer/` | computer clips (incl. `<task>BOL.Cmp`, `AccessDenied*`, `WrongLoc.*`, `Win.Reject`) |
| `Messages/` | node-text messages (`<task>BOL.Mssg`, `AccessDeniedDNA.Mssg`, `LockedDoorsTxt`) |
| `Video/` | `Video NN` clue messages, `Intro`, `Win 1-2`, `Lose 1-3` |
| `Segues/` | `DefDeath<n>`, `NoEnergy<n>`, disable-method segues, `About`, `Credits` |
| `Opening/` | intro, title, `Entering Ship (PPV)`, `Phase1-3` |
| `Misc Files/` (installed) | `DeathStatic`, `FadeToGreen`, `GameOver`, `EndofShip`, `Incin`, `MessgText`, `Paused`, `PowerLow`, `ReBoot`, `UnableDeploy` |

### 14.2 Sounds

Installed AIFF sounds: `1/15/30/60 min`, `Access Denied`, `Acquire`, `Air Latch Open/Close`,
`Door Action`, `Elevator2`, `Enhance`, `Entering`, `GarbageChute Open`, `Probe Rotate`,
`ProbeMove1-3`, `ProbeMoveUp/Down`, `ScanInit`, `SoundCheck`, `Static`, `SysBeeb`. Cast sounds:
menu click, arm stop/lock/motion/hotspot, scan blip, warning one/two, auto scan, jamming.

### 14.3 Tuning constants

| Constant | Value |
|---|---|
| Game length | 90 min |
| Probes | 3 |
| Energy full / empty | 473 / 399 |
| Auto-scan cost / jam cost | 2 / 10 |
| Jam needs energy above | empty + 10 |
| Probe-failure chance | 1 in 50 (certain within 5 of empty) |
| Defender step delay | `(6 − skill)·50 + rand(150) + 100` ticks |
| Defender run before re-roll | `rand(10) + 5` steps |
| Jammed delay | 400 ticks (skill 1), 250 otherwise |
| Defender disabled / trapped | 5 min |
| DNA kit size | 3 |
| Elevator | 100 ticks per floor |
| Scan lock radius (manual) | 20 h / 14 v px; auto: 20 px |
| Arm dock radius | 30 px |
| Defender max plan length | 40 steps |

---

## 15. Open questions

- ❓ Number of skill buttons on the title screen (data supports 5).
- ❓ Why DNA 12 is stored as 11.
- ❓ Node 2.19 carries large values in the elevator columns; probably unused or map-related.
- ❓ Second Scan item per facing (the scripts read only the first); may be unused.
- ❓ Exact meaning of the remaining Cmp "vars" items and Def items not read by the scripts.
- Compare against the PC `HELIX.EXE` where behavior seems ambiguous; the user reports the PC
  and Mac versions play the same.

---

## 16. Implementation plan (Rust)

Status: data (1) and navigation (§4: moves, turns, repeat moves, ladders, doors, the garbage
chute, elevators, the science-lab bend) are done in `helix-core`, with the screen laid out from
the score in `helix-app`. Clearance, the defender and everything after are next.

1. **Data** (done): `director` reads the projector; `helixdb` parses the tables.
2. **Rules core** (no graphics): a deterministic `GameState` with the probe, defender, DNA kit,
   phase, clock and energy; `apply(Action) -> Vec<Effect>` where effects are "play video X",
   "play sound Y", "show message Z". Seeded RNG. Unit tests per section of this document.
3. **Defender**: path planner over the Def tree (§9.3), timers (§9.1), warning levels (§9.4).
4. **Computer**: record interpreter (§8), keypad, tasks.
5. **Presentation**: window, view-window video playback (decoded PACo frames), control screen
   artwork from the Director cast (`BITD`/`CLUT` decoding still to do), map, input.
6. **Asset mapping**: Mac names → PC/Sega files for the definitive edition; Sega exclusives.
