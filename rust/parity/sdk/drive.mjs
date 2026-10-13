// Runs a session-store op script against an SDK implementation (TS source or the wasm npm wrapper).
// api: { InMemorySessionStore, foldSessionSummary, summaryToSessionInfo, importSessionToStore, setNow(ms) }
export async function drive(api, ops) {
  const stores = [];
  const out = [];
  for (const o of ops) {
    let r;
    try {
      switch (o.op) {
        case "new": stores.push(new api.InMemorySessionStore()); r = stores.length - 1; break;
        case "now": api.setNow(o.v); r = null; break;
        case "append": r = (await stores[o.s].append(o.key, o.entries)) ?? null; break;
        case "load": r = await stores[o.s].load(o.key); break;
        case "listSessions": r = await stores[o.s].listSessions(o.pk); break;
        case "listSessionSummaries": r = await stores[o.s].listSessionSummaries(o.pk); break;
        case "listSubkeys": r = await stores[o.s].listSubkeys({ projectKey: o.pk, sessionId: o.sid }); break;
        case "delete": r = (await stores[o.s].delete(o.key)) ?? null; break;
        case "import": r = (await api.importSessionToStore(o.sid, stores[o.s], o.entries, o.opts)) ?? null; break;
        case "fold": r = api.foldSessionSummary(o.prev ?? undefined, o.key, o.entries, o.opts ?? undefined); break;
        case "info": r = api.summaryToSessionInfo(o.summary); break;
        default: throw new Error("bad op " + o.op);
      }
    } catch (e) { r = { __error: String(e?.message ?? e) }; }
    out.push(JSON.parse(JSON.stringify(r === undefined ? null : r)));
  }
  return out;
}
