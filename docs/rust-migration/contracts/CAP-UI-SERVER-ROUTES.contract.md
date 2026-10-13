# CAP-UI-SERVER-ROUTES contract: ui/server HTTP routes on axum

Source: `ui/server/index.js` (Express 4.22 app) and `ui/server/routes/*.js`. Target: `rust/crates/ui-server`
(axum 0.8 + tokio), binary `pilotdeck-ui-server`.

Host rules kept from Express:
- `res.json` → compact JSON, `content-type: application/json; charset=utf-8`;
- `express.json({ type: includes('json') && !multipart })` → non-JSON requests see `req.body = {}`; JSON bodies are
  strict (object/array);
- Node fs error objects (`code`, `"<CODE>: <text>, <syscall> '<path>'"`) via `node_fs::NodeError`.

Parity method: each route is served by the real Express handler and by the axum host against the same on-disk
fixtures; status, content-type and body are compared byte for byte.

## Slice 31
- Host skeleton (`AppState` with project resolver + env, `router`, `json`, `express_json_body`, `node_fs`).
- `POST /api/projects/{projectName}/files/upload/check` (`createWorkspaceUploadCheckHandler`): 3 seeds × 2000
  requests identical (`upload_check_parity.mjs`); evidence ui-server-axum-upload-check.txt.

Pending (not dropped):
- middleware: authenticateToken, requireRealProjectFilesystem, cors, the real extractProjectDirectory resolver;
- the multipart upload handler (multer-compatible parser, staging, hard-link publish, concurrency limit);
- every other route module (agent, auth, commands, config, cron-jobs, gateway, git, mcp, memory, messages, models,
  modules, onboarding, plugins, projects, settings, skills, sop, staffdeck-copy, taskmaster, update, uploads, user,
  webUpdate) and index.js file/project endpoints, WebSocket proxying and static serving.
