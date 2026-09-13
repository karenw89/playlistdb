use crate::track::Track;
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Playlist {
    pub name: String,
    pub tracks: Vec<Track>,
}

impl Playlist {
    pub fn new(name: impl Into<String>) -> Self {
        Playlist { name: name.into(), tracks: Vec::new() }
    }

    /// Parses the body of an M3U/M3U8 file. Any `#EXTINF` line applies only to
    /// the next non-comment line; a path with no preceding `#EXTINF` gets no
    /// metadata. Other `#`-prefixed lines (including `#EXTM3U`) are skipped.
    pub fn parse_m3u(name: impl Into<String>, contents: &str) -> Playlist {
        let mut tracks = Vec::new();
        let mut pending: Option<(Option<i64>, Option<String>, Option<String>)> = None;

        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(rest) = line.strip_prefix("#EXTINF:") {
                pending = Some(parse_extinf(rest));
                continue;
            }
            if line.starts_with('#') {
                continue;
            }

            let (duration_secs, artist, title) = pending.take().unwrap_or((None, None, None));
            tracks.push(Track { path: line.to_string(), title, artist, duration_secs });
        }

        Playlist { name: name.into(), tracks }
    }

    /// Parses the body of a PLS file. Entries are grouped by the trailing
    /// index on each key (`File1`, `Title1`, `Length1`, ...); an entry with
    /// no `File<n>` key is dropped since there's no path to point at.
    /// `Length<n>` of -1 (PLS's "unknown duration" convention) is normalized
    /// to `None`, matching the M3U parser.
    pub fn parse_pls(name: impl Into<String>, contents: &str) -> Playlist {
        let mut entries: BTreeMap<u32, PlsEntry> = BTreeMap::new();

        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('[') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let value = value.trim();
            let Some((prefix, index)) = split_key_index(key.trim()) else {
                continue;
            };

            let entry = entries.entry(index).or_default();
            match prefix.as_str() {
                "file" => entry.file = Some(value.to_string()),
                "title" => entry.title = Some(value.to_string()),
                "length" => entry.length = value.parse::<i64>().ok(),
                _ => {}
            }
        }

        let mut tracks = Vec::new();
        for (_, entry) in entries {
            let Some(path) = entry.file else { continue };
            let duration_secs = entry.length.filter(|d| *d >= 0);
            let (artist, title) = match entry.title {
                Some(label) => match label.split_once(" - ") {
                    Some((artist, title)) => {
                        (Some(artist.trim().to_string()), Some(title.trim().to_string()))
                    }
                    None => (None, Some(label)),
                },
                None => (None, None),
            };
            tracks.push(Track { path, title, artist, duration_secs });
        }

        Playlist { name: name.into(), tracks }
    }

    /// Serializes back to M3U text. Round-trips with `parse_m3u`: a track
    /// gets an `#EXTINF` line only if it has a duration, title, or artist to
    /// report, and an unknown duration is written back out as `-1` (the same
    /// convention `parse_m3u` reads it from).
    pub fn to_m3u(&self) -> String {
        let mut out = String::from("#EXTM3U\n");
        for track in &self.tracks {
            if track.duration_secs.is_some() || track.title.is_some() || track.artist.is_some() {
                let duration = track.duration_secs.unwrap_or(-1);
                let label = match (&track.artist, &track.title) {
                    (Some(artist), Some(title)) => format!("{} - {}", artist, title),
                    (None, Some(title)) => title.clone(),
                    (Some(artist), None) => artist.clone(),
                    (None, None) => String::new(),
                };
                out.push_str(&format!("#EXTINF:{},{}\n", duration, label));
            }
            out.push_str(&track.path);
            out.push('\n');
        }
        out
    }

    pub fn total_duration_secs(&self) -> i64 {
        self.tracks.iter().filter_map(|t| t.duration_secs).sum()
    }

    pub fn missing_metadata_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.title.is_none() || t.artist.is_none()).count()
    }
}

#[derive(Default)]
struct PlsEntry {
    file: Option<String>,
    title: Option<String>,
    length: Option<i64>,
}

/// Splits a PLS key like `Title12` into (`"title"`, `12`). Keys with no
/// trailing digits (`Version`, `NumberOfEntries`) return `None` so callers
/// can ignore them.
fn split_key_index(key: &str) -> Option<(String, u32)> {
    let digit_count = key.chars().rev().take_while(|c| c.is_ascii_digit()).count();
    if digit_count == 0 {
        return None;
    }
    let split_at = key.len() - digit_count;
    let index = key[split_at..].parse::<u32>().ok()?;
    Some((key[..split_at].to_lowercase(), index))
}

