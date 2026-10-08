import { test } from "node:test";
import assert from "node:assert/strict";
import { ingest, timeline, suggestions, privatePeers, privateMessages, redactCommand, helpSections } from "./console.js";

test("passwords are masked and help groups retain their commands", () => {
  assert.equal(redactCommand("/passwd old secret"), "/passwd •••• ••••");
  assert.equal(redactCommand("/user bob secret user"), "/user bob •••• user");
  assert.deepEqual(helpSections("[General]\n/help — Help\n\n[Rooms]\n/kick user — Kick"), [{title:"General", commands:[{usage:"/help",description:"Help"}]},{title:"Rooms", commands:[{usage:"/kick user",description:"Kick"}]}]);
});

test("local command output stays in its origin view", () => {
  const output = [{key:"room",room:"lol",order:1}, {key:"command",room:"@command",order:2}, {key:"private",room:"@direct:bob",order:3}];
  const ledger = {messages:new Map()};
  assert.deepEqual(timeline([], output, ledger, 0, "lol").map((entry) => entry.key), ["room"]);
  assert.deepEqual(timeline([], output, ledger, 0, "@command").map((entry) => entry.key), ["command"]);
  assert.equal(timeline([], output, ledger, 3, "@direct:bob").length, 0);
  assert.equal(timeline([], [], ledger, 0, "lol").length, 0);
});
test("private conversations contain only messages between the selected people", () => {
  const direct = [{from:"alice",to:"bob",text:"outgoing"}, {from:"bob",to:"alice",text:"incoming"}, {from:"carol",to:"alice",text:"other"}];
  assert.deepEqual(privateMessages(direct, "alice", "bob").map((message) => message.text), ["outgoing", "incoming"]);
  assert.deepEqual(privatePeers({username:"alice",users:["alice","bob","dave"],direct}), ["bob","carol"]);
  assert.deepEqual(privatePeers({username:"alice",users:["alice","bob"],direct:[]}), []);
  assert.deepEqual(privatePeers({private_peers:["bob"],direct:[]}), ["bob"]);
  assert.deepEqual(privateMessages(direct, "alice", null), []);
});

test("snapshots preserve output ordering, clear is local, refresh only restores chat", () => {
  const first = { id: "first", time: 1, text: "retained" };
  const initial = { rooms: [{ messages: [first] }], direct: [] };
  const ledger = ingest(initial);
  const output = [
    {
      kind: "command",
      key: "local",
      order: ++ledger.sequence,
      result: "admin",
    },
  ];
  const next = { id: "next", time: 1, text: "live" };
  ingest({ rooms: [{ messages: [first, next] }], direct: [] }, ledger);
  assert.deepEqual(
    timeline([first, next], output, ledger).map((e) => e.key),
    ["first", "local", "next"],
  );
  assert.equal(
    timeline([first, next], output, ledger, ledger.sequence).length,
    0,
  );
  const refreshed = ingest({
    rooms: [{ messages: [first, next] }],
    direct: [],
  });
  assert.deepEqual(
    timeline([first, next], [], refreshed).map((e) => e.key),
    ["first", "next"],
  );
});
test("hints appear only for slash commands and complete usernames or rooms", () => {
  const commands = [
    { name: "/tell", usage: "/tell user message", description: "Private" },
    { name: "/join", usage: "/join room", description: "Switch" },
  ];
  assert.deepEqual(suggestions("hello", commands, ["bob"], ["lobby"]), []);
  assert.equal(
    suggestions("/te", commands, ["bob"], ["lobby"])[0].value,
    "/tell ",
  );
  assert.equal(
    suggestions("/tell b", commands, ["bob"], ["lobby"])[0].value,
    "/tell bob ",
  );
  assert.equal(
    suggestions("/join l", commands, ["bob"], ["lobby"])[0].value,
    "/join lobby ",
  );
  assert.equal(
    suggestions("/tell bob hello", commands, ["bob"], ["lobby"])[0].value,
    null,
  );
});
