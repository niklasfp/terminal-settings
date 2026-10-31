// Links Rio's own config code (rio-backend) to answer two questions about Rio's config:
//
//   schema-dump                 print Config::default() as JSON (input for the editor's schema)
//   schema-dump --toml          the same, as TOML
//   schema-dump validate FILE   parse FILE exactly like Rio does; print the error and exit 1 if Rio rejects it
fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.get(1).map(String::as_str) == Some("validate") {
        let path = args.get(2).expect("usage: schema-dump validate <config.toml>");
        let text = std::fs::read_to_string(path).expect("read config");
        match toml::from_str::<rio_backend::config::Config>(&text) {
            Ok(_) => return,
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }

    let cfg = rio_backend::config::Config::default();
    if args.iter().any(|a| a == "--toml") {
        println!("{}", toml::to_string_pretty(&cfg).unwrap());
    } else {
        println!("{}", serde_json::to_string_pretty(&cfg).unwrap());
    }
}
