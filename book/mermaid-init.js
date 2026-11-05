// Renders ```mermaid blocks. mdBook turns them into <pre><code class="language-mermaid">; mermaid
// wants <pre class="mermaid"> holding the diagram text. No preprocessor is involved, so the build
// needs nothing but mdbook. mermaid.min.js is mermaid 11.6.0 (MIT; its licence line is in the file).
(() => {
    const darkThemes = ['ayu', 'navy', 'coal'];
    const lightThemes = ['light', 'rust'];
    const classes = document.documentElement.classList;
    const dark = darkThemes.some((t) => classes.contains(t));

    const diagrams = [];
    for (const code of document.querySelectorAll('pre > code.language-mermaid')) {
        const pre = document.createElement('pre');
        pre.className = 'mermaid';
        pre.textContent = code.textContent;
        code.parentElement.replaceWith(pre);
        diagrams.push(pre);
    }
    if (diagrams.length === 0) return;

    mermaid.initialize({ startOnLoad: false, theme: dark ? 'dark' : 'default' });
    mermaid.run({ nodes: diagrams });

    // Re-rendering in another theme is simplest by reloading, as mdbook-mermaid does.
    for (const theme of [...darkThemes, ...lightThemes]) {
        const button = document.getElementById('mdbook-theme-' + theme);
        if (!button) continue;
        button.addEventListener('click', () => {
            if (darkThemes.includes(theme) !== dark) window.location.reload();
        });
    }
})();
