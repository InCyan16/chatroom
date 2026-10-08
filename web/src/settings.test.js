import assert from "node:assert/strict";
import test from "node:test";
import {defaultFonts, fontSettings, loadFonts, saveFonts} from "./settings.js";
test("font preferences survive refresh and reset within independent size limits", () => {
  let saved = null;
  const storage = {getItem:()=>saved,setItem:(_,value)=>{saved=value;}};
  assert.deepEqual(loadFonts(storage), defaultFonts);
  saveFonts(storage, {chat:22,ui:17});
  assert.deepEqual(loadFonts(storage), {chat:22,ui:17});
  assert.deepEqual(fontSettings({chat:100,ui:3}), {chat:24,ui:12});
  saveFonts(storage, defaultFonts);
  assert.deepEqual(loadFonts(storage), defaultFonts);
});
test("malformed or unavailable browser storage keeps the interface usable", () => {
  assert.deepEqual(loadFonts({getItem:()=>"broken"}), defaultFonts);
  assert.deepEqual(loadFonts({getItem:()=>'{"chat":null,"ui":"huge"}'}), defaultFonts);
  const blocked = {getItem:()=>{throw Error();},setItem:()=>{throw Error();}};
  assert.deepEqual(loadFonts(blocked), defaultFonts);
  assert.doesNotThrow(()=>saveFonts(blocked,{chat:20,ui:16}));
});
