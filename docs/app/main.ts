document
  .querySelectorAll<HTMLElement>(".footnote-definition a[href^='#fr-']")
  .forEach((elem) => {
    elem.style.marginInlineEnd = "2px";
    if (elem.textContent) {
      elem.textContent = elem.textContent.replaceAll("↩", "↩\ufe0e");
    }
  });
