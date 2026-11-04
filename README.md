# IronHelixRS

A Rust reimplementation of the engine for *Iron Helix* (Drew Pictures / Spectrum HoloByte, 1993),
built by reverse engineering the original game. It runs on data from your own copy of the game;
no game content lives in this repository.

The long-term goal is a "definitive edition" that pulls the best assets from the Mac, Windows
and Sega CD releases (including the Sega CD's extra ending video).

## Layout

```
crates/paco          library: PACo (CAVF) movie parser, RLE decoder, frame compositor
crates/macformats    library: StuffIt 5 (+Arsenic), resource forks, DiskCopy NDIF (+ADC), HFS
crates/cdimage       library: ISO9660 on raw (2352-byte) or cooked (2048-byte) CD images
crates/segafilm      library: Sega FILM container, "Cinepak for Sega" video, Sega CD PCM audio
crates/director      library: Director 3 movies in resource forks (cast, text members, score scripts, labels)
crates/helixdb       library: the game's data tables (navigation graph, defender, deaths, computers)
crates/helix-tools   command line tools (`paco`, `macx`, `isox`, `sfilm`, `tomp4`, `xref`, `helixdb`)
crates/helix-core    the game rules, no graphics or audio
crates/helix-app     the game itself (`ironhelix`), built on macroquad for desktop and mobile
../Reference         original game files (not in the repo)
out/                 generated output (gitignored)
```

## What you need

The game runs on files from your own copies of the original releases. Only the **Mac release** is
required; the PC and Sega CD releases each add something if you have them. The paths below are
where the game looks by default (relative to this folder); each can be changed on the command
line.

| Release | Files you start with | What the game reads | Without it |
|---|---|---|---|
| **Mac** (required) | `../Reference/Game/Mac/Iron-Helix_Mac_EN.sit`, a StuffIt archive holding the CD image `Iron Helix™ CD.img` | From the CD: the `Iron Helix™ Files` folder (movies), at `out/mac/files/Iron Helix™ Files` (`--mac-files`). From `Iron Helix™ Installer` on the CD: the game program's resource fork `Iron Helix™.rsrc` (its scripts, tables, artwork and sounds), at `out/mac/app/Iron Helix™/Iron Helix™.rsrc` (`--mac-app`), with the installed `Iron Helix Sounds` and `Misc Files` folders beside it | The game won't start |
| **PC** (optional) | The `HELIX` folder from the PC CD, copied as is to `../Reference/Game/Windows/HELIX` | `HELIX.EXE` (the hidden rooms' node data), the movies in `NAV`, `SEGUES` and so on, and the credits tune `OPENING/CREDTUNE.WAV` (`--pc-files`) | No hidden rooms (2.42/2.43 and the stasis room 4.08/4.09); quitting shows the Mac's silent credit pages instead of the PC's scrolling credits with their tune |
| **Sega CD** (optional) | The disc image `../Reference/Game/SegaCD/Iron Helix (US)/Iron Helix (US).cue` with its `.img` | The theme from the image's audio track 2, straight from the `.cue`/`.img` (`--sega-cue`); the extended ending `SEGUES/ENDING.SEG` from the files extracted to `out/sega/files` (`--sega-files`) | The Mac's tune on the title screen; no extended ending after a skill-5 win |

Setting up, in order (the Sega step is only for the extended ending; the theme needs no setup):

```
cargo build --release

# Mac: recover the disc image's resource fork (which Windows drops), open the image, extract
# the CD, then unpack the installed game from its installer
target/release/macx sit-extract ../Reference/Game/Mac/Iron-Helix_Mac_EN.sit -o out/mac --rsrc-only
target/release/macx ndif "../Reference/Game/Mac/Iron Helix™ CD.img" --rsrc "out/mac/Iron Helix™ CD.img.rsrc" -o out/mac/helix.hfs
target/release/macx hfs-extract out/mac/helix.hfs -o out/mac/files
target/release/macx sit-extract "out/mac/files/Iron Helix™ Installer" -o out/mac/app

# Sega CD (optional): extract the disc's files
target/release/isox extract "../Reference/Game/SegaCD/Iron Helix (US)/Iron Helix (US).img" -o out/sega/files

# Play
cargo run --release -p helix-app
```

The PC folder needs no extracting. Everything extracted stays in `out/`, which is never
committed. The sections further down explain each tool.

## Running the game: `ironhelix`

The whole game plays: the opening, the title screen (skill 1-5, begin, load, replay the intro,
about, quit), and the game in the original interface (laid out from the Director score), with the
navigation buttons, movement movies and sounds, location strips, doors, ladders, elevators and
the map in all three modes. Scanning works (auto and manual, enhance and acquire), along with the
DNA kit and probe controls, door clearance, the energy bar and the phase changes, whose movies
play on the bridge screen. The probe arm docks at computer ports, and the computers run from the
game's own table: menus, the keypad with this game's generated codes, the Mission and Crew
screens, video messages (clues), and the actions, including the ones aimed at the defender
(bulkheads, turning it off, incinerating or ejecting it), with their segues. The defender roams
the ship by its own map, comes for the probe when a door opens with the sensors on, warns with
the lamp and its sounds as it nears, can be jammed, trapped, turned off or thrown out, comes back
five minutes later, and catches the probe with the original's death scenes; after three probes
the game is over and it's back to the title. Probes break down now and then at certain spots
(1 in 50, or for certain with the power nearly gone) and come back with a reboot from the probe
controls. The 90-minute clock runs in real time with its spoken warnings at 60, 30 and 1
minutes left, and ends the game when it runs out. The game controls bar works: Quit (Quit,
Cancel or Save and quit), Save, Pause and Notes (the codes the video messages have given, over
the view). See [docs/design.md](docs/design.md).

```
cargo run --release -p helix-app -- [--mac-app <Iron Helix™.rsrc>] [--mac-files <Iron Helix™ Files>]
    [--pc-files <HELIX>] [--sega-cue <cue>] [--sega-files <folder>] [--save <file>]
    [--skill 1-5] [--seed N] [--skip-title]
```

Defaults are the paths in [What you need](#what-you-need) (sounds come from the
`Iron Helix Sounds` folder next to the game program; override with `--mac-sounds`). The window
takes the Mac release's icon, the probe, from the game program's resource fork.

With the PC CD's `HELIX` folder (`--pc-files`, default `../Reference/Game/Windows/HELIX` when it
exists) the game adds the **PC release's hidden rooms**: 2.42/2.43 behind the "Zero-Pressure"
door at 2.08 south, and the stasis room 4.08/4.09 behind 4.05 north. Their node data is read
from the player's `HELIX.EXE` and their movies from the PC CD. As on the PC, those
battle-damaged doors open on deck 2 from phase 2 and on deck 4 in phase 3 (design doc §14.4);
until phases are played, **F2** cycles the phase for testing. The PC shipped German location
strips for the stasis room in its English folder; the game rebuilds English ones from two
English strips as they load.

With the Sega CD (`--sega-cue`, default `../Reference/Game/SegaCD/Iron Helix (US)/Iron Helix
(US).cue` when it exists) the title plays the **Sega CD's theme** from the disc image's audio
track 2 (44.1 kHz stereo) in place of the Mac's tune. With its extracted files (`--sega-files`,
default `out/sega/files`), **winning on skill 5** goes on to the Sega CD's **extended ending**
(`SEGUES/ENDING.SEG`, 98 s) after the win movie.

