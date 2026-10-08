import { test } from "node:test";
import assert from "node:assert/strict";
import { mentionSuggestions, mentionEvents, mentionedText } from "./interactions.js";
const snapshot = (messages = [], direct = []) => ({username:"alice", rooms:[{name:"room", messages}], direct});
test("mentions complete room members and preserve Unicode drafts without matching emails", () => {
  assert.equal(mentionSuggestions("你好 @b", ["alice","bob"])[0].value, "你好 @bob ");
  assert.deepEqual(mentionSuggestions("mail@b", ["bob"]), []);
  assert.deepEqual(mentionSuggestions("/tell @b", ["bob"]), []);
  assert.deepEqual(mentionSuggestions("你好@b", ["bob"]), []);
  assert.deepEqual(mentionedText("你好 @alice! mail@alice.com", ["alice"]).filter(p => p.mention).map(p => p.text), ["@alice"]);
});
test("notifications include only new mentions, not own messages, reactions, joined history or cleanup backfill", () => {
  const old = {id:"old",from:"bob",time:10,text:"@alice",mentions:["alice"]};
  const fresh = {...old,id:"new",time:11};
  assert.equal(mentionEvents(snapshot([old]), snapshot([old,fresh])).length, 1);
  assert.equal(mentionEvents(snapshot([old]), snapshot([{...old,reactions:{"👍":["bob"]}}])).length, 0);
  assert.equal(mentionEvents(snapshot([old]), snapshot([old,{...fresh,from:"alice"}])).length, 0);
  assert.equal(mentionEvents(snapshot([old]), snapshot([{...fresh,time:1}])).length, 0);
  assert.equal(mentionEvents({username:"alice",rooms:[],direct:[]}, snapshot([fresh])).length, 0);
  const dm = {...fresh,to:"alice"};
  assert.equal(mentionEvents(snapshot(), snapshot([], [dm]))[0].view, "@direct:bob");
});
