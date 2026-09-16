# playlistdb

A small Rust library for reading M3U/M3U8 and PLS playlists and reporting on
what's in them: track count, total run time, how many entries are missing
artist/title metadata. No dependencies, standard library only.

I keep playlists exported from a few different players and none of them
agree on what counts as "duration unknown" or how consistently they fill in
track labels. Rather than eyeball each file, this gives me one parser per
format and one summary format I can point any of them at, in a form scripts
can consume (`--json`-shaped output) or a human can read directly.

This crate has no binary. It's meant to be pulled into your own CLI or
tool; see below for the couple of lines that wires it up.

## What it does today

- Parses M3U/M3U8 text into a `Playlist` of `Track`s (path, title, artist,
  duration).
- Parses PLS text the same way, keyed off the `File<n>`/`Title<n>`/
  `Length<n>` triples and ordered by index rather than line order.
- `#EXTINF:-1,...` and PLS's `Length<n>=-1` (both conventions for "duration
  unknown") are normalized to `None` rather than kept as a sentinel value.
- Computes total duration and a count of tracks missing title or artist.
- Flags tracks that share the same path, in the order they first appear,
  with how many times each one repeats.
- Renders a `Summary` either as plain text or as JSON, from the same data,
  so a caller's own `--json` flag can select the shape without touching the
  underlying logic.
- Writes a `Playlist` back out to M3U text (`to_m3u`), round-tripping
  through `parse_m3u`.

## Usage

```rust
use playlistdb::{OutputFormat, Playlist, Summary};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let path = args.first().expect("usage: playlist-tool <file.m3u> [--json]");
    let format = OutputFormat::from_args(args.iter().map(|s| s.as_str()));

    let contents = std::fs::read_to_string(path).expect("could not read playlist");
    let playlist = Playlist::parse_m3u(path.as_str(), &contents);

    println!("{}", Summary::new(&playlist).render(format));
}
```

Human output:

```
playlist: favorites.m3u
tracks: 3
total duration: 12:05
missing metadata: 1
  [4:05] Boards of Canada - Roygbiv
  [3:40] Aphex Twin - Xtal
  [?] track_09.flac
```

JSON output (`--json`):

```json
{"name":"favorites.m3u","track_count":3,"total_duration_secs":725,"missing_metadata_count":1,"duplicate_paths":[],"tracks":[{"path":"../music/roygbiv.flac","title":"Roygbiv","artist":"Boards of Canada","duration_secs":245},{"path":"../music/xtal.flac","title":"Xtal","artist":"Aphex Twin","duration_secs":220},{"path":"track_09.flac","title":null,"artist":null,"duration_secs":null}]}
```

## Status

Early. M3U/M3U8 and PLS in, plain-text and JSON summaries out, M3U writer
for round-tripping a `Playlist`, duplicate-path detection. Not yet handling
XSPF, and no pretty-printed JSON option yet.
