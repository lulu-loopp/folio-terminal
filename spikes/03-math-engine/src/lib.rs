use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Instant;

use anyhow::{Context as _, Result, anyhow, bail};
use mitex_spec_gen::DEFAULT_SPEC;
use regex::Regex;
use rquickjs::{Context, Runtime};
use serde::{Deserialize, Serialize};
use typst_as_lib::{TypstEngine, typst_kit_options::TypstKitFontOptions};
use typst_layout::PagedDocument;
use typst_library::foundations::{Dict, IntoValue, Str, Value};
use typst_library::layout::{Frame, FrameItem};

const TYPST_TEMPLATE: &str = r#"
#import "specs/mod.typ": mitex-scope
#set page(width: sys.inputs.width * 1pt, height: auto, margin: 2pt)
#set text(size: 12pt)
#let converted = eval("$" + sys.inputs.source + "$", scope: mitex-scope)
#math.equation(block: true, converted)
"#;

const KATEX_JS: &str = concat!(
    "var module = undefined; var exports = undefined; var process = undefined;\n",
    include_str!("../assets/katex.min.js")
);

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MathSample {
    pub id: String,
    pub category: String,
    pub latex: String,
    pub expected_valid: bool,
    pub adversarial: bool,
}

pub fn corpus() -> Vec<MathSample> {
    let mut samples = Vec::with_capacity(360);
    for i in 0..40 {
        push(
            &mut samples,
            "fraction",
            format!(r"\frac{{x_{} + {}}}{{y_{} - {}}}", i, i + 1, i + 2, i + 3),
            true,
            false,
        );
    }
    for i in 0..40 {
        push(
            &mut samples,
            "root",
            format!(r"\sqrt[{}]{{x^{} + y^{}}}", i % 5 + 2, i % 7 + 2, i % 9 + 2),
            true,
            false,
        );
    }
    for i in 0..30 {
        push(
            &mut samples,
            "left-right",
            format!(
                r"\left( \frac{{x_{}}}{{y_{}}} + \sqrt{{z_{}}} \right)",
                i,
                i + 1,
                i + 2
            ),
            true,
            false,
        );
    }
    for i in 0..40 {
        push(
            &mut samples,
            "large-operator",
            format!(
                r"\sum_{{k=0}}^{{{}}} \frac{{k^2}}{{{} + k}} + \int_0^1 x^{}\,dx",
                i + 3,
                i + 1,
                i % 8 + 1
            ),
            true,
            false,
        );
    }
    for i in 0..40 {
        push(
            &mut samples,
            "matrix",
            format!(
                r"\begin{{pmatrix}} {} & x_{} \\ y_{} & {} \end{{pmatrix}}",
                i,
                i + 1,
                i + 2,
                i + 3
            ),
            true,
            false,
        );
    }
    for i in 0..40 {
        push(
            &mut samples,
            "ams-align",
            format!(
                r"\begin{{aligned}} f_{}(x) &= x^{} + {} \\ f_{}'(x) &= {}x^{} \end{{aligned}}",
                i,
                i % 6 + 2,
                i,
                i,
                i % 6 + 2,
                i % 6 + 1
            ),
            true,
            false,
        );
    }
    for i in 0..30 {
        push(
            &mut samples,
            "unicode-math",
            format!("∀x∈ℝ: α_{i} ≤ β_{} ∧ ∑ᵢ xᵢ = ∞", i + 1),
            true,
            false,
        );
    }
    for i in 0..30 {
        push(
            &mut samples,
            "custom-macro",
            format!(
                r"\newcommand{{\vect}}[1]{{\mathbf{{#1}}}} \vect{{v_{}}} \cdot \vect{{w_{}}}",
                i,
                i + 1
            ),
            true,
            false,
        );
    }
    for i in 0..30 {
        let latex = match i % 5 {
            0 => r"\frac{1}{2".to_owned(),
            1 => r"\begin{matrix}1&2".to_owned(),
            2 => r"x^{^{^{".to_owned(),
            3 => r"\left( x + y".to_owned(),
            _ => format!(r"\unknowncommand{{{i}}}"),
        };
        push(&mut samples, "malformed", latex, false, false);
    }
    for i in 0..20 {
        let latex = match i % 5 {
            0 => r"\input{C:\Windows\win.ini}".to_owned(),
            1 => r"\includegraphics{https://example.com/tracker.png}".to_owned(),
            2 => r"\newcommand{\loop}{\loop}\loop".to_owned(),
            3 => format!("{}x{}", "{".repeat(300), "}".repeat(300)),
            _ => format!(r"x^{{{}}}", "9".repeat(70_000 + i)),
        };
        push(&mut samples, "malicious", latex, false, true);
    }
    for i in 0..20 {
        let terms = (0..400)
            .map(|j| format!("x_{{{j}}}"))
            .collect::<Vec<_>>()
            .join(" + ");
        push(&mut samples, "huge", format!("{terms} + {i}"), true, false);
    }
    debug_assert_eq!(samples.len(), 360);
    samples
}

