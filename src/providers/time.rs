//! World clock: `time in ny`, `tokyo time`, `time in 2h in london`, `in 3h30m in la`,
//! `3pm in ny` (your 3pm over there), `9am ny` (their 9am here), `9am ny in tokyo`.

use std::sync::LazyLock;

use chrono::{DateTime, Duration, Local, NaiveTime, Offset, TimeZone, Utc};
use chrono_tz::{TZ_VARIANTS, Tz};
use regex::Regex;

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Time;

/// Common short names on top of every IANA city.
const ALIASES: &[(&str, &str)] = &[
    ("ny", "America/New_York"), ("nyc", "America/New_York"), ("new york", "America/New_York"),
    ("boston", "America/New_York"), ("miami", "America/New_York"), ("dc", "America/New_York"),
    ("washington", "America/New_York"), ("est", "America/New_York"), ("edt", "America/New_York"),
    ("east coast", "America/New_York"),
    ("la", "America/Los_Angeles"), ("sf", "America/Los_Angeles"), ("san francisco", "America/Los_Angeles"),
    ("seattle", "America/Los_Angeles"), ("silicon valley", "America/Los_Angeles"),
    ("pst", "America/Los_Angeles"), ("pdt", "America/Los_Angeles"), ("west coast", "America/Los_Angeles"),
    ("california", "America/Los_Angeles"),
    ("texas", "America/Chicago"), ("dallas", "America/Chicago"), ("houston", "America/Chicago"),
    ("austin", "America/Chicago"), ("cst", "America/Chicago"),
    ("colorado", "America/Denver"), ("utah", "America/Denver"),
    ("uk", "Europe/London"), ("england", "Europe/London"), ("britain", "Europe/London"),
    ("france", "Europe/Paris"), ("germany", "Europe/Berlin"), ("munich", "Europe/Berlin"),
    ("switzerland", "Europe/Zurich"), ("geneva", "Europe/Zurich"), ("bern", "Europe/Zurich"),
    ("basel", "Europe/Zurich"), ("lausanne", "Europe/Zurich"), ("ch", "Europe/Zurich"),
    ("italy", "Europe/Rome"), ("milan", "Europe/Rome"), ("spain", "Europe/Madrid"),
    ("barcelona", "Europe/Madrid"), ("portugal", "Europe/Lisbon"), ("netherlands", "Europe/Amsterdam"),
    ("austria", "Europe/Vienna"), ("sweden", "Europe/Stockholm"), ("norway", "Europe/Oslo"),
    ("finland", "Europe/Helsinki"), ("poland", "Europe/Warsaw"), ("greece", "Europe/Athens"),
    ("turkey", "Europe/Istanbul"), ("russia", "Europe/Moscow"), ("ukraine", "Europe/Kyiv"),
    ("cet", "Europe/Berlin"), ("cest", "Europe/Berlin"),
    ("tunisia", "Africa/Tunis"), ("morocco", "Africa/Casablanca"), ("egypt", "Africa/Cairo"),
    ("nigeria", "Africa/Lagos"), ("kenya", "Africa/Nairobi"), ("south africa", "Africa/Johannesburg"),
    ("cape town", "Africa/Johannesburg"),
    ("dubai", "Asia/Dubai"), ("uae", "Asia/Dubai"), ("abu dhabi", "Asia/Dubai"),
    ("saudi", "Asia/Riyadh"), ("israel", "Asia/Jerusalem"), ("tel aviv", "Asia/Jerusalem"),
    ("india", "Asia/Kolkata"), ("delhi", "Asia/Kolkata"), ("new delhi", "Asia/Kolkata"),
    ("mumbai", "Asia/Kolkata"), ("bangalore", "Asia/Kolkata"), ("ist", "Asia/Kolkata"),
    ("china", "Asia/Shanghai"), ("beijing", "Asia/Shanghai"), ("shenzhen", "Asia/Shanghai"),
    ("hk", "Asia/Hong_Kong"), ("japan", "Asia/Tokyo"), ("jst", "Asia/Tokyo"), ("osaka", "Asia/Tokyo"),
    ("korea", "Asia/Seoul"), ("vietnam", "Asia/Ho_Chi_Minh"), ("thailand", "Asia/Bangkok"),
    ("bali", "Asia/Makassar"), ("indonesia", "Asia/Jakarta"), ("philippines", "Asia/Manila"),
    ("australia", "Australia/Sydney"), ("canberra", "Australia/Sydney"),
    ("nz", "Pacific/Auckland"), ("new zealand", "Pacific/Auckland"), ("hawaii", "Pacific/Honolulu"),
    ("brazil", "America/Sao_Paulo"), ("rio", "America/Sao_Paulo"), ("argentina", "America/Argentina/Buenos_Aires"),
    ("buenos aires", "America/Argentina/Buenos_Aires"), ("mexico", "America/Mexico_City"),
    ("canada", "America/Toronto"), ("montreal", "America/Toronto"), ("quebec", "America/Toronto"),
    ("utc", "UTC"), ("gmt", "UTC"), ("zulu", "UTC"),
];

