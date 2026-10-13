//! Oracle test against `rust/parity/ui_server/gen_settings_files.mjs` (permissionSettings.js, mcpConfig.js).
use pilotdeck_skills::jsjson::{stringify_compact, JsObject, JsValue};
use pilotdeck_skills::v8json::parse;
use pilotdeck_ui_server_core::settings_files::*;
use serde_json::{json, Value};

fn js(v: &JsValue) -> Value {
    serde_json::from_str(&stringify_compact(v)).unwrap()
}

fn s(v: &Value) -> String {
    v.to_string()
}

#[test]
fn normalizers_match_ts() {
    let f: Value = serde_json::from_str(include_str!("fixtures/settings_files.json")).unwrap();
    for c in f["normEntry"].as_array().unwrap() {
        assert_eq!(normalize_permission_entry(&parse(c["in"].as_str().unwrap()).unwrap()), c["out"].as_str().unwrap(), "{c}");
    }
    for c in f["normPerm"].as_array().unwrap() {
        assert_eq!(stringify_compact(&normalize_permission_settings(&parse(c["in"].as_str().unwrap()).unwrap())), c["out"].as_str().unwrap(), "{c}");
    }
    let mut errs = 0;
    for c in f["normMcp"].as_array().unwrap() {
        let got = match normalize_mcp_config(&parse(c["in"].as_str().unwrap()).unwrap()) {
            Ok(v) => json!({ "ok": stringify_compact(&v) }),
            Err(e) => {
                errs += 1;
                json!({ "err": e })
            }
        };
        assert_eq!(s(&got), s(&c["out"]), "{}", c["in"]);
    }
    assert!(errs > 200, "{errs}");
}

#[test]
fn file_scripts_match_ts() {
    let f: Value = serde_json::from_str(include_str!("fixtures/settings_files.json")).unwrap();
    let mut n = 0;
    for sc in f["scripts"].as_array().unwrap() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = std::fs::canonicalize(tmp.path()).unwrap();
        let d = dir.to_str().unwrap().to_string();
        let home = format!("{d}/home");
        let proj = format!("{d}/proj");
        std::fs::create_dir(&proj).unwrap();
        let env = Env::new();
        let proj_of = |p: &str| -> Option<String> {
            match p {
                "P" => Some(proj.clone()),
                "HOME" => Some(home.clone()),
                "FILE" => Some(format!("{d}/afile")),
                "NEW" => Some(format!("{d}/newproj")),
                _ => None,
            }
        };
        for (op, want) in sc["ops"].as_array().unwrap().iter().zip(sc["outs"].as_array().unwrap()) {
            if op["proj"] == "FILE" {
                std::fs::write(format!("{d}/afile"), "x").unwrap();
            }
            let wrap = |r: Result<JsValue, String>| match r {
                Ok(v) => json!({ "ok": js(&v) }),
                Err(e) => json!({ "err": e }),
            };
            let mcp_file = |scope: &str| if scope == "global" { format!("{home}/mcp.json") } else { format!("{proj}/.pilotdeck/mcp.json") };
            let got = match op["op"].as_str().unwrap() {
                "plantPerm" => {
                    std::fs::create_dir_all(&home).unwrap();
                    std::fs::write(format!("{home}/permissions.json"), op["text"].as_str().unwrap()).unwrap();
                    json!({ "ok": null })
                }
                "readPerm" => json!({ "ok": js(&read_permission_settings(&home)) }),
                "writePerm" => wrap(write_permission_settings(&parse(op["updates"].as_str().unwrap()).unwrap(), &home, "2026-10-13T06:00:00.000Z")),
                "plantMcp" | "mcpDir" => {
                    let file = mcp_file(op["scope"].as_str().unwrap());
                    let p = std::path::Path::new(&file);
                    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                    let _ = std::fs::remove_dir_all(p);
                    let _ = std::fs::remove_file(p);
                    if op["op"] == "plantMcp" {
                        std::fs::write(p, op["text"].as_str().unwrap()).unwrap();
                    } else {
                        std::fs::create_dir_all(p).unwrap();
                    }
                    json!({ "ok": null })
                }
                "readMcp" => wrap(read_mcp_config_file(op["scope"].as_str().unwrap(), proj_of(op["proj"].as_str().unwrap()).as_deref(), &home, &env, &d)),
                "writeMcp" => wrap(write_mcp_config_file(op["scope"].as_str().unwrap(), op["raw"].as_str().unwrap(), proj_of(op["proj"].as_str().unwrap()).as_deref(), &home, &env, &d)),
                "listMcp" => {
                    // Promise.all([global, project]): the first rejection wins, which is the global one if both fail.
                    let p = proj_of(op["proj"].as_str().unwrap());
                    let g = read_mcp_config_file("global", p.as_deref(), &home, &env, &d);
                    let pr = read_mcp_config_file("project", p.as_deref(), &home, &env, &d);
                    match (g, pr) {
                        (Ok(g), Ok(pr)) => {
                            let mut o = JsObject::new();
                            o.set("global", g);
                            o.set("project", pr);
                            json!({ "ok": js(&JsValue::Obj(o)) })
                        }
                        // Both rejected: either message may win the Promise.all race (like `__errorOneOf`).
                        (Err(e1), Err(e2)) => {
                            let w = want["err"].as_str().unwrap_or("").replace("<dir>", &d);
                            json!({ "err": if w == e2 { e2 } else { e1 } })
                        }
                        (Err(e), _) | (_, Err(e)) => json!({ "err": e }),
                    }
                }
                x => panic!("{x}"),
            };
            let got: Value = serde_json::from_str(&got.to_string().replace(&d, "<dir>")).unwrap();
            assert_eq!(s(&got), s(want), "op {op}");
            n += 1;
        }
        // On-disk tree, including file modes.
        let mut tree = serde_json::Map::new();
        fn walk(p: &std::path::Path, rel: &str, out: &mut serde_json::Map<String, Value>) {
            let mut names: Vec<_> = std::fs::read_dir(p).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
            names.sort();
            for name in names {
                let f = p.join(&name);
                let md = std::fs::metadata(&f).unwrap();
                if md.is_dir() {
                    walk(&f, &format!("{rel}{name}/"), out);
                } else {
                    use std::os::unix::fs::PermissionsExt;
                    out.insert(format!("{rel}{name}"), json!({"text": std::fs::read_to_string(&f).unwrap(), "mode": format!("{:o}", md.permissions().mode() & 0o777)}));
                }
            }
        }
        walk(&dir, "", &mut tree);
        assert_eq!(s(&Value::Object(tree)), s(&sc["tree"]), "tree for {}", sc["ops"]);
    }
    assert!(n > 400, "{n}");
}

#[test]
fn virtual_project_path_matches_ts() {
    let f: Value = serde_json::from_str(include_str!("fixtures/settings_files.json")).unwrap();
    let mut n = 0;
    for c in f["virtual"].as_array().unwrap() {
        let env: Env = c["env"].as_object().unwrap().iter().map(|(k, v)| (k.clone(), v.as_str().unwrap().to_string())).collect();
        let got = is_virtual_project_path(c["r"].as_str().unwrap(), "/h", &env, c["cwd"].as_str().unwrap());
        assert_eq!(Value::Bool(got), c["out"], "{c}");
        n += 1;
    }
    assert_eq!(n, 50);
}
