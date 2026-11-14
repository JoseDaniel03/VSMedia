import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import vm from "node:vm";

const css = readFileSync("studio/assets/studio.css", "utf8");
const html = readFileSync("studio/index.html", "utf8");
const dj = readFileSync("studio/assets/radio-dj.js", "utf8");
const menus = readFileSync("studio/assets/ui-polish.js", "utf8");

// Structural checks supplement the real Trunk build and JS syntax checks.
// They protect the standalone publishing drawer and DJ row against regressions.
const withoutComments = css.replace(/\/\*[\s\S]*?\*\//g, "");
let braces = 0;
for (const ch of withoutComments) {
  if (ch === "{") braces++;
  if (ch === "}") braces--;
  assert.ok(braces >= 0, "Unexpected CSS closing bracket");
}
assert.equal(braces, 0, "CSS blocks must be balanced");
for (const name of [
  "--violet-dark:", "--gold-soft:", ".vs-minecraft-body [data-mc-page]",
  ".vs-minecraft-tabs [aria-pressed=true]", ".vsradio-live-controls",
  ".editor-menu-popover", "@media (max-width:380px)",
  "@media (prefers-reduced-motion:reduce)"
]) assert.ok(css.includes(name), "Missing Studio visual rule: " + name);
assert.ok(html.includes("ui-polish.js"), "Menu positioning enhancer must be bundled");
assert.ok(html.includes("vspack-publish.js"), "Keep VSPack original publishing");
assert.ok(html.includes("radio-dj.js"), "Keep live VSRadio controls");
assert.ok(dj.includes("radio.after(wrap)"),
  "DJ controls must live below, not overflow the main toolbar");
assert.ok(dj.includes('role","group"'),
  "DJ controls should remain a labelled group");

// DOM-less bounded viewport test of the actual menu event handlers.
const handlers = new Map();
const menusOpen = [];
const document = {
  readyState:"loading", body:{},
  addEventListener(name, cb) {handlers.set(name, cb);},
  querySelectorAll(selector) {
    assert.equal(selector, ".editor-menu[open]");
    return menusOpen.filter(menu=>menu.open);
  },
};
const windows = new Map();
const window = {
  innerWidth:360, innerHeight:480,
  addEventListener(name, cb) {windows.set(name, cb);}
};
class FakeObserver {
  observe() {}
}
vm.runInNewContext(menus, {
  window, document, MutationObserver:FakeObserver, Math
}, {filename:"ui-polish.js"});
assert.ok(handlers.has("toggle"));
assert.ok(handlers.has("keydown"));
assert.ok(handlers.has("click"));
let focused=false;
function mockMenu(left,top) {
  const summary={
    getBoundingClientRect:()=>({left,top,bottom:top+34}),
    focus(){focused=true;}
  };
  const popup={
    style:{},
    getBoundingClientRect:()=>({width:220,height:180})
  };
  return {
    open:true,isConnected:true,
    matches:selector=>selector===".editor-menu",
    querySelector:selector=>selector==="summary"?summary:popup,
    popup
  };
}
const first=mockMenu(280,100);
menusOpen.push(first);
handlers.get("toggle")({target:first});
assert.ok(Number.parseFloat(first.popup.style.left) <= 130,
  "Menu must not overflow right side at 360px width");
assert.ok(Number.parseFloat(first.popup.style.top) > 0);
const second=mockMenu(20,300);
menusOpen.push(second);
handlers.get("toggle")({target:second});
assert.equal(first.open,false,"Opening another menu closes the first");
handlers.get("keydown")({
  key:"Escape", preventDefault(){this.prevented=true;}
});
assert.equal(second.open,false,"Escape closes dropdown");
assert.ok(focused,"Escape restores keyboard focus");
console.log("VSMedia UI polish smoke checks passed: CSS, drawer, DJ bar, menu viewport/keyboard");
