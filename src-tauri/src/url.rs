use url::Url;

#[derive(Debug, PartialEq, Clone, Copy, serde::Serialize)]
pub enum UrlKind { Video, Playlist, PlaylistOnly, Invalid }

pub fn classify(input: &str) -> UrlKind {
    let Ok(u) = Url::parse(input.trim()) else { return UrlKind::Invalid };
    if u.scheme() != "https" && u.scheme() != "http" { return UrlKind::Invalid }
    let host = u.host_str().unwrap_or("").trim_start_matches("www.").trim_start_matches("m.");
    if host != "youtube.com" && host != "youtu.be" && host != "music.youtube.com" {
        return UrlKind::Invalid;
    }
    if !u.query_pairs().any(|(k, _)| k == "list") { return UrlKind::Video }
    let mut seg = u.path_segments().into_iter().flatten();
    let has_video = u.query_pairs().any(|(k, v)| k == "v" && !v.is_empty())
        || host == "youtu.be" && seg.clone().next().is_some_and(|s| !s.is_empty())
        || matches!(seg.next(), Some("shorts" | "live" | "embed"))
            && seg.next().is_some_and(|s| !s.is_empty() && s != "videoseries");
    if has_video { UrlKind::Playlist } else { UrlKind::PlaylistOnly }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies() {
        assert_eq!(classify("https://www.youtube.com/watch?v=abc"), UrlKind::Video);
        assert_eq!(classify("https://youtu.be/abc"), UrlKind::Video);
        assert_eq!(classify("https://www.youtube.com/watch?v=abc&list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://www.youtube.com/playlist?list=PL1"), UrlKind::PlaylistOnly);
        assert_eq!(classify("https://www.youtube.com/watch?list=PL1"), UrlKind::PlaylistOnly);
        assert_eq!(classify("https://www.youtube.com/watch?v=&list=PL1"), UrlKind::PlaylistOnly);
        assert_eq!(classify("https://music.youtube.com/playlist?list=PL1"), UrlKind::PlaylistOnly);
        assert_eq!(classify("https://youtu.be/abc?list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://www.youtube.com/shorts/abc?list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://www.youtube.com/live/abc?list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://www.youtube.com/embed/abc?list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://www.youtube.com/embed/videoseries?list=PL1"), UrlKind::PlaylistOnly);
        assert_eq!(classify("https://example.com/watch?v=abc"), UrlKind::Invalid);
        assert_eq!(classify("--exec evil"), UrlKind::Invalid);
        assert_eq!(classify("file:///etc/passwd"), UrlKind::Invalid);
    }
}
