# SRC-DEFECT-008: oversized approval input gets no HTTP response instead of 413

- Source: `publicApprovalHttpTransport.ts` throws `APPROVAL_INPUT_TOO_LARGE` (status 413) from inside
  `for await (const chunk of req)`.
  1. Leaving the async iterator destroys the request.
  2. The `'close'` listener calls `controller.abort()`.
  3. The catch block sees `controller.signal.aborted` and destroys the response.
  4. The client gets a reset connection, never a 413.
- Reproduced in `gateway_server_parity.py` (case `public/ap_too_large`: TS gives no response).
- The host transport (`send(413)` then `return`) does deliver its 413 (case `public/h_too_large`).
- Rust treatment: **behavior-compatible**. The connection is closed without a response. **Pending user decision**:
  send the intended 413.

- ~~Decision (ADR-0004, 2026-10-13 13:34): keep reproducing the TS behaviour for now.~~
- FINAL (ADR-0004, user decision 2026-10-13 14:02): fixed in Rust, an intended behaviour difference. Oversized input gets HTTP 413 `APPROVAL_INPUT_TOO_LARGE`. The earlier "keep reproducing" decision is superseded.
