# ADR-0002 D2 (Tauri desktop): status — blocked, not started

- It depends on D1/ADR-0003. Tauri hosts the Wasm UI, and the framework has not been chosen (see
  `ADR-0003-wasm-ui-framework-probe.md`).
- Packaging and signing for macOS and Windows need real machines and signing credentials, which are not
  available. ADR-0002 already lists this as a future blocker.
- A Linux build needs the WebKitGTK 4.1, GTK3 and libsoup3 development packages. The current box image has
  neither `pkg-config` nor these libraries, so a Linux shell build was not attempted.
PENDING (not dropped).
