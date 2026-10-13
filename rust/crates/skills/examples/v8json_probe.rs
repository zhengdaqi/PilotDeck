fn main() {
    for line in std::io::stdin().lines() {
        let line = line.unwrap();
        let input: String = serde_json::from_str(&line).unwrap();
        match pilotdeck_skills::v8json::parse(&input) {
            Ok(v) => println!("{}", serde_json::to_string(&format!("OK {}", pilotdeck_skills::jsjson::stringify_compact(&v))).unwrap()),
            Err(e) => println!("{}", serde_json::to_string(&format!("ERR {e}")).unwrap()),
        }
    }
}
