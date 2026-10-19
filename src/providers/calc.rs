//! `2^10 / 3`, `sqrt(2)*pi` … Enter copies the result.

use crate::engine::{Ctx, Provider};
use crate::item::{Action, Item};

pub struct Calc;

impl Provider for Calc {
    fn title(&self) -> &str {
        "Calculator"
    }

    fn search(&self, cx: &Ctx) -> Vec<Item> {
        let q = cx.query.trim().trim_start_matches('=');
        let looks_like_math = q.chars().any(|c| c.is_ascii_digit())
            && q.chars().any(|c| "+-*/^%()".contains(c) || c.is_alphabetic());
        if !looks_like_math || q.parse::<f64>().is_ok() {
            return Vec::new();
        }
        let Ok(value) = meval::eval_str(q) else { return Vec::new() };
        if !value.is_finite() {
            return Vec::new();
        }
        let shown = format_number(value);
        vec![Item {
            id: String::new(),
            title: format!("= {shown}"),
            subtitle: q.to_owned(),
            hint: "copy".into(),
            glyph: "\u{f1ec}".into(),
            action: Action::Copy(shown),
            score: 10_000,
        }]
    }
}

fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let s = format!("{v:.10}");
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::format_number;

    fn eval(q: &str) -> Option<String> {
        meval::eval_str(q).ok().map(format_number)
    }

    #[test]
    fn evaluates() {
        assert_eq!(eval("2^10/4").as_deref(), Some("256"));
        assert_eq!(eval("(4+4)*12%5").as_deref(), Some("1"));
        assert_eq!(eval("sqrt(2)*10").as_deref(), Some("14.1421356237"));
        assert!(eval("floor(pi)+ceil(e)+abs(-1)+ln(1)+exp(0)").is_some());
        assert!(eval("sin(0)+cos(0)+tan(0)").is_some());
    }
}
