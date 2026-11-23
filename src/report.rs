use std::fmt::Write as _;

use crate::time::{format_date, format_timestamp};
use crate::types::{BacktestResult, EquityPoint};

/// Renders a self-contained HTML report.
///
/// The output contains no wall-clock time, host name, path or locale-dependent
/// number formatting. The same result therefore produces the same bytes, and
/// the bytes are hashed by CI.
pub fn render(
    result: &BacktestResult,
    strategy_name: &str,
    title: &str,
    lookahead_verdict: &str,
) -> String {
    let mut out = String::with_capacity(32 * 1024);
    let metrics = &result.metrics;
    let symbols = result.symbols.join(", ");
    let start = result
        .data_start_ts
        .map(format_date)
        .unwrap_or_else(|| "n/a".into());
    let end = result
        .data_end_ts
        .map(format_date)
        .unwrap_or_else(|| "n/a".into());
    let verdict_class = if lookahead_verdict.contains("PASSED") {
        "pass"
    } else {
        "fail"
    };

    out.push_str("<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n");
    out.push_str("<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(out, "<title>{}</title>", esc(title));
    out.push_str(
        "<style>\n\
         :root{--bg:#f7f7f5;--ink:#17181a;--muted:#64676d;--line:#dcdcd6;--card:#ffffff;--accent:#1d4ed8;--pass:#0f7a3d;--fail:#b42318;}\n\
         *{box-sizing:border-box}body{margin:0;background:var(--bg);color:var(--ink);font:15px/1.55 -apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,Helvetica,Arial,sans-serif}\n\
         main{max-width:1000px;margin:0 auto;padding:48px 24px 64px}\n\
         .eyebrow{letter-spacing:.14em;text-transform:uppercase;font-size:11px;color:var(--muted);margin:0 0 6px}\n\
         h1{font-size:30px;line-height:1.15;margin:0 0 8px;font-weight:650}h2{font-size:18px;margin:0 0 14px}\n\
         .sub{color:var(--muted);margin:0 0 26px}\n\
         section{background:var(--card);border:1px solid var(--line);border-radius:10px;padding:22px 24px;margin:18px 0}\n\
         .verdict{border-left:4px solid;padding:14px 18px;margin:18px 0}.verdict.pass{border-color:var(--pass);background:#eff8f2}.verdict.fail{border-color:var(--fail);background:#fdf1f0}\n\
         .verdict strong{display:block;font-size:12px;letter-spacing:.1em;text-transform:uppercase;margin-bottom:4px}.verdict.pass strong{color:var(--pass)}.verdict.fail strong{color:var(--fail)}\n\
         .cards{display:grid;grid-template-columns:repeat(3,1fr);gap:12px}@media(max-width:720px){.cards{grid-template-columns:1fr 1fr}}\n\
         .card{border:1px solid var(--line);border-radius:8px;padding:14px 16px;background:#fbfbfa}.card .label{font-size:12px;color:var(--muted);text-transform:uppercase;letter-spacing:.06em}.card .value{font-size:24px;font-variant-numeric:tabular-nums;margin-top:2px}.card .note{font-size:12px;color:var(--muted)}\n\
         table{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums}th,td{text-align:left;padding:8px 10px;border-bottom:1px solid var(--line);vertical-align:top}th{font-size:11px;text-transform:uppercase;letter-spacing:.07em;color:var(--muted);font-weight:600}td.num,th.num{text-align:right}tr:last-child td{border-bottom:0}\n\
         code{font:13px/1.4 ui-monospace,SFMono-Regular,Consolas,monospace;background:#f1f1ee;padding:2px 5px;border-radius:4px;word-break:break-all}\n\
         .muted{color:var(--muted)}.scroll{overflow-x:auto}svg{display:block;width:100%;height:auto}\n\
         footer{color:var(--muted);font-size:12px;margin-top:28px;border-top:1px solid var(--line);padding-top:14px}\n\
         </style>\n</head>\n<body>\n<main>\n",
    );

    out.push_str(
        "<p class=\"eyebrow\">backtest-engine &middot; event-driven &middot; point-in-time</p>\n",
    );
    let _ = writeln!(out, "<h1>{}</h1>", esc(title));
    let _ = writeln!(
        out,
        "<p class=\"sub\">{} &middot; {} &middot; {} to {}</p>",
        esc(strategy_name),
        esc(&symbols),
        start,
        end
    );

    let _ = writeln!(
        out,
        "<div class=\"verdict {verdict_class}\"><strong>Look-ahead check</strong>{}</div>",
        esc(lookahead_verdict)
    );

    out.push_str("<section><h2>Performance</h2><div class=\"cards\">\n");
    card(&mut out, "Total return", &pct(metrics.total_return), "");
    card(
        &mut out,
        "Annualized volatility",
        &pct(metrics.annualized_volatility),
        "",
    );
    card(
        &mut out,
        "Sharpe ratio",
        &format!("{:.4}", metrics.sharpe),
        "risk-free = 0",
    );
    card(
        &mut out,
        "Max drawdown",
        &pct(metrics.max_drawdown),
        "peak to trough",
    );
    card(
        &mut out,
        "Turnover",
        &format!("{:.4}x", metrics.turnover),
        "traded notional / mean equity",
    );
    card(
        &mut out,
        "Total cost",
        &format!("{:.2}", metrics.total_cost),
        &format!("{:.2} bps of initial capital", metrics.cost_bps),
    );
    out.push_str("</div></section>\n");

    out.push_str("<section><h2>Equity curve</h2>\n");
    out.push_str(&equity_svg(&result.equity));
    out.push_str("</section>\n");

    out.push_str("<section><h2>Costs</h2><div class=\"scroll\"><table>\n");
    out.push_str("<thead><tr><th>Component</th><th class=\"num\">Amount</th><th class=\"num\">bps of initial capital</th></tr></thead><tbody>\n");
    let initial = metrics.initial_cash;
    let bps = |value: f64| {
        if initial > 0.0 {
            value / initial * 10_000.0
        } else {
            0.0
        }
    };
    cost_row(
        &mut out,
        "Commission",
        metrics.commission_cost,
        bps(metrics.commission_cost),
    );
    cost_row(
        &mut out,
        "Spread",
        metrics.spread_cost,
        bps(metrics.spread_cost),
    );
    cost_row(
        &mut out,
        "Slippage",
        metrics.slippage_cost,
        bps(metrics.slippage_cost),
    );
    cost_row(&mut out, "Total", metrics.total_cost, metrics.cost_bps);
    out.push_str("</tbody></table></div>\n");
    out.push_str("<p class=\"muted\">Spread and slippage are already embedded in the fill price. The table reports their cash value, not an extra deduction.</p></section>\n");

    out.push_str("<section><h2>Configuration</h2><div class=\"scroll\"><table><tbody>\n");
    kv(
        &mut out,
        "Initial cash",
        &format!("{:.2}", result.config.initial_cash),
    );
    kv(
        &mut out,
        "Commission",
        &format!(
            "{:.4} bps + {:.2} fixed",
            result.config.commission_bps, result.config.commission_fixed
        ),
    );
    kv(
        &mut out,
        "Spread",
        &format!("{:.4} bps (half per side)", result.config.spread_bps),
    );
    kv(
        &mut out,
        "Slippage",
        &format!(
            "{:.4} bps at 100% participation",
            result.config.slippage_bps_at_full_volume
        ),
    );
    kv(
        &mut out,
        "Periods per year",
        &result.config.periods_per_year.to_string(),
    );
    kv(
        &mut out,
        "Future access policy",
        if result.config.strict_future_access {
            "rejected (strict)"
        } else {
            "audited"
        },
    );
    kv(
        &mut out,
        "Simulated window",
        &format!(
            "{} to {}",
            result
                .config
                .start_ts
                .map(format_date)
                .unwrap_or_else(|| "first bar".into()),
            result
                .config
                .end_ts
                .map(|ts| format_date(ts - 1))
                .unwrap_or_else(|| "last bar".into())
        ),
    );
    kv(
        &mut out,
        "Bars processed",
        &result.bars_processed.to_string(),
    );
    kv(&mut out, "Fills", &result.trades.len().to_string());
    out.push_str("</tbody></table></div></section>\n");

    out.push_str("<section><h2>Point-in-time data</h2><div class=\"scroll\"><table><tbody>\n");
    kv(&mut out, "Symbols", &symbols);
    kv(&mut out, "Dataset range", &format!("{start} to {end}"));
    kv(
        &mut out,
        "Bars / rows / revisions",
        &format!(
            "{} / {} / {}",
            result.bars_processed, result.data_rows, result.data_revisions
        ),
    );
    let _ = writeln!(
        out,
        "<tr><th>Data SHA-256</th><td><code>{}</code></td></tr>",
        esc(&result.data_hash)
    );
    out.push_str("</tbody></table></div>\n<p class=\"muted\">A read as of T only sees rows with published_at &lt;= T. Revisions stay hidden until their publication timestamp.</p></section>\n");

    out.push_str("<section><h2>Trades</h2><div class=\"scroll\"><table>\n");
    out.push_str("<thead><tr><th>Date</th><th>Side</th><th class=\"num\">Quantity</th><th class=\"num\">Reference</th><th class=\"num\">Fill</th><th class=\"num\">Notional</th><th class=\"num\">Commission</th><th class=\"num\">Spread</th><th class=\"num\">Slippage</th><th class=\"num\">Cost</th></tr></thead><tbody>\n");
    for trade in &result.trades {
        let _ = writeln!(
            out,
            "<tr><td>{}</td><td>{}</td><td class=\"num\">{:.4}</td><td class=\"num\">{:.4}</td><td class=\"num\">{:.4}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.2}</td></tr>",
            format_date(trade.ts),
            esc(&trade.side),
            trade.quantity,
            trade.reference_price,
            trade.price,
            trade.notional,
            trade.commission,
            trade.spread_cost,
            trade.slippage_cost,
            trade.commission + trade.spread_cost + trade.slippage_cost
        );
    }
    if result.trades.is_empty() {
        out.push_str("<tr><td colspan=\"10\" class=\"muted\">No fills in this window.</td></tr>\n");
    }
    out.push_str("</tbody></table></div></section>\n");

    let _ = writeln!(
        out,
        "<footer>Generated by backtest-engine {} &middot; MIT license &middot; Iker Abad &middot; deterministic report, no timestamps or host details. Report hash is verified in CI.</footer>",
        env!("CARGO_PKG_VERSION")
    );
    out.push_str("</main>\n</body>\n</html>\n");
    out
}

fn card(out: &mut String, label: &str, value: &str, note: &str) {
    let _ = writeln!(
        out,
        "<div class=\"card\"><div class=\"label\">{}</div><div class=\"value\">{}</div><div class=\"note\">{}</div></div>",
        esc(label),
        esc(value),
        esc(note)
    );
}

fn cost_row(out: &mut String, label: &str, amount: f64, bps: f64) {
    let _ = writeln!(
        out,
        "<tr><td>{}</td><td class=\"num\">{:.2}</td><td class=\"num\">{:.4}</td></tr>",
        esc(label),
        amount,
        bps
    );
}

fn kv(out: &mut String, key: &str, value: &str) {
    let _ = writeln!(out, "<tr><th>{}</th><td>{}</td></tr>", esc(key), value);
}

fn pct(value: f64) -> String {
    format!("{:.4}%", value * 100.0)
}

fn esc(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    for ch in input.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(ch),
        }
    }
    out
}

