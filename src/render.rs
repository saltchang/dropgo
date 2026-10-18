//! Markdown to HTML, at upload time.

use std::fmt;

use comrak::{
    Options,
    adapters::CodefenceRendererAdapter,
    options::Plugins,
    plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder},
};
use syntect::highlighting::ThemeSet;
use syntect::html::{ClassStyle, css_for_theme_with_class_style};

/// Every stored page starts and ends with these, so serving can swap the theme and add the footer
/// without touching the user's content.
pub const PAGE_PREFIX: &str = r#"<!doctype html><html data-theme="dark">"#;
pub const PAGE_SUFFIX: &str = "</body></html>";

/// Pinned CDN builds for the standalone HTML download. Keep in step with `web/package.json`.
const CDN_KATEX: &str = "https://cdn.jsdelivr.net/npm/katex@0.19.0/dist";
const CDN_MERMAID: &str = "https://cdn.jsdelivr.net/npm/mermaid@12.2.0/dist/mermaid.min.js";

pub struct Renderer {
    syntect: SyntectAdapter,
}

struct Mermaid;

impl CodefenceRendererAdapter for Mermaid {
    fn write(
        &self,
        out: &mut dyn fmt::Write,
        _lang: &str,
        _meta: &str,
        code: &str,
        _pos: Option<comrak::nodes::Sourcepos>,
    ) -> fmt::Result {
        out.write_str("<pre class=\"mermaid\">")?;
        out.write_str(&escape(code))?;
        out.write_str("</pre>\n")
    }
}

pub fn escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            '\'' => o.push_str("&#39;"),
            c => o.push(c),
        }
    }
    o
}

fn options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.table = true;
    o.extension.tasklist = true;
    o.extension.strikethrough = true;
    o.extension.autolink = true;
    o.extension.footnotes = true;
    o.extension.math_dollars = true;
    o.render.r#unsafe = true;
    o
}

impl Default for Renderer {
    fn default() -> Self {
        Renderer { syntect: SyntectAdapterBuilder::new().css_with_class_prefix(HL).build() }
    }
}

impl Renderer {
    fn body(&self, md: &str) -> String {
        let mermaid = Mermaid;
        let mut plugins = Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&self.syntect);
        plugins.render.codefence_renderers.insert("mermaid".into(), &mermaid);
        comrak::markdown_to_html_with_plugins(md, &options(), &plugins)
    }

    /// The page stored next to a `.md` file. `title` is the file name.
    pub fn page(&self, md: &str, title: &str) -> String {
        let body = self.body(md);
        let (math, mermaid) = features(&body);
        let mut head = String::from(
            r#"<link rel="stylesheet" href="/_dropgo/assets/dropgo.css"><link rel="stylesheet" href="/_dropgo/assets/syntax.css">"#,
        );
        if math {
            head.push_str(r#"<link rel="stylesheet" href="/_dropgo/assets/vendor/katex/katex.min.css"><script defer src="/_dropgo/assets/vendor/katex/katex.min.js"></script>"#);
        }
        if mermaid {
            head.push_str(r#"<script defer src="/_dropgo/assets/vendor/mermaid.min.js"></script>"#);
        }
        if math || mermaid {
            head.push_str(r#"<script defer src="/_dropgo/assets/render.js"></script>"#);
        }
        wrap(title, &head, &body)
    }

    /// One self-contained file for the `html` download: dark theme, CSS inlined, no footer.
    pub fn standalone(&self, md: &str, title: &str, css: &str) -> String {
        let body = self.body(md);
        let (math, mermaid) = features(&body);
        let mut head = format!("<style>{css}\n{}</style>", syntax_css());
        if math {
            head.push_str(&format!(r#"<link rel="stylesheet" href="{CDN_KATEX}/katex.min.css"><script defer src="{CDN_KATEX}/katex.min.js"></script>"#));
        }
        if mermaid {
            head.push_str(&format!(r#"<script defer src="{CDN_MERMAID}"></script>"#));
        }
        if math || mermaid {
            head.push_str(&format!("<script>{RENDER_JS}</script>"));
        }
        wrap(title, &head, &body)
    }
}

pub const RENDER_JS: &str = include_str!("../static/render.js");

fn features(body: &str) -> (bool, bool) {
    (body.contains("data-math-style="), body.contains("<pre class=\"mermaid\">"))
}

fn wrap(title: &str, head: &str, body: &str) -> String {
    format!(
        "{PAGE_PREFIX}<head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>{}</title>{head}</head><body><main class=\"md\">{body}</main>{PAGE_SUFFIX}",
        escape(title)
    )
}

/// Prefix for syntect classes, so scope names like `meta` or `name` miss the page CSS.
const HL: &str = "hl-";

/// Highlight colors for both themes, scoped by `data-theme`.
pub fn syntax_css() -> String {
    let themes = ThemeSet::load_defaults();
    let css = |name: &str| {
        css_for_theme_with_class_style(&themes.themes[name], ClassStyle::SpacedPrefixed { prefix: HL })
            .expect("theme css")
    };
    format!(
        "[data-theme=dark] {{\n{}\n}}\n[data-theme=light] {{\n{}\n}}\n",
        css("base16-ocean.dark"),
        css("InspiredGitHub")
    )
}

/// Sets the theme on a stored page and inserts the footer before its closing tags.
pub fn personalize(stored: &str, theme: &str, footer: &str) -> Option<String> {
    let body = stored.strip_prefix(PAGE_PREFIX)?.strip_suffix(PAGE_SUFFIX)?;
    Some(format!("<!doctype html><html data-theme=\"{theme}\">{body}{footer}{PAGE_SUFFIX}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_gfm_code_and_diagrams() {
        let r = Renderer::default();
        let page = r.page(
            "# T\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n\n~~gone~~ https://example.com\n\nx[^1]\n\n[^1]: note\n\n```rust\nfn main() {}\n```\n\n```mermaid\ngraph TD; A-->B\n```\n\n$x^2$\n\n<div data-theme=\"x\">raw</div>\n",
            "notes.md",
        );
        assert!(page.starts_with(PAGE_PREFIX) && page.ends_with(PAGE_SUFFIX));
        for want in [
            "<table>",
            "type=\"checkbox\"",
            "<del>",
            "href=\"https://example.com\"",
            "footnote",
            "<span class=\"hl-source hl-rust\">",
            "<pre class=\"mermaid\">graph TD; A--&gt;B",
            "data-math-style",
            "<div data-theme=\"x\">raw</div>",
            "mermaid.min.js",
            "katex.min.js",
        ] {
            assert!(page.contains(want), "missing {want}");
        }
        let out = personalize(&page, "light", "<footer>f</footer>").unwrap();
        assert!(out.starts_with("<!doctype html><html data-theme=\"light\">"));
        assert!(out.contains("<div data-theme=\"x\">raw</div>"));
        assert!(out.ends_with("<footer>f</footer></body></html>"));
    }

    #[test]
    fn plain_pages_load_no_scripts() {
        let page = Renderer::default().page("hello", "a.md");
        assert!(!page.contains("<script"));
    }

    #[test]
    fn syntax_css_has_both_themes() {
        let css = syntax_css();
        assert!(css.contains("[data-theme=dark]") && css.contains("[data-theme=light]") && css.contains(".hl-keyword"));
    }
}
