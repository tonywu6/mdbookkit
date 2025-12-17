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
  const diagrams = document.querySelectorAll("pre:has(code.language-mermaid)");

  if (!diagrams.length) {
    return;
  }

  diagrams.forEach((elem) => {
    const code = elem.querySelector("code")?.textContent;
    if (!code) {
      return;
    }
    elem.textContent = code;
    elem.setAttribute("class", "mermaid");
  });

  const { default: mermaid } = await import("mermaid");
  mermaid.initialize({});
  await mermaid.run();
})();
