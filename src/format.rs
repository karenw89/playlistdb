use crate::playlist::Playlist;
use std::fmt;

/// Which shape to render a `Summary` as. This crate ships no binary of its
/// own; a consuming CLI is expected to map its own `--json` flag onto
/// `OutputFormat::Json` (see `from_args` for a ready-made way to do that).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Human,
    Json,
}

impl OutputFormat {
    pub fn from_args<'a, I: IntoIterator<Item = &'a str>>(args: I) -> OutputFormat {
        if args.into_iter().any(|a| a == "--json") {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        }
    }
}

/// A read-only view over a playlist's stats, renderable as plain text or
/// JSON. Building the JSON by hand (rather than via a serde derive) is the
/// price of the zero-dependency constraint; `json_string` is the one place
/// that has to get escaping right.
pub struct Summary<'a> {
    playlist: &'a Playlist,
}

impl<'a> Summary<'a> {
    pub fn new(playlist: &'a Playlist) -> Self {
        Summary { playlist }
    }

    pub fn render(&self, format: OutputFormat) -> String {
        match format {
            OutputFormat::Human => self.to_string(),
            OutputFormat::Json => self.to_json(),
        }
    }

    pub fn to_json(&self) -> String {
        let mut out = String::new();
        out.push('{');
        out.push_str("\"name\":");
        out.push_str(&json_string(&self.playlist.name));
        out.push_str(",\"track_count\":");
        out.push_str(&self.playlist.tracks.len().to_string());
        out.push_str(",\"total_duration_secs\":");
        out.push_str(&self.playlist.total_duration_secs().to_string());
        out.push_str(",\"missing_metadata_count\":");
        out.push_str(&self.playlist.missing_metadata_count().to_string());
        out.push_str(",\"duplicate_paths\":[");
        for (i, (path, count)) in self.playlist.duplicate_paths().into_iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            out.push_str("\"path\":");
            out.push_str(&json_string(path));
            out.push_str(",\"count\":");
            out.push_str(&count.to_string());
            out.push('}');
        }
        out.push(']');
        out.push_str(",\"tracks\":[");
        for (i, track) in self.playlist.tracks.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            out.push('{');
            out.push_str("\"path\":");
            out.push_str(&json_string(&track.path));
            out.push_str(",\"title\":");
            out.push_str(&json_opt_string(&track.title));
            out.push_str(",\"artist\":");
            out.push_str(&json_opt_string(&track.artist));
            out.push_str(",\"duration_secs\":");
            match track.duration_secs {
                Some(d) => out.push_str(&d.to_string()),
                None => out.push_str("null"),
            }
            out.push('}');
        }
        out.push_str("]}");
        out
    }
}

impl<'a> fmt::Display for Summary<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "playlist: {}", self.playlist.name)?;
        writeln!(f, "tracks: {}", self.playlist.tracks.len())?;
        writeln!(f, "total duration: {}", format_duration(self.playlist.total_duration_secs()))?;
        writeln!(f, "missing metadata: {}", self.playlist.missing_metadata_count())?;
        let duplicates = self.playlist.duplicate_paths();
        if !duplicates.is_empty() {
            writeln!(f, "duplicate tracks (by path): {}", duplicates.len())?;
            for (path, count) in &duplicates {
                writeln!(f, "  [{}x] {}", count, path)?;
            }
        }
        for track in &self.playlist.tracks {
            let label = match (&track.artist, &track.title) {
                (Some(artist), Some(title)) => format!("{} - {}", artist, title),
                (None, Some(title)) => title.clone(),
                _ => track.path.clone(),
            };
            let duration = track.duration_secs.map(format_duration).unwrap_or_else(|| "?".to_string());
            writeln!(f, "  [{}] {}", duration, label)?;
        }
        Ok(())
    }
}

fn format_duration(total_secs: i64) -> String {
    let total_secs = total_secs.max(0);
    format!("{}:{:02}", total_secs / 60, total_secs % 60)
}

fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn json_opt_string(s: &Option<String>) -> String {
    match s {
        Some(s) => json_string(s),
        None => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_escapes_quotes_in_titles() {
        let mut playlist = Playlist::new("test");
        playlist.tracks.push(crate::track::Track {
            path: "a.mp3".to_string(),
            title: Some("say \"hi\"".to_string()),
            artist: None,
            duration_secs: Some(10),
        });

        let json = Summary::new(&playlist).to_json();
        assert!(json.contains("\"title\":\"say \\\"hi\\\"\""));
    }

    #[test]
    fn json_lists_duplicate_paths_with_counts() {
        let mut playlist = Playlist::new("test");
        for _ in 0..2 {
            playlist.tracks.push(crate::track::Track {
                path: "a.mp3".to_string(),
                title: None,
                artist: None,
                duration_secs: None,
            });
        }

        let json = Summary::new(&playlist).to_json();
        assert!(json.contains("\"duplicate_paths\":[{\"path\":\"a.mp3\",\"count\":2}]"));
    }

    #[test]
    fn human_output_lists_duplicates_when_present() {
        let mut playlist = Playlist::new("test");
        for _ in 0..2 {
            playlist.tracks.push(crate::track::Track {
                path: "a.mp3".to_string(),
                title: None,
                artist: None,
                duration_secs: None,
            });
        }

        let human = Summary::new(&playlist).render(OutputFormat::Human);
        assert!(human.contains("duplicate tracks (by path): 1"));
        assert!(human.contains("  [2x] a.mp3"));
    }

    #[test]
    fn human_output_omits_duplicate_section_when_none() {
        let mut playlist = Playlist::new("test");
        playlist.tracks.push(crate::track::Track {
            path: "a.mp3".to_string(),
            title: None,
            artist: None,
            duration_secs: None,
        });

        let human = Summary::new(&playlist).render(OutputFormat::Human);
        assert!(!human.contains("duplicate tracks"));
    }

    #[test]
    fn human_output_marks_unknown_duration() {
        let mut playlist = Playlist::new("test");
        playlist.tracks.push(crate::track::Track {
            path: "a.mp3".to_string(),
            title: None,
            artist: None,
            duration_secs: None,
        });

        let human = Summary::new(&playlist).render(OutputFormat::Human);
        assert!(human.contains("[?]"));
    }
}
