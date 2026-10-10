#[derive(Debug, PartialEq)]
pub enum Line {
    Title(String),
    Progress { percent: f32, format_id: String, is_video: bool },
    Formats(Vec<String>),
    File(String),
    /// start of a playlist item; 0 / "" when yt-dlp prints NA
    Item { index: u32, total: u32, playlist_title: String },
}

pub fn parse(line: &str) -> Option<Line> {
    if let Some(f) = line.strip_prefix("ytdown-formats:") {
        return Some(Line::Formats(f.trim().split('+').map(String::from).collect()));
    }
    if let Some(t) = line.strip_prefix("ytdown-title:") {
        return Some(Line::Title(t.trim().to_string()));
    }
    if let Some(f) = line.strip_prefix("ytdown-file:") {
        return Some(Line::File(f.trim().to_string()));
    }
    if let Some(f) = line.strip_prefix("ytdown-item:") {
        let mut it = f.trim().splitn(3, '|');
        let mut num = || it.next().and_then(|v| v.parse().ok()).unwrap_or(0);
        let (index, total) = (num(), num());
        let t = it.next().unwrap_or("");
        return Some(Line::Item { index, total, playlist_title: if t == "NA" { "" } else { t }.into() });
    }
    let mut it = line.strip_prefix("ytdown:")?.trim().splitn(5, '|');
    let mut percent = it.next()?.trim().trim_end_matches('%').trim().parse().ok()?;
    let format_id = it.next().unwrap_or("").trim().to_string();
    // "none" = audio-only stream; NA/unknown is treated as video
    let is_video = it.next().is_none_or(|v| v.trim() != "none");
    // fragmented streams (HLS/DASH) have no total size, so yt-dlp's percent is a jittery estimate:
    // use fragment progress instead when available
    let mut num = || it.next().and_then(|v| v.trim().parse::<f32>().ok());
    if let (Some(i), Some(n)) = (num(), num()) && n > 0.0 {
        percent = (i / n * 100.0).clamp(0.0, 100.0);
    }
    Some(Line::Progress { percent, format_id, is_video })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn pr(percent: f32, f: &str, is_video: bool) -> Option<Line> {
        Some(Line::Progress { percent, format_id: f.into(), is_video })
    }
    #[test]
    fn parses() {
        assert_eq!(parse("ytdown-title:My Video"), Some(Line::Title("My Video".into())));
        assert_eq!(parse("ytdown:  0.5%|401|av01.0.08M.08"), pr(0.5, "401", true));
        assert_eq!(parse("ytdown:100.0%|251|none\r"), pr(100.0, "251", false));
        assert_eq!(parse("ytdown: 42.3%|NA|NA"), pr(42.3, "NA", true));
        assert_eq!(parse("ytdown-formats:401+251"), Some(Line::Formats(vec!["401".into(), "251".into()])));
        assert_eq!(parse("ytdown-formats:251\r"), Some(Line::Formats(vec!["251".into()])));
        assert_eq!(parse("ytdown-file:/dl/My  Video.mp4\r"), Some(Line::File("/dl/My  Video.mp4".into())));
        assert_eq!(parse("[download] Destination: x"), None);
    }
    #[test]
    fn fragment_progress_replaces_estimate() {
        let p = |l: &str| match parse(l) { Some(Line::Progress { percent, .. }) => percent, x => panic!("{x:?}") };
        let near = |a: f32, b: f32| assert!((a - b).abs() < 0.1, "{a} vs {b}");
        // captured from yt-dlp 2026.08.19 (QWFaZf3MpKk, stream 616 m3u8_native, total size NA)
        near(p("ytdown:100.0%|616|avc1.640028|0|43"), 0.0);
        near(p("ytdown:  1.2%|616|avc1.640028|1|43"), 2.3);
        near(p("ytdown: 11.3%|616|avc1.640028|1|43"), 2.3);
        near(p("ytdown: 10.1%|616|avc1.640028|1|43"), 2.3);
        near(p("ytdown: 14.0%|616|avc1.640028|5|43"), 11.6);
        // known size (https): no fragments, percent unchanged
        near(p("ytdown: 37.5%|251|none|NA|NA"), 37.5);
        near(p("ytdown: 37.5%|251|none"), 37.5);
    }
    #[test]
    fn parses_real_ytdlp_lines() {
        // captured from yt-dlp 2026.08.19 (jNQXAC9IVRw, default settings)
        assert_eq!(parse("ytdown-formats:395+251"), Some(Line::Formats(vec!["395".into(), "251".into()])));
        assert_eq!(parse("ytdown: 58.1%|395|av01.0.00M.08.0.110.05.01.06.0"), pr(58.1, "395", true));
        assert_eq!(parse("ytdown:  0.4%|251|none"), pr(0.4, "251", false));
    }
    #[test]
    fn parses_playlist_item() {
        let it = |index, total, t: &str| Some(Line::Item { index, total, playlist_title: t.into() });
        // captured from yt-dlp 2026.08.19 (PLW7B4PyySHApSUCjSpXzBd6LXT8IhjvH9)
        assert_eq!(parse("ytdown-item:4|7|gallery 23:5 seconds countdown"), it(4, 7, "gallery 23:5 seconds countdown"));
        assert_eq!(parse("ytdown-item:001|120|a | b\r"), it(1, 120, "a | b")); // zero-padded index, '|' in title
        assert_eq!(parse("ytdown-item:NA|NA|NA"), it(0, 0, ""));
    }
}
