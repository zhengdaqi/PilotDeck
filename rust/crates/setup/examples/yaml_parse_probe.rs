fn main() {
    for line in std::io::stdin().lines() {
        let text: String = serde_json::from_str(&line.unwrap()).unwrap();
        let r = match pilotdeck_setup::yamlfmt::parse(&text) {
            Ok(v) => format!("OK {}", pilotdeck_skills::jsjson::stringify_compact(&v)),
            Err(_) => "ERR".to_string(),
        };
        println!("{}", serde_json::to_string(&r).unwrap());
    }
}
