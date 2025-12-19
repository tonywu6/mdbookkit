// https://github.com/rust-lang/mdBook/blob/master/CHANGELOG.md#mdbook-0448
document
  .querySelectorAll<HTMLElement>(".footnote-definition a[href^='#fr-']")
  .forEach((elem) => {
    elem.style.marginInlineEnd = "2px";
    if (elem.textContent) {
      elem.textContent = elem.textContent.replaceAll("↩", "↩\ufe0e");
    }
  });

(async () => {
  if (document.querySelector("pre.mermaid")) {
    const { default: mermaid } = await import("mermaid");
    mermaid.initialize({
      securityLevel: "antiscript",
      theme: "dark",
      themeVariables: {
        fontFamily: `
          "Noto Sans",
          "Open Sans",
          -apple-system,
          BlinkMacSystemFont,
          "Segoe UI",
          "Helvetica Neue",
          ui-sans-serif,
          sans-serif,
          "Apple Color Emoji",
          "Segoe UI Emoji";
        `,
      },
    });
    await mermaid.run();
  }
})();
