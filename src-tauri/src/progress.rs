#[derive(Debug, PartialEq)]
pub enum Line {
    Title(String),
    Percent(f32),
}

pub fn parse(line: &str) -> Option<Line> {
    if let Some(t) = line.strip_prefix("ytdown-title:") {
        return Some(Line::Title(t.trim().to_string()));
    }
    let p = line.strip_prefix("ytdown:")?.trim().trim_end_matches('%').trim();
    p.parse().ok().map(Line::Percent)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses() {
        assert_eq!(parse("ytdown-title:My Video"), Some(Line::Title("My Video".into())));
        assert_eq!(parse("ytdown: 42.3%"), Some(Line::Percent(42.3)));
        assert_eq!(parse("ytdown:100%"), Some(Line::Percent(100.0)));
        assert_eq!(parse("[download] Destination: x"), None);
    }
}