fn equity_svg(points: &[EquityPoint]) -> String {
    if points.is_empty() {
        return "<p class=\"muted\">No equity observations in this window.</p>\n".into();
    }
    let width = 920.0;
    let height = 320.0;
    let left = 72.0;
    let right = 18.0;
    let top = 18.0;
    let bottom = 38.0;
    let plot_width = width - left - right;
    let plot_height = height - top - bottom;
    let min = points
        .iter()
        .map(|point| point.equity)
        .fold(f64::INFINITY, f64::min);
    let max = points
        .iter()
        .map(|point| point.equity)
        .fold(f64::NEG_INFINITY, f64::max);
    let range = if max > min { max - min } else { 1.0 };
    let x = |index: usize| {
        if points.len() <= 1 {
            left + plot_width / 2.0
        } else {
            left + index as f64 / (points.len() - 1) as f64 * plot_width
        }
    };
    let y = |value: f64| top + (max - value) / range * plot_height;

    let mut svg = String::new();
    let _ = writeln!(
        svg,
        "<svg viewBox=\"0 0 {width:.0} {height:.0}\" role=\"img\" aria-label=\"Equity curve\">"
    );
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#ffffff\"/>\n");
    for step in 0..=2 {
        let value = min + range * step as f64 / 2.0;
        let py = y(value);
        let _ = writeln!(
            svg,
            "<line x1=\"{left:.2}\" y1=\"{py:.2}\" x2=\"{:.2}\" y2=\"{py:.2}\" stroke=\"#e6e6e1\" stroke-width=\"1\"/>",
            width - right
        );
        let _ = writeln!(
            svg,
            "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"#64676d\" font-size=\"11\" text-anchor=\"end\">{:.0}</text>",
            left - 10.0,
            py + 4.0,
            value
        );
    }
    let mut path = String::new();
    for (index, point) in points.iter().enumerate() {
        if index > 0 {
            path.push(' ');
        }
        let _ = write!(path, "{:.2},{:.2}", x(index), y(point.equity));
    }
    let _ = writeln!(
        svg,
        "<polyline fill=\"none\" stroke=\"#1d4ed8\" stroke-width=\"2\" stroke-linejoin=\"round\" stroke-linecap=\"round\" points=\"{path}\"/>"
    );
    let _ = writeln!(
        svg,
        "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"#64676d\" font-size=\"11\" text-anchor=\"start\">{}</text>",
        left,
        height - 12.0,
        points
            .first()
            .map(|point| format_date(point.ts))
            .unwrap_or_default()
    );
    let _ = writeln!(
        svg,
        "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"#64676d\" font-size=\"11\" text-anchor=\"end\">{}</text>",
        width - right,
        height - 12.0,
        points
            .last()
            .map(|point| format_date(point.ts))
            .unwrap_or_default()
    );
    let _ = writeln!(
        svg,
        "<text x=\"{:.2}\" y=\"{:.2}\" fill=\"#64676d\" font-size=\"10\" text-anchor=\"end\">as of {}</text>",
        width - right,
        top + 12.0,
        points
            .last()
            .map(|point| format_timestamp(point.ts))
            .unwrap_or_default()
    );
    svg.push_str("</svg>\n");
    svg
}