The game saves to one slot, `IronHelix/save.txt` in the user's application data folder
(`%APPDATA%` on Windows, `~/Library/Application Support` on macOS, `~/.local/share` elsewhere),
or `--save <file>`; Load Saved Game on the title screen picks it up.

Controls: click or tap the arrows, the function buttons (Scan, Arm, CTRL), the control screen's
panels and the map mode switch; with the scanner on, click (auto scan) or drag (manual scan) in
the view; with the arm out, press and drag in the view to move it onto a port. Keys: arrows move
and turn, U/D ladders, Tab repeat moves (double arrows), Enter door, 1-6 elevator floor (facing
the elevator door), 0-9 and Backspace (cancel) on the computer keypad; and, not in the original,
S scan, X arm, C probe controls, M map mode, Enter on the keypad. Space or the Jam button jams
the defender; N shows the notes; P pauses; Esc brings up the quit dialog (Q quit, S save and
quit, Esc cancel). The notes use the default font here; the original draws them in 9-point
Geneva. F pixel-perfect scaling, H debug overlay. On the title screen: 1-5 skill, Enter begin,
Esc quit; a click or key skips the opening movies. `--skill` picks the title's starting skill,
`--skip-title` goes straight into a game. Computer codes show in a small built-in font; the
original draws them with Mac text.