/// (lowercase name, display name, zone)
static PLACES: LazyLock<Vec<(String, String, Tz)>> = LazyLock::new(|| {
    let mut places: Vec<(String, String, Tz)> = ALIASES
        .iter()
        .filter_map(|(name, zone)| Some((name.to_string(), city(zone.parse().ok()?), zone.parse().ok()?)))
        .collect();
    for tz in TZ_VARIANTS {
        if tz.name().starts_with("Etc/") || !tz.name().contains('/') {
            continue;
        }
        let display = city(tz);
        places.push((display.to_lowercase(), display, tz));
    }
    places
});

fn city(tz: Tz) -> String {
    match tz.name() {
        "UTC" => "UTC".into(),
        name => name.rsplit('/').next().unwrap_or(name).replace('_', " "),
    }
}

fn place(name: &str) -> Option<(String, Tz)> {
    let name = name.trim().trim_end_matches('?').trim();
    if name.is_empty() {
        return None;
    }
    let exact = PLACES.iter().find(|(n, _, _)| n == name);
    // `tok` → Tokyo, but only from 3 letters on
    let prefix = || (name.len() >= 3).then(|| PLACES.iter().find(|(n, _, _)| n.starts_with(name))).flatten();
    exact.or_else(prefix).map(|(_, display, tz)| (display.clone(), *tz))
}

/// `2h`, `90 min`, `1h30m`, `2 hours and 15 minutes`, `3 days`
fn duration(text: &str) -> Option<Duration> {
    static PART: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(\d+(?:\.\d+)?)\s*(days?|d|hours?|hrs?|h|minutes?|mins?|m)").unwrap()
    });
    let rest = PART.replace_all(text, "");
    if !rest.replace("and", "").trim().is_empty() {
        return None;
    }
    let mut total = 0.0;
    for cap in PART.captures_iter(text) {
        let n: f64 = cap[1].parse().ok()?;
        total += n * match &cap[2][..1] {
            "d" => 86_400.0,
            "h" => 3_600.0,
            _ => 60.0,
        };
    }
    (total > 0.0).then(|| Duration::seconds(total as i64))
}

/// `3pm`, `3:30 pm`, `15:00`, `noon`, `midnight`
fn clock(text: &str) -> Option<NaiveTime> {
    static CLOCK: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(\d{1,2})(?::(\d{2}))?\s*(am|pm)?$").unwrap());
    match text.trim() {
        "noon" => return NaiveTime::from_hms_opt(12, 0, 0),
        "midnight" => return NaiveTime::from_hms_opt(0, 0, 0),
        _ => {}
    }
    let cap = CLOCK.captures(text.trim())?;
    if cap.get(2).is_none() && cap.get(3).is_none() {
        return None; // a bare number is too ambiguous
    }
    let mut h: u32 = cap[1].parse().ok()?;
    let m: u32 = cap.get(2).map_or(Some(0), |m| m.as_str().parse().ok())?;
    match cap.get(3).map(|m| m.as_str()) {
        Some("pm") if h < 12 => h += 12,
        Some("am") if h == 12 => h = 0,
        Some(_) if h > 12 => return None,
        _ => {}
    }
    NaiveTime::from_hms_opt(h, m, 0)
}

enum Ask {
    /// What time is it in `place`, `shift` from now.
    Now { place: (String, Tz), shift: Option<Duration> },
    /// `time` at `from` is what in `to`.
    Convert { time: NaiveTime, from: Option<(String, Tz)>, to: Option<(String, Tz)> },
}

