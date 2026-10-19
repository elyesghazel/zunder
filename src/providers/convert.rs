//! Unit and currency conversion: `5 km in mi`, `30c`, `2.5 gb to mib`, `100 usd in chf`, `€20`.
//! Without a target, units go metric ⇄ imperial and money goes to your currency.

use std::fs;
use std::path::PathBuf;
use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

use crate::engine::{Ctx, Job, Provider};
use crate::item::{Action, Item};

pub struct Convert;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dim {
    Length,
    Mass,
    Volume,
    Area,
    Speed,
    Time,
    Data,
    Energy,
    Temperature,
}

struct Unit {
    /// First name is the one shown.
    names: &'static [&'static str],
    dim: Dim,
    /// In the dimension's base unit (m, kg, l, m², m/s, s, byte, J). Unused for temperature.
    factor: f64,
    /// Where it goes when no target is given.
    default_to: &'static str,
}

const fn u(names: &'static [&'static str], dim: Dim, factor: f64, default_to: &'static str) -> Unit {
    Unit { names, dim, factor, default_to }
}

use Dim::*;
const UNITS: &[Unit] = &[
    u(&["mm", "millimeter", "millimeters", "millimetre", "millimetres"], Length, 0.001, "in"),
    u(&["cm", "centimeter", "centimeters", "centimetre", "centimetres"], Length, 0.01, "in"),
    u(&["m", "meter", "meters", "metre", "metres"], Length, 1.0, "ft"),
    u(&["km", "kilometer", "kilometers", "kilometre", "kilometres"], Length, 1000.0, "mi"),
    u(&["in", "inch", "inches", "\""], Length, 0.0254, "cm"),
    u(&["ft", "foot", "feet", "'"], Length, 0.3048, "m"),
    u(&["yd", "yard", "yards"], Length, 0.9144, "m"),
    u(&["mi", "mile", "miles"], Length, 1609.344, "km"),
    u(&["nmi", "nautical mile", "nautical miles"], Length, 1852.0, "km"),
    u(&["mg", "milligram", "milligrams"], Mass, 1e-6, "g"),
    u(&["g", "gram", "grams"], Mass, 0.001, "oz"),
    u(&["kg", "kilo", "kilos", "kilogram", "kilograms"], Mass, 1.0, "lb"),
    u(&["t", "ton", "tons", "tonne", "tonnes"], Mass, 1000.0, "lb"),
    u(&["oz", "ounce", "ounces"], Mass, 0.028349523125, "g"),
    u(&["lb", "lbs", "pound", "pounds"], Mass, 0.45359237, "kg"),
    u(&["st", "stone", "stones"], Mass, 6.35029318, "kg"),
    u(&["ml", "milliliter", "milliliters", "millilitre", "millilitres"], Volume, 0.001, "floz"),
    u(&["cl", "centiliter", "centiliters"], Volume, 0.01, "floz"),
    u(&["dl", "deciliter", "deciliters"], Volume, 0.1, "cup"),
    u(&["l", "liter", "liters", "litre", "litres"], Volume, 1.0, "gal"),
    u(&["m3", "m³", "cubic meter", "cubic meters"], Volume, 1000.0, "gal"),
    u(&["tsp", "teaspoon", "teaspoons"], Volume, 0.00492892159375, "ml"),
    u(&["tbsp", "tablespoon", "tablespoons"], Volume, 0.01478676478125, "ml"),
    u(&["floz", "fl oz", "fluid ounce", "fluid ounces"], Volume, 0.0295735295625, "ml"),
    u(&["cup", "cups"], Volume, 0.2365882365, "ml"),
    u(&["pt", "pint", "pints"], Volume, 0.473176473, "l"),
    u(&["qt", "quart", "quarts"], Volume, 0.946352946, "l"),
    u(&["gal", "gallon", "gallons"], Volume, 3.785411784, "l"),
    u(&["cm2", "cm²"], Area, 1e-4, "in2"),
    u(&["m2", "m²", "sqm", "square meter", "square meters"], Area, 1.0, "ft2"),
    u(&["km2", "km²", "square kilometer", "square kilometers"], Area, 1e6, "mi2"),
    u(&["ha", "hectare", "hectares"], Area, 1e4, "acre"),
    u(&["in2", "in²", "square inch", "square inches"], Area, 0.00064516, "cm2"),
    u(&["ft2", "ft²", "sqft", "square foot", "square feet"], Area, 0.09290304, "m2"),
    u(&["acre", "acres"], Area, 4046.8564224, "ha"),
    u(&["mi2", "mi²", "square mile", "square miles"], Area, 2589988.110336, "km2"),
    u(&["m/s", "mps", "meters per second"], Speed, 1.0, "km/h"),
    u(&["km/h", "kmh", "kph", "kilometers per hour"], Speed, 1.0 / 3.6, "mph"),
    u(&["mph", "miles per hour"], Speed, 0.44704, "km/h"),
    u(&["kn", "kt", "knot", "knots"], Speed, 0.514444444, "km/h"),
    u(&["ms", "millisecond", "milliseconds"], Time, 0.001, "s"),
    u(&["s", "sec", "secs", "second", "seconds"], Time, 1.0, "min"),
    u(&["min", "mins", "minute", "minutes"], Time, 60.0, "h"),
    u(&["h", "hr", "hrs", "hour", "hours"], Time, 3600.0, "min"),
    u(&["day", "days", "d"], Time, 86400.0, "h"),
    u(&["week", "weeks", "wk"], Time, 604800.0, "day"),
    u(&["year", "years", "yr"], Time, 31557600.0, "day"),
    u(&["bit", "bits"], Data, 0.125, "B"),
    u(&["B", "byte", "bytes"], Data, 1.0, "bit"),
    u(&["KB", "kb", "kilobyte", "kilobytes"], Data, 1e3, "KiB"),
    u(&["MB", "mb", "megabyte", "megabytes"], Data, 1e6, "MiB"),
    u(&["GB", "gb", "gigabyte", "gigabytes"], Data, 1e9, "GiB"),
    u(&["TB", "tb", "terabyte", "terabytes"], Data, 1e12, "TiB"),
    u(&["KiB", "kib", "kibibyte"], Data, 1024.0, "KB"),
    u(&["MiB", "mib", "mebibyte"], Data, 1048576.0, "MB"),
    u(&["GiB", "gib", "gibibyte"], Data, 1073741824.0, "GB"),
    u(&["TiB", "tib", "tebibyte"], Data, 1099511627776.0, "TB"),
    u(&["J", "j", "joule", "joules"], Energy, 1.0, "cal"),
    u(&["kJ", "kj", "kilojoule", "kilojoules"], Energy, 1000.0, "kcal"),
    u(&["cal", "calorie", "calories"], Energy, 4.184, "J"),
    u(&["kcal", "kilocalorie", "kilocalories"], Energy, 4184.0, "kJ"),
    u(&["Wh", "wh", "watt hour", "watt hours"], Energy, 3600.0, "kJ"),
    u(&["kWh", "kwh", "kilowatt hour", "kilowatt hours"], Energy, 3.6e6, "MJ"),
    u(&["MJ", "mj", "megajoule", "megajoules"], Energy, 1e6, "kWh"),
    u(&["°C", "c", "°c", "celsius", "degc"], Temperature, 0.0, "°F"),
    u(&["°F", "f", "°f", "fahrenheit", "degf"], Temperature, 0.0, "°C"),
    u(&["K", "k", "kelvin"], Temperature, 0.0, "°C"),
];

