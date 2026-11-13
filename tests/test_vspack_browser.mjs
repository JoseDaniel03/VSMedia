import assert from "node:assert/strict";
import {readFileSync} from "node:fs";
import vm from "node:vm";

const source=readFileSync("studio/assets/vspack-publish.js","utf8");
const target={window:{},document:{
  readyState:"loading",addEventListener(){}
}, TextEncoder,Uint8Array,Uint32Array,DataView,Blob,Math};
vm.runInNewContext(source,target,{filename:"vspack-publish.js"});
assert.equal(typeof target.window.VSPackBrowser?.zipFolder,"function");

const make=(path,data)=>{
  const buf=Buffer.from(data);
  return {name:path.split("/").pop(),webkitRelativePath:path,
    size:buf.length,async arrayBuffer(){return Uint8Array.from(buf).buffer;}};
};
const files=[
  make("MyPack/pack.mcmeta",'{"pack":{"description":"Virgoland","pack_format":15}}'),
  make("MyPack/assets/demo/models/item/rifle.json",
       '{"parent":"item/generated","textures":{"layer0":"demo:item/rifle"}}'),
  make("MyPack/assets/demo/textures/item/rifle.png",Buffer.from([137,80,78,71,13,10,26,10,1,2,3])),
  make("MyPack/assets/demo/textures/item/rifle.png.mcmeta",
       '{"animation":{"frametime":3}}'),
  make("MyPack/assets/demo/items/rifle.json",
       '{"model":{"type":"minecraft:model","model":"demo:item/rifle"}}')
];
const zip=await target.window.VSPackBrowser.zipFolder(files);
const bytes=new Uint8Array(await zip.arrayBuffer());
const view=new DataView(bytes.buffer);
let cursor=0;
const decoded=new Map();
const decoder=new TextDecoder();
for(let i=0;i<files.length;i++){
  assert.equal(view.getUint32(cursor,true),0x04034b50);
  assert.equal(view.getUint16(cursor+8,true),0,"ZIP creator should use stored entries");
  const nameLength=view.getUint16(cursor+26,true);
  const dataLength=view.getUint32(cursor+18,true);
  const name=decoder.decode(bytes.slice(cursor+30,cursor+30+nameLength));
  const start=cursor+30+nameLength;
  decoded.set(name,bytes.slice(start,start+dataLength));
  cursor=start+dataLength;
}
assert.deepEqual([...decoded.keys()],[
  "assets/demo/items/rifle.json",
  "assets/demo/models/item/rifle.json",
  "assets/demo/textures/item/rifle.png",
  "assets/demo/textures/item/rifle.png.mcmeta",
  "pack.mcmeta"
]);
const png=decoded.get("assets/demo/textures/item/rifle.png");
assert.deepEqual([...png.slice(0,8)],[137,80,78,71,13,10,26,10]);
assert.equal(view.getUint32(cursor,true),0x02014b50);
const footer=bytes.length-22;
assert.equal(view.getUint32(footer,true),0x06054b50);
assert.equal(view.getUint16(footer+10,true),5);
assert.equal(view.getUint32(footer+16,true),cursor);

await assert.rejects(
  target.window.VSPackBrowser.zipFolder(files.filter(f=>f.name!=="pack.mcmeta")),
  /pack.mcmeta/
);
await assert.rejects(
  target.window.VSPackBrowser.zipFolder([...files,
    make("MyPack/assets/../secret.txt","danger")]),
  /insegura/
);
console.log("VSPack browser folder ZIP tests passed: models, PNG, animation and manifest");