fn parse(q: &str) -> Option<Ask> {
    let q = q.to_lowercase();
    let q = q.trim().trim_end_matches('?');
    let q = ["what's the ", "whats the ", "what is the ", "current "].iter().fold(q, |q, p| q.strip_prefix(p).unwrap_or(q));

    let now_in = |rest: &str| -> Option<Ask> {
        // `2h in tokyo`, `tokyo in 2h`, `tokyo`
        if let Some((a, b)) = rest.split_once(" in ") {
            if let (Some(shift), Some(place)) = (duration(a), place(b)) {
                return Some(Ask::Now { place, shift: Some(shift) });
            }
            if let (Some(place), Some(shift)) = (place(a), duration(b)) {
                return Some(Ask::Now { place, shift: Some(shift) });
            }
        }
        Some(Ask::Now { place: place(rest)?, shift: None })
    };

    if let Some(rest) = q.strip_prefix("time in ").or_else(|| q.strip_prefix("time at ")).or_else(|| q.strip_prefix("time ")) {
        return now_in(rest.trim_start_matches("in "));
    }
    if let Some(rest) = q.strip_prefix("in ") {
        let (a, b) = rest.split_once(" in ")?;
        return Some(Ask::Now { place: place(b)?, shift: Some(duration(a)?) });
    }
    if let Some(head) = q.strip_suffix(" time") {
        return Some(Ask::Now { place: place(head)?, shift: None });
    }
    if let Some((head, shift)) = q.split_once(" time in ") {
        return Some(Ask::Now { place: place(head)?, shift: Some(duration(shift)?) });
    }

    // conversions: `3pm in ny`, `3pm ny`, `3pm ny in tokyo`
    static CONVERT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(\d{1,2}(?::\d{2})?\s*(?:am|pm)?|noon|midnight)\s+(?:(in|to)\s+)?(.+?)(?:\s+(?:in|to)\s+(.+))?$").unwrap()
    });
    let cap = CONVERT.captures(q)?;
    let time = clock(&cap[1])?;
    let first = place(&cap[3])?;
    let second = match cap.get(4) {
        Some(m) => Some(place(m.as_str())?),
        None => None,
    };
    Some(if cap.get(2).is_some() {
        Ask::Convert { time, from: None, to: Some(first) }
    } else {
        Ask::Convert { time, from: Some(first), to: second }
    })
}

fn offset_text(t: &DateTime<Tz>) -> String {
    let secs = t.offset().fix().local_minus_utc();
    let (sign, secs) = if secs < 0 { ('−', -secs) } else { ('+', secs) };
    match secs % 3600 {
        0 => format!("UTC{sign}{}", secs / 3600),
        m => format!("UTC{sign}{}:{:02}", secs / 3600, m / 60),
    }
}

/// `6h behind you`, `same time as you`
fn relative(t: &DateTime<Tz>) -> String {
    let theirs = t.offset().fix().local_minus_utc();
    let ours = t.with_timezone(&Local).offset().fix().local_minus_utc();
    let diff = theirs - ours;
    if diff == 0 {
        return "same as here".into();
    }
    let h = diff.abs() as f64 / 3600.0;
    let h = if h.fract() == 0.0 { format!("{h}") } else { format!("{h:.1}") };
    format!("{h}h {}", if diff > 0 { "ahead" } else { "behind" })
}

fn day_note(t: &DateTime<Tz>, reference: chrono::NaiveDate) -> String {
    match (t.date_naive() - reference).num_days() {
        0 => String::new(),
        1 => " (tomorrow)".into(),
        -1 => " (yesterday)".into(),
        d => format!(" ({d:+} days)"),
    }
}

fn item(title: String, subtitle: String, copy: String) -> Item {
    Item {
        title,
        subtitle,
        hint: "copy".into(),
        glyph: "\u{f017}".into(),
        action: Action::Copy(copy),
        score: 10_000,
        ..Default::default()
    }
}

