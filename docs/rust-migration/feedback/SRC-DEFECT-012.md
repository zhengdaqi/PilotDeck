# SRC-DEFECT-012: `@file` includes drop the newline or tab in front of them

- Source: `commandParser.js` `processFileIncludes` keeps the leading character of the match only when it is a space. A newline, tab or CR in front of `@file` is lost, which joins lines (`x\n@a.md` becomes `xA`).
- Fix: keep the whitespace that was matched.
- Evidence: intended cases `x\n@a.md`, `x\t@a.md`, `x\r\n@a.md`, `@nl.md` and `@a.md\n@b.md`.