Testing aids: `--window 2400x1080` tries other screen shapes; `--autoplay "SK(400,60)A"` plays a
sequence of steps in a game (with `--title`, starting at the title screen, a step every half
second: a click there skips a movie) (F/B forward/back, L/R turn, U/D ladder, O door, 1-6
elevator, M map mode, P phase, S scan, X arm, C controls, K click the scan point (or, with the arm out, the port), A
acquire, G grant DNA 01/02/10 and skip to phase 2 like the original's debug keys, Y be caught by
the defender, J jam, N notes, digits on the keypad, `(x,y)` a click at stage coordinates);
`--screenshot <png>` saves the screen (after the autoplay, or after a few seconds; or at
`--screenshot-frame N`) and exits; `--seed N` makes a run repeatable; `--play-ending` plays the
Sega CD ending at once; `--debug` starts with the overlay on (skill, energy, DNA kit, minutes
left, a failed probe, phase, probe, the defender's node, warning level and plan, this game's
codes and video clues, defender tasks).

The stage is the original **512×384 (4:3)**, scaled to fit the screen and centered. On wider
screens the space on each side is filled with artwork from **`assets/side_panel.png`** (plain
black if it's missing):

- The texture is drawn at the full screen height, **anchored to its right edge** (the edge that
  touches the stage) and cropped on the left when the space is narrower than the art. It is never
  stretched unless the art is too narrow.
- The right-hand panel is the same texture mirrored.
- Each side is 1/6 of the screen width on 16:9 and up to ~0.44 × the screen height on 20:9
  phones, so art about **0.45 × its height wide** (e.g. 500×1080) covers common phones. Put the
  important detail near the right edge.
- Screens taller than 4:3 get plain black bars above and below for now.

## The `paco` tool

```
cargo build --release
target/release/paco info   <movie>               header, opcodes, per-frame decode stats
target/release/paco frames <movie> -o <dir>      one indexed PNG per frame
target/release/paco gif    <movie> -o <file>     animated GIF preview
target/release/paco apng   <movie> -o <file>     lossless animated PNG
target/release/paco export <dir> -o <dir>        whole folder tree to .apng (+ .wav for movies with sound)
target/release/paco check  <dir>                 decode everything, report failures
```

Example: `target/release/paco export ../Reference/Game/Windows/HELIX -o out/windows`

`paco digest <dir>` prints a hash of each movie's decoded pixels, for comparing releases.
`paco timing <dir>` compares each movie's frame delays with its sound's length.

## Getting the Mac files out: `macx`

The Mac release is a StuffIt 5 archive holding a compressed DiskCopy 6 image of an HFS CD. If the
`.img` was extracted on Windows, its resource fork (which holds the image's chunk map) was lost;
`macx` recovers it from the `.sit`:

```
target/release/macx sit-extract ../Reference/Game/Mac/Iron-Helix_Mac_EN.sit -o out/mac --rsrc-only
target/release/macx ndif "../Reference/Game/Mac/Iron Helix™ CD.img" --rsrc "out/mac/Iron Helix™ CD.img.rsrc" -o out/mac/helix.hfs
target/release/macx hfs-extract out/mac/helix.hfs -o out/mac/files
```

Resource forks are written next to each file as `<name>.rsrc`; type/creator codes go to
`_finder.tsv`. `macx hfs-ls` lists a volume without extracting.

## The game's logic and data: `macx director` and `helixdb`

The Mac game program is inside `Iron Helix™ Installer`, whose data fork is a classic StuffIt
(`SIT!`) archive. The program is a **Director 3 projector** (creator `MMPB`): its resource fork
holds a Director movie, and Director 3 stored Lingo as source text, so the game's scripts and
data tables are all readable text members.

```
target/release/macx sit-extract "out/mac/files/Iron Helix™ Installer" -o out/mac/app
target/release/macx director "out/mac/app/Iron Helix™/Iron Helix™.rsrc" -o out/mac/director
target/release/helixdb summary "out/mac/app/Iron Helix™/Iron Helix™.rsrc"
target/release/helixdb nodes   "out/mac/app/Iron Helix™/Iron Helix™.rsrc" --level 1
target/release/helixdb videos  "out/mac/app/Iron Helix™/Iron Helix™.rsrc" --nav "out/mac/files/Iron Helix™ Files/Navigation"
```

`macx director` sorts the movie's text into `scripts/` (Lingo), `tables/` and `text/`, plus the
score scripts, frame labels and cast names, and exports every bitmap to `bitmaps/` as PNG with
its stage position in `bitmaps.tsv`. The movie is **Director 3.1** (the real version is at
offset 36 of `VWCF`), with a **512×384 stage**. Bitmaps (`BITD`) are 8-bit (one is 1-bit),
raw or run-length encoded (a control byte below 0x80 copies the next n+1 bytes, otherwise the
next byte repeats 257−n times), and all use the Mac system palette; the format comes from
[ScummVM's Director engine](https://github.com/scummvm/scummvm/tree/master/engines/director). **These are the game's own assets: they stay in
`out/` and are never committed.** The engine reads them from the player's copy at runtime.

`helixdb` parses the tables (see `crates/helixdb` for the field layouts):

- **Navigation** (Node, Map, Ports, Scan 1-5): 110 nodes over 6 levels. The four tables are
  line-aligned (line N of each describes the same node), and exits are *relative line offsets*,
  so separator lines count. Per node: an exit and a door for each facing (N/E/S/W), ladders, map
  coordinates, the probe arm's target per facing, and scan points per skill level.
- **Defender** (Def): its own graph, arranged as a binary search tree over node numbers for
  pathing, with line-of-sight lists.
- **Deaths** (DeathDB), **computer screens** (Cmp), probe-failure spots and no-arm spots.

Check: the movement videos implied by the graph (`<node>.F<dir>`, `.B<dir>` unless a door is
behind, `.U`/`.D` for ladders) are exactly the 375 on the Mac CD. `helixdb walk <app> --files
<Iron Helix™ Files>` goes further: it explores every state reachable through the actual rules
(helix-core) and checks every movie and clip they request, in every game phase: all 110
nodes, 1,276 movies, none missing. With `--pc <HELIX>` it adds the PC's hidden rooms and looks
up movies on both CDs: all 114 nodes, 1,329 movies, none missing. It also checks the movies
behind every scan point (122) and everything the computers can play: menu clips, keypad
actions, fixed messages, video clues and segues (87), none missing, and the death videos the
DeathDB can pick (205; 3 are missing on the CD too, slips in the original's table). `helixdb defender <app>
[--steps N] [--seed N]` lets the defender roam with the probe idle at the start and checks every
step lands on a Def row: over three seeds of 3,000 steps, none off the table, all six decks
visited, the longest plan 29 steps, a step every ~7.6 s at skill 1.

The **score** (`VWSC`) gives the screen layout: a u32 size, then frames of (u8 length, u8
offset) word patches to a 512-byte buffer that carries over between frames: 32 bytes of main
channels and 30 sprite channels of 16 bytes (script, type, colors, ink, cast member, loc v/h,
height/width). A sprite's loc is where its bitmap's registration point goes. `macx director`
writes it out as `score.tsv`.

**[docs/design.md](docs/design.md)** describes how the whole game works (navigation, doors,
DNA, scanning, the arm, computers, video clues, the defender's AI, deaths, energy, the clock and
the win conditions), written from the Lingo in our own words as the spec for the Rust rebuild.

## The Sega CD version: `isox` and `sfilm`

```
target/release/isox extract "../Reference/Game/SegaCD/Iron Helix (US)/Iron Helix (US).img" -o out/sega/files
target/release/sfilm info   out/sega/files/SEGUES/ENDING.SEG
target/release/sfilm apng   out/sega/files/SEGUES/ENDING.SEG -o out/sega/ENDING.apng   (+ ENDING.wav)
target/release/sfilm export out/sega/files -o out/sega/export
target/release/sfilm check  out/sega/files
```

`sfilm frames <movie> -o <dir>` writes one PNG per frame.

## Making MP4s: `tomp4`

`tomp4` converts a movie from any release (Windows or Mac PACo, or Sega CD FILM) to H.264/AAC
MP4, detecting the format by content. It needs [ffmpeg](https://ffmpeg.org/), found via
`--ffmpeg <path>`, then the `FFMPEG` environment variable, then `Tools/FFMPEG/bin` in or next to
the current folder, then `PATH`.

```
target/release/tomp4 out/sega/files/SEGUES/ENDING.SEG -o out/ENDING.mp4
target/release/tomp4 ../Reference/Game/Windows/HELIX -o out/mp4/windows     (a whole folder, in parallel)
```

Frames are upscaled 4× with nearest neighbor (`--scale` to change), so pixel art stays sharp.
Sega CD movies are tagged with the Genesis 8:7 pixel aspect so players show them at their TV
shape; PC and Mac pixels are square. Each frame keeps its own duration: a movie with a constant
frame rate is encoded at that rate, anything uneven at 60 fps with frames repeated to match.

## Comparing releases: `xref`

```
target/release/xref out/sega/files --against ../Reference/Game/Windows/HELIX --against "out/mac/files/Iron Helix™ Files" > out/sega_xref.tsv
```

For each movie in the first folder, lists the closest-looking movies in the others (any format).
Fingerprints are 24 sampled frames shrunk to 8×8 brightness grids; the score is the average
distance from each sampled frame to its nearest frame in the candidate, so different frame rates,
lengths, palettes and codecs still match. Around 0.4–0.7 means the same footage. Only movies of
similar shape are compared, so reframed versions (like the Sega computer screens) show up as
unmatched and need a look.

## Format findings

PACo (CoSA's PICS Animation Compiler) writes CAVF files. Primary docs: `CAVF.h` and the RLE notes in
[rfsheffer/PACoPlayer](https://github.com/rfsheffer/PACoPlayer/tree/RLEdecoding/documentation).
Things learned from the actual Iron Helix files that the docs don't say:

- **Big-endian everywhere**, including the Windows release. Header is 38 bytes, then the chunk
  table, then chunks of opcodes. Opcode lengths include their 4-byte header.
- **`COMP_RLE_INTERLACED` = even rows first, then odd rows.** Decoding sequentially gives two
  squashed copies of the image stacked vertically.
- **The compact 4-bit LUT persists across frames**, not just across lines of one bitmap.
- **Escape 5 ("skip scan lines", count 0)** is how change-only frames say "rest of this line is
  unchanged". It always moves down a line, even right after a line that ended exactly at the
  right edge: there it means "the next line is unchanged". (An earlier version treated that case
  as a no-op, which shifted every later line of the field: interlaced movies showed streaks
  through their text, as in the phase movies and some location strips, and 88 of deck 2's
  death clips decoded differently from the PC's copies. With the fix, the Mac and PC copies of
  deck 2 agree on all but 2 movies.)
- **Phantom lines:** the packer often encodes data below the bottom of the area, built from
  leftover memory past its buffer (sometimes recognizable 68k machine code). Usually it's just a
  trailing newline; occasionally literal pixels. Anything written below the area is discarded.
- **Empty frames:** a `0x0` BITMAP (2 bytes of padding) means "nothing changed"; it keeps frame
  count and timing intact.
- **Palette:** movies use the *system* palette, i.e. the classic Mac 8-bit CLUT (index 0 = white).
  `PACO.PAL` on the PC is that palette shifted by one with Windows' black/white at 0/255.
- **Sound:** SOUND opcodes hold unsigned 8-bit mono PCM at 11,025 Hz (rate codes 0 and 2), not
  locked to frame timing. Matches the standalone WAVs the PC version also ships.

All 3,353 PACo movies in the Windows release decode with zero errors.

## Other releases

- **Mac (v1.0.1, July 1993):** 2,343 files; 2,333 are PACo movies (type `CAVF`, creator `PACP`),
  all decoding cleanly. Same resolutions as the PC (240×128 navigation, 160×130 video, 144×32
  strips) and the same palette, so **no video-quality advantage**. 1,982 Mac movies are
  pixel-identical to a PC movie; the rest differ in packaging (interlacing, frame rate, embedded
  sound vs. the PC's separate WAVs), re-rendered random static effects, and a slightly different set
  of small 144×32 navigation strips. The game application itself is inside the installers' data
  forks (classic StuffIt `SIT!` archives with stored entries), and it is a Director 3 projector
  whose Lingo and data tables are readable (see above).
- **Sega CD:** `.img` is Mode 1 / 2352-byte-sector ISO9660 (volume `MEGA_CD`) plus a CD audio
  track. Same folder layout as the PC (`NAV`, `SEGUES`, `VIDEO`, ...), but the videos are Sega
  FILM files using the "Cinepak for Sega" codec (see below).

### What the Sega CD has that the PC and Mac don't

Found by fingerprinting every movie (`xref`, below) and checking the leftovers by eye:

- **`SEGUES/ENDING`**: a 97.8-second extended ending with no PC/Mac counterpart: the research
  vessel *Indiana*, a starbase, the tractor beam in "arrest mode", docking, interiors, and
  live-action footage of the special guest star (Cintra Wilson, per the Sega credits).
- **`OPENING/OPENING`** and **`SML_OPEN`**: a re-edited opening with shots not in the PC/Mac
  files compared (a "TARGET: NONUS-7" orbit display, an Earth-like planet).
- **`OPENING/OPENCRDT`**: opening credits with an animated 3D Drew Pictures logo.
- **`SEGUES/CREDITS`**: end credits adding the Sega CD team (Brian A. Rice, Inc.).
- **Re-edited segues**: `DEATH1/2` and `ENERGY1/2` follow the "probe destroyed" message with the
  probe-launch footage (reused from `ENTER`) that the PC versions don't show there.
- **Sega-specific screens**: title card (`FIREHELX`), conversion logo (`BARLOGO`), main menu
  (`SELECT`), and all computer screens rebuilt as a wide 234×118 layout with a "Remote DataLink"
  panel and A/C controller prompts.
- **CD audio track 2**: the Iron Helix theme as 44.1 kHz 16-bit stereo CD audio (116.8 s), a
  much better copy than the PC's 11 kHz 8-bit `RIH_TUNE.WAV`.

Not new, despite being Sega-only file names: `VIDEO/CPTSPEE` is the captain's speech (in the PC
opening and the Mac's `CAPT SPEECH`); 32×32 "movies" (`SOUND/*`, `*CLOSE`, `HELIXTUN`) are
audio clips with a dummy picture.

## Sega CD video: "Cinepak for Sega"

Despite the name it has little to do with desktop Cinepak. It is a vector quantizer built around
the Genesis video hardware: 8×8 tiles of 4-bit pixels, each tile drawn with one of up to four
16-color palettes. Each frame (`SM` header) holds the palettes, a 2-bit palette number per tile,
two codebooks (4×2-pixel and 2×2-pixel blocks), a 2-bit coding method per tile, then per-tile
data: a raw tile, 8 indices into the 4×2 codebook, 16 indices into the 2×2 codebook, or nothing
(reuse the tile from two frames back). Sound is 8-bit sign-magnitude PCM played at 16,276 Hz.
See `crates/segafilm/src/cinepak_sega.rs` for the full layout.

What we found in the Iron Helix files on top of that:

- Every frame is a keyframe; the "reuse from two frames back" method is never used.
- 2,333 of 2,334 movies use this codec. The exception, `COMPUTER/ACCDEN.CMP`, uses a different
  `rle ` codec that isn't supported yet.
- A few movies have damaged sample tables (entries all claiming the same length while the real
  frames are shorter and packed back to back, sometimes with padding or a corrupt frame between
  them). The decoder finds the real frame headers, so all of them still decode.

## Credits

The Sega CD video codec was not worked out here. Its format comes from **bgvanbur's
[scdtools](https://github.com/classiccoding/scdtools)** (2011, GPL), whose `scdmoviedecode` and
`scdcinepakencode` scripts decode and encode "Cinepak for Sega", as described in the Sonic Retro
thread [Demystifying Cinepak For Sega](https://forums.sonicretro.org/threads/demystifying-cinepak-for-sega.26243/).
Our first attempt got the tile methods and codebooks right, but we could not work out the
per-tile palettes until we read that work. `crates/segafilm` is an independent Rust
implementation of the format as scdtools describes it; no scdtools code is used.

Also used:

- [MultimediaWiki: Sega FILM](https://wiki.multimedia.cx/index.php/Sega_FILM) (based on Mike
  Melanson's FILM/CPK format document): container layout, keyframe flag, Sega CD sign-magnitude
  audio.
- [TrekkiesUnite118/SegaSaturnFilmMuxer](https://github.com/TrekkiesUnite118/SegaSaturnFilmMuxer):
  cross-check of the FILM header and sample table.
- [rfsheffer/PACoPlayer](https://github.com/rfsheffer/PACoPlayer): CoSA's `CAVF.h` and RLE notes,
  the starting point for the PACo decoder.
- [ScummVM's Director engine](https://github.com/scummvm/scummvm/tree/master/engines/director):
  the Director 3 config, cast record and `BITD` bitmap formats (independently reimplemented).