impl Provider for Time {
    fn title(&self) -> &str {
        "Time"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        let Some(ask) = parse(cx.query) else { return Vec::new() };
        let now = Utc::now();
        match ask {
            Ask::Now { place: (name, tz), shift } => {
                let t = (now + shift.unwrap_or_default()).with_timezone(&tz);
                let when = match shift {
                    Some(d) => format!("in {}", human(d)),
                    None => "now".into(),
                };
                let title = format!("{}  {}{}", t.format("%H:%M"), t.format("%a %d %b"), day_note(&t, Local::now().date_naive()));
                let subtitle = format!("{name} {when} · {} · {}", offset_text(&t), relative(&t));
                vec![item(title, subtitle, t.format("%H:%M").to_string())]
            }
            Ask::Convert { time, from, to } => {
                let today = Local::now().date_naive().and_time(time);
                // the moment, from the source zone's wall clock
                let moment: DateTime<Utc> = match &from {
                    Some((_, tz)) => tz.from_local_datetime(&today).earliest().map(|t| t.with_timezone(&Utc)),
                    None => Local.from_local_datetime(&today).earliest().map(|t| t.with_timezone(&Utc)),
                }
                .unwrap_or(now);
                let (to_name, to_tz) = to.clone().unwrap_or_else(|| ("here".into(), local_tz()));
                let t = moment.with_timezone(&to_tz);
                let from_name = from.as_ref().map_or("here", |(n, _)| n.as_str());
                let title = format!("{}  {to_name}{}", t.format("%H:%M"), day_note(&t, today.date()));
                let subtitle = format!("{} {from_name} · {}", time.format("%H:%M"), offset_text(&t));
                vec![item(title, subtitle, t.format("%H:%M").to_string())]
            }
        }
    }
}

/// The system zone, for converting into "here".
fn local_tz() -> Tz {
    std::env::var("TZ")
        .ok()
        .or_else(|| std::fs::read_link("/etc/localtime").ok().map(|p| p.to_string_lossy().into_owned()))
        .and_then(|p| p.split("zoneinfo/").last().map(str::to_owned))
        .and_then(|name| name.parse().ok())
        .unwrap_or(Tz::UTC)
}

fn human(d: Duration) -> String {
    let m = d.num_minutes();
    match (m / 1440, m % 1440 / 60, m % 60) {
        (0, 0, m) => format!("{m}m"),
        (0, h, 0) => format!("{h}h"),
        (0, h, m) => format!("{h}h{m:02}m"),
        (d, 0, 0) => format!("{d}d"),
        (d, h, _) => format!("{d}d {h}h"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now_place(q: &str) -> Option<(String, Option<i64>)> {
        match parse(q)? {
            Ask::Now { place, shift } => Some((place.0, shift.map(|d| d.num_minutes()))),
            Ask::Convert { .. } => None,
        }
    }

    #[test]
    fn parses_questions() {
        assert_eq!(now_place("time in ny"), Some(("New York".into(), None)));
        assert_eq!(now_place("tokyo time"), Some(("Tokyo".into(), None)));
        assert_eq!(now_place("time in 2h in london"), Some(("London".into(), Some(120))));
        assert_eq!(now_place("in 1h30m in la"), Some(("Los Angeles".into(), Some(90))));
        assert_eq!(now_place("time in zurich in 2 hours"), Some(("Zurich".into(), Some(120))));
        assert_eq!(now_place("what's the time in tunisia?"), Some(("Tunis".into(), None)));
        assert!(parse("firefox").is_none());
        assert!(parse("time in narnia").is_none());
    }

    #[test]
    fn parses_conversions() {
        let Some(Ask::Convert { time, from, to }) = parse("3pm ny in tokyo") else { panic!() };
        assert_eq!(time, NaiveTime::from_hms_opt(15, 0, 0).unwrap());
        assert_eq!(from.unwrap().0, "New York");
        assert_eq!(to.unwrap().0, "Tokyo");
        let Some(Ask::Convert { from: None, to: Some(to), .. }) = parse("9:30 am in london") else { panic!() };
        assert_eq!(to.0, "London");
        assert!(parse("5 ny").is_none());
    }

    #[test]
    fn reads_durations() {
        assert_eq!(duration("2h").map(|d| d.num_minutes()), Some(120));
        assert_eq!(duration("2 hours and 15 minutes").map(|d| d.num_minutes()), Some(135));
        assert_eq!(duration("tokyo"), None);
    }
}