fn push(
    samples: &mut Vec<MathSample>,
    category: &str,
    latex: String,
    expected_valid: bool,
    adversarial: bool,
) {
    let id = format!(
        "{category}-{:03}",
        samples
            .iter()
            .filter(|sample| sample.category == category)
            .count()
    );
    samples.push(MathSample {
        id,
        category: category.to_owned(),
        latex,
        expected_valid,
        adversarial,
    });
}

fn validate_input(input: &str) -> Result<()> {
    if input.len() > 64 * 1024 {
        bail!("host input cap: expression exceeds 64 KiB");
    }
    if input.contains("\\input") || input.contains("\\includegraphics") {
        bail!("host sandbox rejects file/network-capable commands");
    }
    let mut depth = 0_u16;
    for byte in input.bytes() {
        match byte {
            b'{' => {
                depth += 1;
                if depth > 256 {
                    bail!("host nesting cap exceeds 256");
                }
            }
            b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if input.contains(r"\newcommand{\loop}{\loop}") {
        bail!("host rejects directly recursive macro");
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize)]
pub struct RenderArtifact {
    pub output: String,
    pub output_kind: String,
    pub width_pt: f64,
    pub height_pt: f64,
    pub ascent_pt: Option<f64>,
    pub descent_pt: Option<f64>,
}

pub struct MitexTypstEngine {
    engine: TypstEngine<typst_as_lib::TypstTemplateMainFile>,
}

impl MitexTypstEngine {
    pub fn new() -> Self {
        let engine = TypstEngine::builder()
            .main_file(TYPST_TEMPLATE)
            .with_static_source_file_resolver([
                ("specs/mod.typ", include_str!("../assets/specs/mod.typ")),
                (
                    "specs/prelude.typ",
                    include_str!("../assets/specs/prelude.typ"),
                ),
                (
                    "specs/latex/standard.typ",
                    include_str!("../assets/specs/latex/standard.typ"),
                ),
            ])
            .search_fonts_with(
                TypstKitFontOptions::default()
                    .include_system_fonts(false)
                    .include_embedded_fonts(true),
            )
            .build();
        Self { engine }
    }

    pub fn render(&self, latex: &str, width_pt: u32) -> Result<RenderArtifact> {
        validate_input(latex)?;
        let converted = mitex::convert_math(latex, Some(DEFAULT_SPEC.clone()))
            .map_err(|error| anyhow!("MiTeX: {error}"))?;
        let mut inputs = Dict::new();
        inputs.insert("source".into(), Value::Str(Str::from(converted)));
        inputs.insert("width".into(), width_pt.into_value());
        let compiled = self.engine.compile_with_input::<_, PagedDocument>(inputs);
        let document = compiled.output.map_err(|error| anyhow!("Typst: {error}"))?;
        let page = document.pages().first().context("Typst returned no page")?;
        let svg = typst_svg::svg(page, &Default::default());
        let (ascent, descent) = find_math_metrics(&page.frame)
            .map(|(ascent, descent)| (Some(ascent), Some(descent)))
            .unwrap_or((None, None));
        Ok(RenderArtifact {
            output: svg,
            output_kind: "standalone-svg".to_owned(),
            width_pt: page.frame.width().to_pt(),
            height_pt: page.frame.height().to_pt(),
            ascent_pt: ascent,
            descent_pt: descent,
        })
    }
}

impl Default for MitexTypstEngine {
    fn default() -> Self {
        Self::new()
    }
}

fn find_math_metrics(frame: &Frame) -> Option<(f64, f64)> {
    let mut best = frame.has_baseline().then(|| {
        (
            frame.height().to_pt(),
            frame.ascent().to_pt(),
            frame.descent().to_pt(),
        )
    });
    for (_, item) in frame.items() {
        if let FrameItem::Group(group) = item
            && let Some((ascent, descent)) = find_math_metrics(&group.frame)
        {
            let height = ascent + descent;
            if best.is_none_or(|current| height > current.0) {
                best = Some((height, ascent, descent));
            }
        }
    }
    best.map(|(_, ascent, descent)| (ascent, descent))
}

pub struct QuickJsKatexEngine {
    _runtime: Runtime,
    context: Context,
}

impl QuickJsKatexEngine {
    pub fn new() -> Result<Self> {
        let runtime = Runtime::new()?;
        runtime.set_memory_limit(128 * 1024 * 1024);
        runtime.set_max_stack_size(2 * 1024 * 1024);
        let context = Context::full(&runtime)?;
        context.with(|ctx| ctx.eval::<(), _>(KATEX_JS))?;
        Ok(Self {
            _runtime: runtime,
            context,
        })
    }

    pub fn render(&self, latex: &str, _width_pt: u32) -> Result<RenderArtifact> {
        validate_input(latex)?;
        let started = Instant::now();
        let html = self.context.with(|ctx| -> rquickjs::Result<String> {
            ctx.globals().set("BT_INPUT", latex)?;
            ctx.eval(
                r#"katex.renderToString(BT_INPUT, {
                    displayMode: true,
                    throwOnError: true,
                    output: "htmlAndMathml",
                    strict: "error",
                    trust: false,
                    maxSize: 200,
                    maxExpand: 1000
                })"#,
            )
        })?;
        if started.elapsed().as_secs() >= 1 {
            bail!("KaTeX in-process watchdog observed >1s; production must discard worker");
        }
        let (ascent_em, descent_em) = katex_metrics(&html).unwrap_or((0.0, 0.0));
        let svg = format!(
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1200" height="240"><foreignObject width="100%" height="100%"><div xmlns="http://www.w3.org/1999/xhtml">{html}</div></foreignObject></svg>"#
        );
        Ok(RenderArtifact {
            output: svg,
            output_kind: "svg-foreignObject-containing-katex-html".to_owned(),
            width_pt: 0.0,
            height_pt: (ascent_em + descent_em) * 12.0,
            ascent_pt: Some(ascent_em * 12.0),
            descent_pt: Some(descent_em * 12.0),
        })
    }
}

