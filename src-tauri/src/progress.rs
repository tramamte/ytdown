#[derive(Debug, PartialEq)]
pub enum Line {
    Title(String),
    Progress { percent: f32, format_id: String, is_video: bool },
    Formats(Vec<String>),
    File(String),
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
    let mut it = line.strip_prefix("ytdown:")?.trim().splitn(3, '|');
    let percent = it.next()?.trim().trim_end_matches('%').trim().parse().ok()?;
    let format_id = it.next().unwrap_or("").trim().to_string();
    // "none" = audio-only stream; NA/unknown is treated as video
    let is_video = it.next().is_none_or(|v| v.trim() != "none");
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
    fn parses_real_ytdlp_lines() {
        // captured from yt-dlp 2026.08.19 (jNQXAC9IVRw, default settings)
        assert_eq!(parse("ytdown-formats:395+251"), Some(Line::Formats(vec!["395".into(), "251".into()])));
        assert_eq!(parse("ytdown: 58.1%|395|av01.0.00M.08.0.110.05.01.06.0"), pr(58.1, "395", true));
        assert_eq!(parse("ytdown:  0.4%|251|none"), pr(0.4, "251", false));
    }
}