fn unit(name: &str) -> Option<&'static Unit> {
    let name = name.trim().trim_end_matches('.');
    // exact case first (MB vs mb are the same here, but B vs b would not be)
    UNITS
        .iter()
        .find(|u| u.names.contains(&name))
        .or_else(|| UNITS.iter().find(|u| u.names.iter().any(|n| n.eq_ignore_ascii_case(name))))
}

fn to_celsius(name: &str, v: f64) -> f64 {
    match name {
        "°F" => (v - 32.0) * 5.0 / 9.0,
        "K" => v - 273.15,
        _ => v,
    }
}

fn from_celsius(name: &str, c: f64) -> f64 {
    match name {
        "°F" => c * 9.0 / 5.0 + 32.0,
        "K" => c + 273.15,
        _ => c,
    }
}

fn convert_units(v: f64, from: &Unit, to: &Unit) -> Option<f64> {
    if from.dim != to.dim {
        return None;
    }
    Some(if from.dim == Temperature {
        from_celsius(to.names[0], to_celsius(from.names[0], v))
    } else {
        v * from.factor / to.factor
    })
}

// ── currency ────────────────────────────────────────────

/// Words and symbols people type for money. "pound" stays a weight; use £ or gbp.
const CURRENCY_WORDS: &[(&str, &str)] = &[
    ("$", "USD"), ("dollar", "USD"), ("dollars", "USD"), ("bucks", "USD"),
    ("€", "EUR"), ("euro", "EUR"), ("euros", "EUR"),
    ("£", "GBP"), ("quid", "GBP"),
    ("¥", "JPY"), ("yen", "JPY"),
    ("franc", "CHF"), ("francs", "CHF"), ("fr", "CHF"), ("chf", "CHF"),
    ("dinar", "TND"), ("dinars", "TND"),
    ("rupee", "INR"), ("rupees", "INR"), ("yuan", "CNY"), ("won", "KRW"),
    ("bitcoin", "BTC"),
];