fn katex_metrics(html: &str) -> Option<(f64, f64)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let regex = RE.get_or_init(|| {
        Regex::new(r#"class="strut" style="height:([0-9.]+)em;(?:vertical-align:-([0-9.]+)em;)?"#)
            .expect("valid KaTeX strut regex")
    });
    regex
        .captures_iter(html)
        .filter_map(|captures| {
            let total = captures.get(1)?.as_str().parse::<f64>().ok()?;
            let depth = captures
                .get(2)
                .and_then(|value| value.as_str().parse().ok())
                .unwrap_or(0.0);
            Some(((total - depth).max(0.0), depth))
        })
        .max_by(|left, right| (left.0 + left.1).total_cmp(&(right.0 + right.1)))
}

#[derive(Clone, Debug, Serialize)]
pub struct PathSummary {
    pub path: String,
    pub total: usize,
    pub successes: usize,
    pub expected_valid: usize,
    pub expected_valid_successes: usize,
    pub safety_rejections: usize,
    pub h_d_available: usize,
    pub missing_h_d_by_category: BTreeMap<String, usize>,
    pub p50_us: u64,
    pub p95_us: u64,
    pub errors_by_category: BTreeMap<String, usize>,
}

pub fn summarize(
    path: &str,
    samples: &[MathSample],
    observations: &[(u64, Result<RenderArtifact, String>)],
) -> PathSummary {
    let mut durations = observations
        .iter()
        .map(|observation| observation.0)
        .collect::<Vec<_>>();
    durations.sort_unstable();
    let mut summary = PathSummary {
        path: path.to_owned(),
        total: samples.len(),
        successes: 0,
        expected_valid: samples
            .iter()
            .filter(|sample| sample.expected_valid)
            .count(),
        expected_valid_successes: 0,
        safety_rejections: 0,
        h_d_available: 0,
        missing_h_d_by_category: BTreeMap::new(),
        p50_us: percentile(&durations, 50),
        p95_us: percentile(&durations, 95),
        errors_by_category: BTreeMap::new(),
    };
    for (sample, (_, result)) in samples.iter().zip(observations) {
        match result {
            Ok(artifact) => {
                summary.successes += 1;
                summary.expected_valid_successes += usize::from(sample.expected_valid);
                summary.h_d_available +=
                    usize::from(artifact.ascent_pt.is_some() && artifact.descent_pt.is_some());
                if artifact.ascent_pt.is_none() || artifact.descent_pt.is_none() {
                    *summary
                        .missing_h_d_by_category
                        .entry(sample.category.clone())
                        .or_default() += 1;
                }
            }
            Err(error) => {
                *summary
                    .errors_by_category
                    .entry(sample.category.clone())
                    .or_default() += 1;
                summary.safety_rejections += usize::from(error.starts_with("host "));
            }
        }
    }
    summary
}

