use crate::track::Track;
use std::collections::{BTreeMap, HashMap, HashSet};

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

    /// Parses the body of an XSPF playlist (the XML dialect emitted by
    /// tools like foobar2000 and MusicBee). Only `<trackList>/<track>`
    /// children are read: `<location>`, `<title>`, `<creator>`,
    /// `<duration>`. A track with no `<location>` is dropped, same as a PLS
    /// entry with no `File<n>`. `<duration>` is milliseconds per the XSPF
    /// spec and is converted to whole seconds; anything negative or
    /// unparsable is normalized to `None`. `file://` locations have the
    /// scheme stripped and are percent-decoded; other schemes (`http://`,
    /// etc.) are kept as-is. CDATA sections aren't handled - values are
    /// expected as plain escaped text, which is what every XSPF export
    /// I've come across actually produces.
    pub fn parse_xspf(name: impl Into<String>, contents: &str) -> Playlist {
        let mut tracks = Vec::new();

        for track_xml in extract_elements(contents, "track") {
            let Some(location) = extract_element(track_xml, "location") else { continue };
            let path = decode_location(&location);
            let title = extract_element(track_xml, "title").map(|s| unescape_xml(&s));
            let artist = extract_element(track_xml, "creator").map(|s| unescape_xml(&s));
            let duration_secs = extract_element(track_xml, "duration")
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|ms| *ms >= 0)
                .map(|ms| ms / 1000);
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

    /// Paths that occur more than once, in order of first appearance, paired
    /// with how many times each occurs. Same path different case, or a
    /// relative vs. absolute form of the same file, are treated as distinct;
    /// this only catches exact string duplicates.
    pub fn duplicate_paths(&self) -> Vec<(&str, usize)> {
        let mut counts: HashMap<&str, usize> = HashMap::new();
        for track in &self.tracks {
            *counts.entry(track.path.as_str()).or_insert(0) += 1;
        }

        let mut seen = HashSet::new();
        self.tracks
            .iter()
            .filter_map(|track| {
                let path = track.path.as_str();
                let count = counts[path];
                if count > 1 && seen.insert(path) { Some((path, count)) } else { None }
            })
            .collect()
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

/// Finds every top-level `<tag>...</tag>` element in `xml` and returns its
/// inner text, untrimmed and still XML-escaped. Not a general XML parser -
/// it doesn't track nesting depth, so it assumes `tag` doesn't contain
/// itself (true for `track` inside `trackList`). A self-closing `<tag/>`
/// has no inner text and is skipped, matching how `parse_xspf` treats a
/// track with no `<location>`.
fn extract_elements<'a>(xml: &'a str, tag: &str) -> Vec<&'a str> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let mut elements = Vec::new();
    let mut rest = xml;

    while let Some(start) = find_tag_start(rest, &open) {
        let after_open = &rest[start..];
        let Some(gt) = after_open.find('>') else { break };
        if after_open.as_bytes()[gt - 1] == b'/' {
            rest = &after_open[gt + 1..];
            continue;
        }
        let body_start = gt + 1;
        let Some(end) = after_open[body_start..].find(&close) else { break };
        elements.push(&after_open[body_start..body_start + end]);
        rest = &after_open[body_start + end + close.len()..];
    }

    elements
}

fn extract_element(xml: &str, tag: &str) -> Option<String> {
    extract_elements(xml, tag).into_iter().next().map(|s| s.trim().to_string())
}

/// Locates the next `open` (e.g. `"<track"`) whose following byte is
/// whitespace, `>`, or `/`, so `<track>` matches but `<trackList>` doesn't.
fn find_tag_start(xml: &str, open: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(idx) = xml[search_from..].find(open) {
        let pos = search_from + idx;
        let after = pos + open.len();
        match xml.as_bytes().get(after) {
            Some(b) if b.is_ascii_whitespace() || *b == b'>' || *b == b'/' => return Some(pos),
            None => return Some(pos),
            _ => search_from = pos + open.len(),
        }
    }
    None
}

