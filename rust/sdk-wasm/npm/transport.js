// Browser/Node WebSocket host for the wasm transport core: same API and timing shape as
// packages/sdk/src/transport.ts GatewayTransport. Frame handling, close mapping and error normalization run in wasm.
import { WasmTransportCore, serverInfo, helloFrame, closeError, retryDelayMs, mapError as mapRaw } from "./pkg/pilotdeck_sdk_wasm.js";
import "./index.js"; // initializes the wasm module

export class PilotDeckError extends Error {
  constructor(e) {
    super(e.message);
    this.name = "PilotDeckError";
    this.code = e.code;
    this.retryable = e.retryable ?? false;
    if (e.details !== undefined) this.details = e.details;
    if (e.requestId !== undefined) this.requestId = e.requestId;
    if (e.cause !== undefined) this.cause = e.cause;
  }
}
const err = (e) => new PilotDeckError(e);
const uuid = () => globalThis.crypto.randomUUID();
const delay = (ms) => new Promise((r) => setTimeout(r, ms));

export function mapError(error) {
  if (error instanceof PilotDeckError) return error;
  const code = typeof error?.code === "string" ? error.code : undefined;
  const message = typeof error?.message === "string" ? error.message : String(error);
  const e = JSON.parse(mapRaw(code, message, JSON.stringify(error?.details ?? null)));
  if (error?.details !== undefined) e.details = error.details;
  return err({ ...e, cause: error?.cause ?? error });
}