fn percentile(sorted: &[u64], p: usize) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = (p * sorted.len()).div_ceil(100);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corpus_has_required_size_and_categories() {
        let corpus = corpus();
        assert_eq!(corpus.len(), 360);
        for category in [
            "fraction",
            "root",
            "left-right",
            "large-operator",
            "matrix",
            "ams-align",
            "unicode-math",
            "custom-macro",
            "malformed",
            "malicious",
            "huge",
        ] {
            assert!(
                corpus.iter().any(|sample| sample.category == category),
                "missing {category}"
            );
        }
    }

    #[test]
    fn host_caps_reject_file_network_recursion_depth_and_size() {
        assert!(validate_input(r"\input{x}").is_err());
        assert!(validate_input(r"\includegraphics{https://x}").is_err());
        assert!(validate_input(r"\newcommand{\loop}{\loop}\loop").is_err());
        assert!(validate_input(&format!("{}{}", "{".repeat(257), "}".repeat(257))).is_err());
        assert!(validate_input(&"x".repeat(65 * 1024)).is_err());
    }

    #[test]
    fn katex_strut_yields_ascent_and_descent_not_just_image_height() {
        let html = r#"<span class="strut" style="height:1.2em;vertical-align:-0.3em;"></span>"#;
        let (ascent, descent) = katex_metrics(html).unwrap();
        assert!((ascent - 0.9).abs() < 1e-12);
        assert!((descent - 0.3).abs() < 1e-12);
    }

    #[test]
    fn a_missing_result_is_counted_as_failure() {
        let samples = vec![MathSample {
            id: "x".into(),
            category: "fraction".into(),
            latex: "x".into(),
            expected_valid: true,
            adversarial: false,
        }];
        let observations = vec![(10, Err("broken".to_owned()))];
        let summary = summarize("fault", &samples, &observations);
        assert_eq!(summary.successes, 0);
        assert_eq!(summary.expected_valid_successes, 0);
    }
}
