#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
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
