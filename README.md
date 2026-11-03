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
crates/helix-tools   command line tools (`paco`, `macx`)
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
  track. Same folder layout as the PC (`NAV`, `SEGUES`, `VIDEO`, ...), but videos are `.SEG`
  files in Sega FILM containers (`FILM`/`FDSC`/`STAB` chunks) with a `sega` codec, not PACo.