export class AsyncEventQueue {
  #values = []; #waiters = []; #closed = false; #error;
  push(v) { const w = this.#waiters.shift(); if (w) w.resolve({ done: false, value: v }); else this.#values.push(v); }
  close() { this.#closed = true; while (this.#waiters.length) this.#waiters.shift().resolve({ done: true, value: undefined }); }
  fail(e) { this.#error = e; this.#closed = true; while (this.#waiters.length) this.#waiters.shift().reject(e); }
  [Symbol.asyncIterator]() { return { next: () => { if (this.#values.length) return Promise.resolve({ done: false, value: this.#values.shift() }); if (this.#error) return Promise.reject(this.#error); if (this.#closed) return Promise.resolve({ done: true, value: undefined }); return new Promise((resolve, reject) => this.#waiters.push({ resolve, reject })); } }; }
}

export class GatewayTransport {
  #o; #core = new WasmTransportCore(); #socket; #pending = new Map(); #streams = new Map(); #listeners = new Set();
  #connecting; #openReject; #helloReject;
  constructor(options) { this.#o = options; }

  connect() {
    const hello = this.#core.hello();
    if (hello && this.#socket?.readyState === 1) return Promise.resolve(JSON.parse(serverInfo(hello)));
    if (this.#connecting) return this.#connecting;
    const c = this.#openWithRetry();
    this.#connecting = c;
    const clear = () => { if (this.#connecting === c) this.#connecting = undefined; };
    c.then(clear, clear);
    return c;
  }

  async #openWithRetry() {
    const p = this.#o.reconnect;
    const maxAttempts = Math.max(1, Math.floor(p?.maxAttempts ?? 1));
    const initial = p?.initialDelayMs ?? 100, max = p?.maxDelayMs ?? 2_000;
    const jitter = Math.min(1, Math.max(0, p?.jitter ?? 0.2));
    let attempt = 0;
    while (true) {
      if (this.#core.closed()) throw err({ code: "transport_error", message: "Gateway client closed.", retryable: false });
      attempt += 1;
      try { return await this.#open(); } catch (error) {
        const mapped = mapError(error);
        if (attempt >= maxAttempts || !(mapped.code === "transport_error" || mapped.code === "timeout") || this.#core.closed()) throw mapped;
        const failed = this.#socket; this.#socket = undefined; this.#core.setConnected(false); failed?.close();
        const factor = jitter === 0 ? 1 : 1 + ((Math.random() * 2 - 1) * jitter);
        await delay(retryDelayMs(attempt, initial, max, factor));
      }
    }
  }

  async #open() {
    const Socket = globalThis.WebSocket;
    if (!Socket) throw err({ code: "transport_error", message: "WebSocket is unavailable in this runtime.", retryable: false });
    this.#core.clearHello();
    const socket = new Socket(this.#o.url);
    this.#socket = socket;
    socket.addEventListener("message", (ev) => { if (this.#socket === socket) this.#apply(this.#core.handleMessage(String(ev.data ?? ""))); });
    socket.addEventListener("close", (ev) => {
      if (this.#socket !== socket) return;
      const code = Number(ev?.code ?? 0), reason = String(ev?.reason ?? "");
      const e = err(JSON.parse(closeError(code, reason)));
      this.#openReject?.(e); this.#helloReject?.(e);
      this.#core.setConnected(true); // the close event fails everything pending on this socket
      this.#apply(this.#core.handleClose(code, reason), e);
    });
    await new Promise((resolve, reject) => {
      if (socket.readyState === 1) return resolve();
      let timer;
      const cleanup = () => { clearTimeout(timer); if (this.#openReject === rejectOpen) this.#openReject = undefined; };
      const rejectOpen = (e) => { cleanup(); reject(e); };
      this.#openReject = rejectOpen;
      timer = setTimeout(() => rejectOpen(err({ code: "timeout", message: "Gateway connection timed out.", retryable: true })), this.#o.connectTimeoutMs ?? 10_000);
      socket.addEventListener("open", () => { cleanup(); resolve(); });
      socket.addEventListener("error", () => rejectOpen(err({ code: "transport_error", message: "Gateway connection failed.", retryable: true })));
    });
    if (this.#core.closed() || this.#socket !== socket) throw err({ code: "transport_error", message: "Gateway client closed.", retryable: false });
    this.#core.setConnected(true);
    socket.send(helloFrame(this.#o.protocolVersion, this.#o.clientName, this.#o.clientVersion, this.#o.token));
    const hello = await new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.#helloReject = undefined; reject(err({ code: "timeout", message: "Gateway hello timed out.", retryable: true })); }, this.#o.connectTimeoutMs ?? 10_000);
      this.#helloReject = (e) => { clearTimeout(timer); this.#helloReject = undefined; reject(e); };
      const poll = () => {
        const h = this.#core.hello();
        if (h) { clearTimeout(timer); this.#helloReject = undefined; return resolve(h); }
        if (!this.#socket) { clearTimeout(timer); this.#helloReject = undefined; return reject(err({ code: "transport_error", message: "Gateway disconnected during hello.", retryable: true })); }
        setTimeout(poll, 10);
      };
      poll();
    });
    return JSON.parse(serverInfo(hello));
  }

  #send(frameJson) {
    const r = JSON.parse(frameJson);
    if (r.error || !this.#socket || this.#socket.readyState !== 1) throw err(r.error ?? { code: "transport_error", message: "Gateway WebSocket is not connected.", retryable: true });
    this.#socket.send(r.frame);
  }

  request(method, params) {
    const id = uuid();
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { this.#pending.delete(id); const e = this.#core.timeoutRequest(id, method); reject(err(e ? JSON.parse(e) : { code: "timeout", message: `Gateway request timed out: ${method}`, requestId: id, retryable: true })); }, this.#o.requestTimeoutMs ?? 30_000);
      this.#pending.set(id, { resolve, reject, timer });
      try { this.#send(this.#core.beginRequest(id, method, JSON.stringify(params ?? null))); }
      catch (e) { clearTimeout(timer); this.#pending.delete(id); this.#core.timeoutRequest(id, method); reject(e); }
    });
  }

  stream(method, params) {
    const id = uuid();
    const q = new AsyncEventQueue();
    this.#streams.set(id, q);
    try { this.#send(this.#core.beginStream(id, method, JSON.stringify(params ?? null))); }
    catch (e) { this.#streams.delete(id); q.fail(e); }
    return q;
  }

  onNotification(listener) { this.#listeners.add(listener); return () => this.#listeners.delete(listener); }

  close() {
    if (this.#core.closed()) return;
    const socket = this.#socket;
    const e = err({ code: "transport_error", message: "Gateway client closed.", retryable: false });
    this.#openReject?.(e); this.#helloReject?.(e);
    this.#apply(this.#core.close(), e);
    this.#socket = undefined;
    socket?.close();
  }

  // `failWith`: the same Error object TS hands to every waiter on a close.
  #apply(effectsJson, failWith) {
    for (const ef of JSON.parse(effectsJson)) {
      if (ef.t === "settle") {
        const p = this.#pending.get(ef.id); if (!p) continue;
        clearTimeout(p.timer); this.#pending.delete(ef.id);
        if (ef.ok) p.resolve(ef.value); else p.reject(failWith ?? err(ef.error));
      } else if (ef.t === "item") this.#streams.get(ef.id)?.push(ef.value);
      else if (ef.t === "end") {
        const q = this.#streams.get(ef.id); if (!q) continue;
        this.#streams.delete(ef.id);
        if (ef.error) q.fail(failWith ?? err(ef.error)); else q.close();
      } else if (ef.t === "note") {
        for (const l of this.#listeners) { try { l(ef.value); } catch { /* listeners must not break transport semantics */ } }
      } else if (ef.t === "drop") {
        const h = this.#helloReject; this.#helloReject = undefined;
        h?.(failWith ?? err({ code: "validation_error", message: "Invalid Gateway JSON frame.", retryable: false }));
        this.#socket = undefined;
      }
    }
  }
}
