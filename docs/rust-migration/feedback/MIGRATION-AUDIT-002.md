# MIGRATION-AUDIT-002 (closed 2026-10-13): serde_json `Value` equality ignores key order

With `preserve_order`, `serde_json::Map` equality is order-insensitive, because IndexMap compares as a set. Oracle tests that `assert_eq!` two `Value`s therefore do not check JS key order.

Audit:
- `model_references`, `session_store` and `file_store` (op results) already compared serialized strings.
- `command_parser` `validateCommand`, `frontmatter` and `transport` compared `Value`s; they now compare serialized strings.
- A mutation that moves `sequence` to the end of SDK stream events passed the old test and fails the new one.
- `file_store` on-disk trees are filename maps whose order is not observable, so they stay as they are.
- `web-ui-core` vectors (no `preserve_order`) feed Leptos components field by field and are never re-serialized, so key order is not observable there.
- Every test passes with string comparison.
