// Draws mermaid diagrams and KaTeX math in rendered Markdown pages.
document.addEventListener('DOMContentLoaded', function () {
  var dark = document.documentElement.getAttribute('data-theme') !== 'light';
  if (window.mermaid) {
    window.mermaid.initialize({ startOnLoad: false, theme: dark ? 'dark' : 'default', securityLevel: 'strict' });
    window.mermaid.run({ querySelector: 'pre.mermaid' });
  }
  if (window.katex) {
    document.querySelectorAll('[data-math-style]').forEach(function (el) {
      window.katex.render(el.textContent, el, { displayMode: el.getAttribute('data-math-style') === 'display', throwOnError: false });
    });
  }
});
