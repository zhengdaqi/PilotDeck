# SRC-DEFECT-007 (security): `/api/web/projects/:key/files/write` follows symlinks out of the workspace

- Source: in `ProjectFileService.writeFile` (`src/adapters/web/projectFiles.ts`), `resolveSafeWriting` only checks the
  lexical path. It never realpath-checks the target or any existing parent. `fs.writeFile` follows symlinks, so a write to
  `link_out/planted.txt` (where `link_out -> /outside`) or to `secret_link.txt` (a symlink to a file outside the root)
  writes outside the project root and returns `200 {"ok":true}`.
- Reads are protected: `resolveSafe` does run realpath, so these reads return 403.
- Reproduced on the TS dist server in `gateway_server_parity.py` (cases `w_via_link_out` and `w_via_secret_link`, plus
  `outside_after`, which lists the planted file).
- Also note: `projectRoot` defaults to the URL `projectKey`, because the server entry binds no `resolveProject`. Any
  authenticated client can therefore read and write under any absolute directory it names.
- Rust treatment: **behavior-compatible (same escape)**. The port is not allowed to silently change semantics.
  **Pending user decision**: either realpath-check the deepest existing ancestor and the target before writing, returning
  403 `workspace_boundary`, or keep the TS behavior.

- Decision (ADR-0004, 2026-10-13): keep reproducing the TS behaviour for now.
