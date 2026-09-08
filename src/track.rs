/// A single entry in a playlist. `title` and `artist` are absent when the
/// source playlist gave no `#EXTINF` line for the entry, or gave one with no
/// usable label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub path: String,
    pub title: Option<String>,
    pub artist: Option<String>,
    /// Negative durations in the source (M3U uses -1 for "unknown") are
    /// normalized to `None` rather than kept as-is.
    pub duration_secs: Option<i64>,
}
