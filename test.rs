fn main() { let s = "2026-02-23T07:05"; let dt = chrono::DateTime::parse_from_rfc3339(&format!("{}:00Z", s)); println!("{:?}", dt); }