const KNOWN: &[&str] = &[
    "USD", "EUR", "CHF", "GBP", "JPY", "CNY", "CAD", "AUD", "NZD", "SEK", "NOK", "DKK", "PLN",
    "CZK", "HUF", "TRY", "RUB", "UAH", "INR", "KRW", "SGD", "HKD", "BRL", "MXN", "ARS", "ZAR",
    "TND", "MAD", "EGP", "AED", "SAR", "ILS", "THB", "IDR", "PHP", "VND", "MYR", "TWD",
];

fn currency(text: &str, rates: Option<&Rates>) -> Option<String> {
    let t = text.trim().to_lowercase();
    if let Some((_, code)) = CURRENCY_WORDS.iter().find(|(w, _)| *w == t) {
        return Some((*code).to_owned());
    }
    let code = t.to_uppercase();
    let known = KNOWN.contains(&code.as_str()) || rates.is_some_and(|r| r.rates.get(&code).is_some());
    (code.len() == 3 && code.chars().all(|c| c.is_ascii_alphabetic()) && known).then_some(code)
}

/// Your currency: the setting, else from the time zone, else from the locale.
pub fn home_currency() -> String {
    let set = crate::config::get().currency.trim().to_uppercase();
    if !set.is_empty() {
        return set;
    }
    let tz = fs::read_link("/etc/localtime").map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let by_zone: &[(&str, &str)] = &[
        ("Europe/Zurich", "CHF"), ("Europe/London", "GBP"), ("Europe/Stockholm", "SEK"),
        ("Europe/Oslo", "NOK"), ("Europe/Copenhagen", "DKK"), ("Europe/Warsaw", "PLN"),
        ("Europe/Prague", "CZK"), ("Europe/Budapest", "HUF"), ("Europe/Istanbul", "TRY"),
        ("Europe/Kyiv", "UAH"), ("Asia/Tokyo", "JPY"), ("Asia/Shanghai", "CNY"), ("Asia/Seoul", "KRW"),
        ("Asia/Kolkata", "INR"), ("Asia/Dubai", "AED"), ("Asia/Singapore", "SGD"), ("Africa/Tunis", "TND"),
        ("America/Toronto", "CAD"), ("America/Vancouver", "CAD"), ("America/Mexico_City", "MXN"),
        ("America/Sao_Paulo", "BRL"), ("Australia/", "AUD"), ("Pacific/Auckland", "NZD"),
        ("America/", "USD"), ("Europe/", "EUR"),
    ];
    if let Some((_, code)) = by_zone.iter().find(|(z, _)| tz.contains(z)) {
        return (*code).to_owned();
    }
    let lang = std::env::var("LC_MONETARY").ok().filter(|s| !s.is_empty()).or_else(|| std::env::var("LANG").ok());
    match lang.as_deref().and_then(|l| l.split(['_', '.']).nth(1)) {
        Some("CH") => "CHF",
        Some("GB") => "GBP",
        Some("US") => "USD",
        Some("JP") => "JPY",
        Some("CA") => "CAD",
        Some("AU") => "AUD",
        Some("TN") => "TND",
        _ => "EUR",
    }
    .into()
}

