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
crates/helix-tools   command line tools (`paco`, `macx`, `isox`, `sfilm`, `tomp4`, `xref`)
../Reference         original game files (not in the repo)
out/                 generated output (gitignored)
```

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
  unchanged". If a line ends exactly at the right edge, the cursor has already wrapped and the
  escape is a no-op.
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
  forks, which are classic StuffIt 1.5 (`SIT!`) archives, not yet supported.
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
