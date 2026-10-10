use url::Url;

#[derive(Debug, PartialEq, Clone, Copy, serde::Serialize)]
pub enum UrlKind { Video, Playlist, Invalid }

pub fn classify(input: &str) -> UrlKind {
    let Ok(u) = Url::parse(input.trim()) else { return UrlKind::Invalid };
    if u.scheme() != "https" && u.scheme() != "http" { return UrlKind::Invalid }
    let host = u.host_str().unwrap_or("").trim_start_matches("www.").trim_start_matches("m.");
    if host != "youtube.com" && host != "youtu.be" && host != "music.youtube.com" {
        return UrlKind::Invalid;
    }
    if u.query_pairs().any(|(k, _)| k == "list") { UrlKind::Playlist } else { UrlKind::Video }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn classifies() {
        assert_eq!(classify("https://www.youtube.com/watch?v=abc"), UrlKind::Video);
        assert_eq!(classify("https://youtu.be/abc"), UrlKind::Video);
        assert_eq!(classify("https://www.youtube.com/watch?v=abc&list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://www.youtube.com/playlist?list=PL1"), UrlKind::Playlist);
        assert_eq!(classify("https://example.com/watch?v=abc"), UrlKind::Invalid);
        assert_eq!(classify("--exec evil"), UrlKind::Invalid);
        assert_eq!(classify("file:///etc/passwd"), UrlKind::Invalid);
    }
}
