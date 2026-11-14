/* VSMedia Studio UI: menu placement and keyboard polish.
 * Additive enhancement only; no replacement for the FullCanvas Leptos UI.
 * Keeps native details semantics and recalculates anchored position after
 * window zoom, scroll and responsive reflow.
 */
(() => {
  "use strict";
  const menuSelector = ".editor-menu";
  let active = null;

  const clamp = (value, lo, hi) => Math.max(lo, Math.min(value, Math.max(lo, hi)));

  function position(details) {
    if (!details?.open || !details.isConnected) return;
    const summary = details.querySelector("summary");
    const popup = details.querySelector(".editor-menu-popover");
    if (!summary || !popup) return;
    const trigger = summary.getBoundingClientRect();
    const width = Math.min(popup.getBoundingClientRect().width || 195,
      Math.max(185, window.innerWidth - 20));
    const height = popup.getBoundingClientRect().height || 100;
    const margin = 10;
    const left = clamp(trigger.left, margin, window.innerWidth - width - margin);
    const below = window.innerHeight - trigger.bottom - 6;
    const above = trigger.top - 6;
    const top = below >= Math.min(height, 160) || below >= above
      ? trigger.bottom + 6
      : Math.max(margin, trigger.top - height - 6);
    popup.style.left = Math.round(left) + "px";
    popup.style.top = Math.round(clamp(top, margin,
      window.innerHeight - Math.min(height, window.innerHeight - 20) - margin)) + "px";
  }

  function close(except = null) {
    for (const details of document.querySelectorAll(menuSelector + "[open]")) {
      if (details !== except) details.open = false;
    }
    active = except;
  }

  // Native toggle is captured because it does not consistently bubble.
  // Bind to document so recreated editor screens work automatically.
  document.addEventListener("toggle", event => {
    const details = event.target;
    if (!details?.matches?.(menuSelector)) return;
    if (!details.open) {
      if (active === details) active = null;
      return;
    }
    close(details);
    position(details);
  }, true);

  document.addEventListener("click", event => {
    const clicked = event.target;
    const menu = clicked?.closest?.(menuSelector);
    if (!menu) {
      close();
    } else if (clicked.closest(".editor-menu-popover button")) {
      // Actions should not leave a floating menu above the next screen.
      close();
    }
  });

  document.addEventListener("keydown", event => {
    if (event.key !== "Escape" || !active) return;
    const summary = active.querySelector("summary");
    close();
    summary?.focus();
    event.preventDefault();
  });

  window.addEventListener("resize", () => {
    if (active) position(active);
  }, {passive: true});
  document.addEventListener("scroll", () => {
    if (active) position(active);
  }, {capture: true, passive: true});

  // Defensive cleanup when the Leptos workspace switches editors.
  const watch = new MutationObserver(() => {
    if (active && !active.isConnected) active = null;
  });
  const start = () => {
    if (document.body) watch.observe(document.body, {childList:true,subtree:true});
  };
  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", start, {once:true});
  } else start();
})();
