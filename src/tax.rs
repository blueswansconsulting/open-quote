//! Sales tax for any country: up to two named tax lines (VAT, GST, HST,
//! GST + PST, sales tax…), entered as exclusive (added on top) or inclusive
//! (already in the prices, e.g. Australian GST-inclusive quotes).
//!
//! The trade sets the label and rate; nothing here looks rates up. Rates are
//! held in thousandths of a percent (9.975% = 9975) and all money in integer
//! minor units, rounded half-up.

use serde::Deserialize;

pub const MAX_TAX_LINES: usize = 2;
pub const MAX_RATE_PERCENT: f64 = 30.0;

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaxLineInput {
    pub label: String,
    /// Percent, e.g. 20 or 9.975.
    pub rate: f64,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaxInput {
    pub lines: Vec<TaxLineInput>,
    /// Prices already include the tax (one tax line only).
    pub inclusive: bool,
    /// Shown when no tax is charged, e.g. "No GST charged".
    pub untaxed_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxRate {
    pub label: String,
    /// Thousandths of a percent: 20% = 20_000.
    pub milli: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxSetup {
    pub rates: Vec<TaxRate>,
    pub inclusive: bool,
    pub untaxed_note: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxAmount {
    pub label: String,
    pub milli: i64,
    pub amount: i64,
}

/// "20", "13.5", "9.975".
pub fn rate_text(milli: i64) -> String {
    let whole = milli / 1000;
    let frac = milli % 1000;
    if frac == 0 {
        whole.to_string()
    } else {
        format!("{whole}.{frac:03}")
            .trim_end_matches('0')
            .to_string()
    }
}

fn clean_label(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string()
}

/// Validate the tax settings. `legacy_vat` keeps old clients working: a
/// request with no `tax` object and `vat: true` means UK VAT at 20%.
pub fn setup(input: Option<TaxInput>, legacy_vat: bool) -> Result<TaxSetup, String> {
    let Some(input) = input else {
        return Ok(TaxSetup {
            rates: if legacy_vat {
                vec![TaxRate {
                    label: "VAT".into(),
                    milli: 20_000,
                }]
            } else {
                vec![]
            },
            inclusive: false,
            untaxed_note: "No VAT charged".into(),
        });
    };
    if input.lines.len() > MAX_TAX_LINES {
        return Err(format!("At most {MAX_TAX_LINES} tax lines"));
    }
    let mut rates = Vec::with_capacity(input.lines.len());
    for (i, line) in input.lines.iter().enumerate() {
        let n = i + 1;
        let label = clean_label(&line.label);
        if label.is_empty() {
            return Err(format!("Tax {n}: add a name, like VAT or GST"));
        }
        if label.chars().count() > 20 {
            return Err(format!("Tax {n}: name is too long (max 20 characters)"));
        }
        if !line.rate.is_finite() || !(0.0..=MAX_RATE_PERCENT).contains(&line.rate) {
            return Err(format!(
                "Tax {n}: rate must be between 0% and {MAX_RATE_PERCENT}%"
            ));
        }
        rates.push(TaxRate {
            label,
            milli: (line.rate * 1000.0).round() as i64,
        });
    }
    if input.inclusive && rates.len() != 1 {
        return Err("Prices that include tax need exactly one tax line".into());
    }
    let untaxed_note = clean_label(&input.untaxed_note);
    if untaxed_note.chars().count() > 60 {
        return Err("The no-tax note is too long (max 60 characters)".into());
    }
    Ok(TaxSetup {
        rates,
        inclusive: input.inclusive,
        untaxed_note,
    })
}

/// `value * num / den`, rounded half-up, for non-negative inputs.
fn mul_div_round(value: i64, num: i64, den: i64) -> i64 {
    let (v, n, d) = (value as i128, num as i128, den as i128);
    ((v * n * 2 + d) / (d * 2)) as i64
}

/// Tax amounts and the grand total.
/// - Exclusive: each tax = taxable × rate; total = subtotal + taxes.
/// - Inclusive: taxable amounts already contain the tax; tax = taxable −
///   taxable / (1 + rate); total = subtotal.
pub fn compute(subtotal: i64, taxable: i64, setup: &TaxSetup) -> (Vec<TaxAmount>, i64) {
    if setup.inclusive {
        let Some(rate) = setup.rates.first() else {
            return (vec![], subtotal);
        };
        let net = mul_div_round(taxable, 100_000, 100_000 + rate.milli);
        let amounts = vec![TaxAmount {
            label: rate.label.clone(),
            milli: rate.milli,
            amount: taxable - net,
        }];
        return (amounts, subtotal);
    }
    let amounts: Vec<TaxAmount> = setup
        .rates
        .iter()
        .map(|r| TaxAmount {
            label: r.label.clone(),
            milli: r.milli,
            amount: mul_div_round(taxable, r.milli, 100_000),
        })
        .collect();
    let total = subtotal + amounts.iter().map(|a| a.amount).sum::<i64>();
    (amounts, total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(lines: &[(&str, f64)], inclusive: bool) -> Option<TaxInput> {
        Some(TaxInput {
            lines: lines
                .iter()
                .map(|(l, r)| TaxLineInput {
                    label: (*l).into(),
                    rate: *r,
                })
                .collect(),
            inclusive,
            untaxed_note: String::new(),
        })
    }

    #[test]
    fn legacy_requests_keep_uk_vat() {
        let s = setup(None, true).unwrap();
        assert_eq!(
            s.rates,
            vec![TaxRate {
                label: "VAT".into(),
                milli: 20_000
            }]
        );
        assert!(setup(None, false).unwrap().rates.is_empty());
    }

    #[test]
    fn exclusive_single_and_double_lines() {
        // UK VAT 20% on £720.00.
        let s = setup(input(&[("VAT", 20.0)], false), false).unwrap();
        let (t, total) = compute(72_000, 72_000, &s);
        assert_eq!((t[0].amount, total), (14_400, 86_400));
        // British Columbia: GST 5% + PST 7% on $1,000.00, each on the pre-tax amount.
        let s = setup(input(&[("GST", 5.0), ("PST", 7.0)], false), false).unwrap();
        let (t, total) = compute(100_000, 100_000, &s);
        assert_eq!((t[0].amount, t[1].amount, total), (5_000, 7_000, 112_000));
        // Quebec QST 9.975% on $100.00 = $9.975 -> $9.98 (half-up).
        let s = setup(input(&[("GST", 5.0), ("QST", 9.975)], false), false).unwrap();
        let (t, _) = compute(10_000, 10_000, &s);
        assert_eq!(t[1].amount, 998);
    }

    #[test]
    fn inclusive_gst_is_extracted_from_the_total() {
        // Australia: $1,100 incl. GST 10% contains $100 GST; total stays $1,100.
        let s = setup(input(&[("GST", 10.0)], true), false).unwrap();
        let (t, total) = compute(110_000, 110_000, &s);
        assert_eq!((t[0].amount, total), (10_000, 110_000));
        // NZ: $115 incl. GST 15% contains $15.
        let s = setup(input(&[("GST", 15.0)], true), false).unwrap();
        assert_eq!(compute(11_500, 11_500, &s).0[0].amount, 1_500);
        // Rounding: $10.00 incl. 10% -> net $9.09, GST $0.91.
        let s = setup(input(&[("GST", 10.0)], true), false).unwrap();
        assert_eq!(compute(1_000, 1_000, &s).0[0].amount, 91);
    }

    #[test]
    fn untaxed_lines_are_excluded_from_the_base() {
        // US: $500 materials taxed at 8.875%, $300 labour untaxed.
        let s = setup(input(&[("Sales tax", 8.875)], false), false).unwrap();
        let (t, total) = compute(80_000, 50_000, &s);
        assert_eq!(t[0].amount, 4_438); // 44.375 -> 44.38
        assert_eq!(total, 84_438);
    }

    #[test]
    fn huge_values_do_not_overflow() {
        let s = setup(input(&[("GST", 30.0)], false), false).unwrap();
        let big = 1_000_000_000_000_000; // £10 trillion in pence
        let (t, _) = compute(big, big, &s);
        assert_eq!(t[0].amount, 300_000_000_000_000);
    }

    #[test]
    fn rejects_bad_settings() {
        assert!(setup(input(&[("A", 1.0), ("B", 1.0), ("C", 1.0)], false), false).is_err());
        assert!(setup(input(&[("", 10.0)], false), false)
            .unwrap_err()
            .contains("name"));
        assert!(setup(input(&[("GST", 31.0)], false), false)
            .unwrap_err()
            .contains("rate"));
        assert!(setup(input(&[("GST", -1.0)], false), false).is_err());
        assert!(setup(input(&[("GST", f64::NAN)], false), false).is_err());
        assert!(setup(input(&[("GST", 5.0), ("PST", 7.0)], true), false)
            .unwrap_err()
            .contains("exactly one"));
        assert!(setup(input(&[], true), false).is_err());
    }

    #[test]
    fn rate_text_is_compact() {
        assert_eq!(rate_text(20_000), "20");
        assert_eq!(rate_text(13_500), "13.5");
        assert_eq!(rate_text(9_975), "9.975");
        assert_eq!(rate_text(8_875), "8.875");
        assert_eq!(rate_text(0), "0");
    }
}
