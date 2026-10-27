#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|arg| arg == "--launch-smoke") {
        let position = args
            .iter()
            .position(|arg| arg == "--launch-smoke")
            .unwrap_or(0);
        let shared = args.get(position + 1).cloned().unwrap_or_default();
        let version = args.get(position + 2).cloned().unwrap_or_default();
        let java = args.get(position + 3).cloned().unwrap_or_default();
        let seconds = args
            .get(position + 4)
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(20);
        match aurora_launcher_lib::smoke_launch(&shared, &version, &java, "AuroraTest", seconds) {
            Ok(log) => println!("{log}"),
            Err(error) => {
                eprintln!("launch smoke failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    if std::env::args().any(|arg| arg == "--self-test") {
        match aurora_launcher_lib::self_test() {
            Ok(report) => println!("{report}"),
            Err(error) => {
                eprintln!("self-test failed: {error}");
                std::process::exit(1);
            }
        }
        return;
    }
    aurora_launcher_lib::run();
}