#[derive(serde::Deserialize, serde::Serialize)]
struct Rates {
    /// Units of each currency per 1 USD.
    rates: std::collections::HashMap<String, f64>,
    time_last_update_unix: i64,
}

fn rates_path() -> PathBuf {
    dirs::cache_dir().unwrap_or_default().join("zunder/rates.json")
}

fn cached_rates() -> Option<(Rates, bool)> {
    let path = rates_path();
    let rates: Rates = serde_json::from_str(&fs::read_to_string(&path).ok()?).ok()?;
    let age = fs::metadata(&path).ok()?.modified().ok()?.elapsed().unwrap_or(Duration::MAX);
    Some((rates, age < Duration::from_secs(12 * 3600)))
}

fn fetch_rates() -> Option<Rates> {
    let out = std::process::Command::new("curl")
        .args(["-fsSL", "-m", "8", "https://open.er-api.com/v6/latest/USD"])
        .output()
        .ok()?;
    let rates: Rates = serde_json::from_slice(&out.stdout).ok()?;
    let path = rates_path();
    let _ = fs::create_dir_all(path.parent()?);
    let _ = fs::write(&path, serde_json::to_string(&rates).ok()?);
    Some(rates)
}

fn convert_money(v: f64, from: &str, to: &str, rates: &Rates) -> Option<f64> {
    let rate = |c: &str| if c == "USD" { Some(1.0) } else { rates.rates.get(c).copied() };
    Some(v / rate(from)? * rate(to)?)
}

// ── parsing ─────────────────────────────────────────────

enum From {
    Unit(&'static Unit),
    Money(String),
}

struct Ask {
    value: f64,
    from: From,
    /// The target as typed, `None` for the defaults.
    to: Option<String>,
    shown: String,
}

fn parse(query: &str, rates: Option<&Rates>) -> Option<Ask> {
    static LEFT: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(?P<sym>[$€£¥])?\s*(?P<num>-?\d+(?:[.,]\d+)?)\s*(?P<unit>[^\d\s].*?)?$").unwrap()
    });
    let q = query.trim();
    let (left, to) = [" in ", " to ", " as ", " = "]
        .iter()
        .filter_map(|sep| q.rfind(sep).map(|i| (&q[..i], Some(q[i + sep.len()..].trim().to_owned()))))
        .max_by_key(|(l, _)| l.len())
        .unwrap_or((q, None));
    let cap = LEFT.captures(left.trim())?;
    let value: f64 = cap["num"].replace(',', ".").parse().ok()?;
    let unit_text = cap.name("sym").or(cap.name("unit")).map(|m| m.as_str())?;
    let from = match currency(unit_text, rates) {
        Some(code) => From::Money(code),
        None => From::Unit(unit(unit_text)?),
    };
    Some(Ask { value, from, to: to.filter(|t| !t.is_empty()), shown: left.trim().to_owned() })
}

fn number(v: f64, decimals: usize) -> String {
    let s = format!("{v:.decimals$}");
    let s = if s.contains('.') { s.trim_end_matches('0').trim_end_matches('.').to_owned() } else { s };
    if s == "-0" { "0".into() } else { s }
}

/// Up to 6 significant digits, no trailing zeros.
fn pretty(v: f64) -> String {
    let int_digits = if v.abs() >= 1.0 { v.abs().log10().floor() as usize + 1 } else { 1 };
    number(v, 6usize.saturating_sub(int_digits).max(if v.abs() < 1.0 { 6 } else { 0 }))
}

fn item(title: String, subtitle: String, copy: String) -> Item {
    Item {
        title,
        subtitle,
        hint: "copy".into(),
        glyph: "\u{f0ec}".into(),
        action: Action::Copy(copy),
        score: 10_000,
        ..Default::default()
    }
}

