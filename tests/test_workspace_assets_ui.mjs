import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import vm from "node:vm";

const js=readFileSync("studio/assets/workspace-assets.js","utf8");
const css=readFileSync("studio/assets/workspace-assets.css","utf8");
const html=readFileSync("studio/index.html","utf8");
const backend=readFileSync("studio/backend/src/workspace_assets.rs","utf8");
assert.ok(html.includes("workspace-assets.js"),"The actual Studio must load the new workspace UI");
assert.ok(html.includes("workspace-assets.css"),"The workspace stylesheet must be loaded");
assert.ok(js.includes("data-vsassets-tab"),"Navigation tab is required");
assert.ok(js.includes("data-vsassets-home"),"Home workspace entry is required");
assert.ok(js.includes("data-assets-folder-input"),"Folder import is required");
assert.ok(js.includes("data-assets-drop"),"Drag/drop import is required");
assert.ok(js.includes("URL.createObjectURL"),"Safe local preview/download is required");
for(const path of ["/files","/file","/folders","/move","/verify","/export"]){
  assert.ok(js.includes(path),"Missing project asset endpoint "+path);
}
assert.ok(!js.includes("/api/v1/packs/"),"Workspace UI must not automatically publish to VSPack");
assert.ok(!backend.includes("vsmedia_pack::archive"),"Workspace backend must not implicitly produce resource packs");
assert.ok(backend.includes('"resourcepack_published":false'),
  "Project writes must declare that they do not publish to Minecraft");
assert.ok(backend.includes("Sha256::digest"),"Integrity must use SHA-256");
assert.ok(backend.includes("segments(path,false)"),"File operations must validate paths");

let braces=0;
const trimmed=css.replace(/\/\*[\s\S]*?\*\//g,"");
for(const ch of trimmed){if(ch==="{")braces++;if(ch==="}")braces--;assert.ok(braces>=0);}
assert.equal(braces,0);
for(const selector of [
  ".workspace-assets-screen[hidden]",".assets-file-row",".assets-preview",
  ".assets-drop-zone.drag-over","@media(max-width:750px)",
  "@media(prefers-reduced-motion:reduce)"
])assert.ok(css.includes(selector),"Missing UX style: "+selector);

let start=null;
const sandbox={
  document:{readyState:"loading",addEventListener(event,listener){
    if(event==="DOMContentLoaded")start=listener;
  }},
  window:{},
  MutationObserver:class {observe(){}},
  URL,Math,
};
vm.runInNewContext(js,sandbox,{filename:"workspace-assets.js"});
assert.equal(typeof start,"function","Workspace is mounted only after DOM is ready");

console.log("VSAssets UI smoke tests passed: backend isolation, project CRUD, previews, layout, no automatic Resource Pack upload");
