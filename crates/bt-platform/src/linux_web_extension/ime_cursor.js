(() => {
  "use strict";
  const page = globalThis;
  const document = page.document;
  const stringify = JSON.stringify;
  const binding = page["__FOLIO_IME_BINDING__"];
  const token = "__FOLIO_IME_TOKEN__";
  const signal = event => {
    if (!event.isTrusted) return;
    binding(stringify({token}));
  };
  for (const type of [
    "focusin",
    "focusout",
    "input",
    "keyup",
    "mouseup",
    "pointerup",
    "compositionupdate",
    "compositionend",
    "selectionchange",
    "scroll"
  ]) {
    (type === "selectionchange" ? document : page).addEventListener(type, signal, true);
  }
  page.addEventListener("resize", signal, true);
})();
