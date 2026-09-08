use crate::track::Track;

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

    pub fn total_duration_secs(&self) -> i64 {
        self.tracks.iter().filter_map(|t| t.duration_secs).sum()
    }

    pub fn missing_metadata_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.title.is_none() || t.artist.is_none()).count()
    }
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
}
