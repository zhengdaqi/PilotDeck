# SRC-DEFECT-011: `@file` includes follow symlinks out of the base directory

- Source: `commandParser.js` `isPathSafe` checks only the path text (`path.relative`). An include such as `@link` that names a symlink inside the base which points outside it is read anyway.
- Fix: also compare the real path of the file with the real path of the base. The error is the same as TS's traversal error: `Invalid file path (directory traversal detected): <name>`.
- Evidence: intended cases `@link_out` and `@dirlink/secret.txt`. TS returns `SECRET`; Rust returns the traversal error. The mutation that removes the realpath check fails the test.