fn decode_location(raw: &str) -> String {
    let unescaped = unescape_xml(raw);
    match unescaped.strip_prefix("file://") {
        Some(rest) => percent_decode(rest),
        None => unescaped,
    }
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn unescape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }

        let mut entity = String::new();
        let mut closed = false;
        for _ in 0..16 {
            match chars.peek() {
                Some(';') => {
                    chars.next();
                    closed = true;
                    break;
                }
                Some(&ec) => {
                    entity.push(ec);
                    chars.next();
                }
                None => break,
            }
        }

        if !closed {
            out.push('&');
            out.push_str(&entity);
            continue;
        }

        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ if entity.starts_with('#') => {
                let digits = &entity[1..];
                let code = match digits.strip_prefix('x').or_else(|| digits.strip_prefix('X')) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => digits.parse::<u32>().ok(),
                };
                if let Some(ch) = code.and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            _ => {
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            }
        }
    }

    out
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
    fn parses_xspf_with_artist_title_and_duration() {
        let xspf = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
                    <playlist version=\"1\" xmlns=\"http://xspf.org/ns/0/\">\n\
                    <trackList>\n\
                    <track>\n\
                    <location>file:///music/roygbiv.flac</location>\n\
                    <title>Roygbiv</title>\n\
                    <creator>Boards of Canada</creator>\n\
                    <duration>245000</duration>\n\
                    </track>\n\
                    </trackList>\n\
                    </playlist>\n";
        let playlist = Playlist::parse_xspf("test", xspf);

        assert_eq!(playlist.tracks.len(), 1);
        let track = &playlist.tracks[0];
        assert_eq!(track.path, "/music/roygbiv.flac");
        assert_eq!(track.title.as_deref(), Some("Roygbiv"));
        assert_eq!(track.artist.as_deref(), Some("Boards of Canada"));
        assert_eq!(track.duration_secs, Some(245));
    }

    #[test]
    fn parses_xspf_with_multiple_tracks_and_no_metadata() {
        let xspf = "<playlist><trackList>\
                    <track><location>a.mp3</location></track>\
                    <track><location>b.mp3</location></track>\
                    </trackList></playlist>";
        let playlist = Playlist::parse_xspf("test", xspf);

        assert_eq!(playlist.tracks.len(), 2);
        assert_eq!(playlist.tracks[0].path, "a.mp3");
        assert_eq!(playlist.tracks[1].path, "b.mp3");
        assert!(playlist.tracks[0].title.is_none());
    }

    #[test]
    fn xspf_track_without_location_is_dropped() {
        let xspf = "<playlist><trackList>\
                    <track><title>Orphan</title></track>\
                    <track><location>b.mp3</location></track>\
                    </trackList></playlist>";
        let playlist = Playlist::parse_xspf("test", xspf);

        assert_eq!(playlist.tracks.len(), 1);
        assert_eq!(playlist.tracks[0].path, "b.mp3");
    }

    #[test]
    fn xspf_unescapes_entities_in_title() {
        let xspf = "<playlist><trackList><track>\
                    <location>a.mp3</location>\
                    <title>Rock &amp; Roll &lt;live&gt;</title>\
                    </track></trackList></playlist>";
        let playlist = Playlist::parse_xspf("test", xspf);

        assert_eq!(playlist.tracks[0].title.as_deref(), Some("Rock & Roll <live>"));
    }

    #[test]
    fn xspf_negative_duration_is_normalized_to_none() {
        let xspf = "<playlist><trackList><track>\
                    <location>stream.mp3</location>\
                    <duration>-1</duration>\
                    </track></trackList></playlist>";
        let playlist = Playlist::parse_xspf("test", xspf);

        assert_eq!(playlist.tracks[0].duration_secs, None);
    }

    #[test]
    fn xspf_keeps_non_file_uris_as_is() {
        let xspf = "<playlist><trackList><track>\
                    <location>http://example.invalid/stream.mp3</location>\
                    </track></trackList></playlist>";
        let playlist = Playlist::parse_xspf("test", xspf);

        assert_eq!(playlist.tracks[0].path, "http://example.invalid/stream.mp3");
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
    fn duplicate_paths_reports_repeated_paths_in_first_seen_order() {
        let m3u = "b.mp3\na.mp3\nb.mp3\na.mp3\na.mp3\nc.mp3\n";
        let playlist = Playlist::parse_m3u("test", m3u);

        let duplicates = playlist.duplicate_paths();

        assert_eq!(duplicates, vec![("b.mp3", 2), ("a.mp3", 3)]);
    }

    #[test]
    fn duplicate_paths_is_empty_when_all_paths_are_unique() {
        let m3u = "a.mp3\nb.mp3\n";
        let playlist = Playlist::parse_m3u("test", m3u);

        assert!(playlist.duplicate_paths().is_empty());
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
