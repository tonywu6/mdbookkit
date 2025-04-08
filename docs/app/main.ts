// https://github.com/rust-lang/mdBook/blob/master/CHANGELOG.md#mdbook-0448
document
  .querySelectorAll<HTMLElement>(".footnote-definition a[href^='#fr-']")
  .forEach((elem) => {
    elem.style.marginInlineEnd = "2px";
    if (elem.textContent) {
      elem.textContent = elem.textContent.replaceAll("↩", "↩\ufe0e");
    }
  });