fn money_items(ask: &Ask, code: &str, rates: &Rates) -> Vec<Item> {
    let targets: Vec<String> = match &ask.to {
        Some(t) => currency(t, Some(rates)).into_iter().collect(),
        // your currency, plus dollars (or euros, for dollars)
        None => {
            let mut v = vec![home_currency(), if code == "USD" { "EUR" } else { "USD" }.to_owned()];
            v.retain(|c| c != code);
            v.dedup();
            v
        }
    };
    let day = chrono::DateTime::from_timestamp(rates.time_last_update_unix, 0)
        .map(|d| d.format("%d %b").to_string())
        .unwrap_or_default();
    targets
        .into_iter()
        .filter_map(|to| {
            let v = convert_money(ask.value, code, &to, rates)?;
            let shown = number(v, 2);
            Some(item(format!("= {shown} {to}"), format!("{} {code} · rate of {day}", number(ask.value, 2)), shown))
        })
        .collect()
}

impl Provider for Convert {
    fn title(&self) -> &str {
        "Convert"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        let cached = cached_rates();
        let rates = cached.as_ref().map(|(r, _)| r);
        let Some(ask) = parse(cx.query, rates) else { return Vec::new() };
        match &ask.from {
            From::Unit(from) => {
                let to_name = ask.to.as_deref().unwrap_or(from.default_to);
                let Some(to) = unit(to_name) else { return Vec::new() };
                let Some(v) = convert_units(ask.value, from, to) else { return Vec::new() };
                let shown = pretty(v);
                vec![item(format!("= {shown} {}", to.names[0]), ask.shown, shown)]
            }
            From::Money(code) => {
                let Some((rates, fresh)) = cached else { return Vec::new() }; // first time: `deferred`
                if !fresh {
                    std::thread::spawn(fetch_rates);
                }
                money_items(&ask, code, &rates)
            }
        }
    }

    fn deferred(&self, cx: &Ctx) -> Option<(String, Job)> {
        if cached_rates().is_some() {
            return None;
        }
        let ask = parse(cx.query, None)?;
        let From::Money(code) = &ask.from else { return None };
        let code = code.clone();
        Some((
            "Convert".into(),
            Box::new(move || match fetch_rates() {
                Some(rates) => money_items(&ask, &code, &rates),
                None => vec![Item {
                    title: "couldn't load exchange rates".into(),
                    subtitle: "needs internet once, then they're cached for 12 hours".into(),
                    glyph: "\u{f071}".into(),
                    ..Default::default()
                }],
            }),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(q: &str) -> Option<String> {
        let ask = parse(q, None)?;
        let From::Unit(from) = ask.from else { return None };
        let to = unit(ask.to.as_deref().unwrap_or(from.default_to))?;
        Some(format!("{} {}", pretty(convert_units(ask.value, from, to)?), to.names[0]))
    }

    #[test]
    fn converts_units() {
        assert_eq!(conv("5 km in mi").as_deref(), Some("3.10686 mi"));
        assert_eq!(conv("10 km").as_deref(), Some("6.21371 mi"));
        assert_eq!(conv("30c").as_deref(), Some("86 °F"));
        assert_eq!(conv("100 f to c").as_deref(), Some("37.7778 °C"));
        assert_eq!(conv("2 gb in mib").as_deref(), Some("1907.35 MiB"));
        assert_eq!(conv("1,5 l to ml").as_deref(), Some("1500 ml"));
        assert_eq!(conv("5 in in cm").as_deref(), Some("12.7 cm"));
        assert_eq!(conv("90 min in h").as_deref(), Some("1.5 h"));
        assert_eq!(conv("5 kg in km"), None);
        assert!(parse("firefox", None).is_none());
        assert!(parse("2+2", None).is_none());
    }

    #[test]
    fn reads_money() {
        let Some(Ask { from: From::Money(c), to, value, .. }) = parse("100 usd in chf", None) else { panic!() };
        assert_eq!((c.as_str(), to.as_deref(), value), ("USD", Some("chf"), 100.0));
        let Some(Ask { from: From::Money(c), .. }) = parse("€20", None) else { panic!() };
        assert_eq!(c, "EUR");
        assert!(matches!(parse("5 pounds", None).map(|a| a.from), Some(From::Unit(_))));
    }
}