fn parse_extinf(rest: &str) -> (Option<i64>, Option<String>, Option<String>) {
    let (duration_part, label) = rest.split_once(',').unwrap_or((rest, ""));

    let duration_secs = duration_part.trim().parse::<i64>().ok().filter(|d| *d >= 0);

    let label = label.trim();
    if label.is_empty() {
        return (duration_secs, None, None);
    }

    match label.split_once(" - ") {
        Some((artist, title)) => {
            (duration_secs, Some(artist.trim().to_string()), Some(title.trim().to_string()))
        }
        None => (duration_secs, None, Some(label.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_extinf_with_artist_and_title() {
        let m3u = "#EXTM3U\n#EXTINF:245,Boards of Canada - Roygbiv\n../music/roygbiv.flac\n";
        let playlist = Playlist::parse_m3u("test", m3u);

        assert_eq!(playlist.tracks.len(), 1);
        let track = &playlist.tracks[0];
        assert_eq!(track.path, "../music/roygbiv.flac");
        assert_eq!(track.artist.as_deref(), Some("Boards of Canada"));
        assert_eq!(track.title.as_deref(), Some("Roygbiv"));
        assert_eq!(track.duration_secs, Some(245));
    }

    #[test]
    fn path_without_extinf_has_no_metadata() {
        let m3u = "song.mp3\n";
        let playlist = Playlist::parse_m3u("test", m3u);

        assert_eq!(playlist.tracks.len(), 1);
        assert!(playlist.tracks[0].title.is_none());
        assert_eq!(playlist.missing_metadata_count(), 1);
    }

    #[test]
    fn unknown_duration_is_normalized_to_none() {
        let m3u = "#EXTINF:-1,Live Stream\nhttp://example.invalid/stream\n";
        let playlist = Playlist::parse_m3u("test", m3u);

        assert_eq!(playlist.tracks[0].duration_secs, None);
    }

    #[test]
    fn parses_pls_with_artist_and_title() {
        let pls = "[playlist]\n\
                    NumberOfEntries=2\n\
                    File1=../music/roygbiv.flac\n\
                    Title1=Boards of Canada - Roygbiv\n\
                    Length1=245\n\
                    File2=../music/xtal.flac\n\
                    Title2=Aphex Twin - Xtal\n\
                    Length2=220\n\
                    Version=2\n";
        let playlist = Playlist::parse_pls("test", pls);

        assert_eq!(playlist.tracks.len(), 2);
        assert_eq!(playlist.tracks[0].path, "../music/roygbiv.flac");
        assert_eq!(playlist.tracks[0].artist.as_deref(), Some("Boards of Canada"));
        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Roygbiv"));
        assert_eq!(playlist.tracks[0].duration_secs, Some(245));
        assert_eq!(playlist.tracks[1].path, "../music/xtal.flac");
    }

    #[test]
    fn pls_entries_are_ordered_by_index_not_file_order() {
        let pls = "[playlist]\nTitle2=Second\nFile2=b.mp3\nTitle1=First\nFile1=a.mp3\n";
        let playlist = Playlist::parse_pls("test", pls);

        assert_eq!(playlist.tracks[0].path, "a.mp3");
        assert_eq!(playlist.tracks[1].path, "b.mp3");
    }

    #[test]
    fn pls_unknown_length_is_normalized_to_none() {
        let pls = "[playlist]\nFile1=stream.mp3\nTitle1=Live Stream\nLength1=-1\n";
        let playlist = Playlist::parse_pls("test", pls);

        assert_eq!(playlist.tracks[0].duration_secs, None);
    }

    #[test]
    fn pls_entry_without_file_is_dropped() {
        let pls = "[playlist]\nTitle1=Orphan Title\nFile2=b.mp3\n";
        let playlist = Playlist::parse_pls("test", pls);

        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].path, "b.mp3");
    }

    #[test]
    fn to_m3u_round_trips_through_parse_m3u() {
        let m3u = "#EXTM3U\n\
                   #EXTINF:245,Boards of Canada - Roygbiv\n\
                   ../music/roygbiv.flac\n\
                   #EXTINF:-1,Live Stream\n\
                   http://example.invalid/stream\n\
                   song.mp3\n";
        let original = Playlist::parse_m3u("test", m3u);

        let rewritten = original.to_m3u();
        let reparsed = Playlist::parse_m3u("test", &rewritten);

        assert_eq!(reparsed.tracks, original.tracks);
    }

    #[test]
    fn to_m3u_skips_extinf_for_tracks_with_no_metadata() {
        let mut playlist = Playlist::new("test");
        playlist.tracks.push(Track { path: "plain.mp3".to_string(), title: None, artist: None, duration_secs: None });

        let m3u = playlist.to_m3u();

        assert_eq!(m3u, "#EXTM3U\nplain.mp3\n");
    }

    #[test]
    fn to_m3u_writes_unknown_duration_as_negative_one() {
        let mut playlist = Playlist::new("test");
        playlist.tracks.push(Track {
            path: "stream.mp3".to_string(),
            title: Some("Live Stream".to_string()),
            artist: None,
            duration_secs: None,
        });

        let m3u = playlist.to_m3u();

        assert!(m3u.contains("#EXTINF:-1,Live Stream\n"));
    }
}
