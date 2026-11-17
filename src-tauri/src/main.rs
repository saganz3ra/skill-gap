// Em release, não abre uma janela de console junto com o app no Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // O mesmo executável vira o auxiliar elevado quando aberto com esta flag (via UAC).
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--elevated-helper") {
        std::process::exit(skill_gap_lib::run_elevated_helper(&args[2..]));
    }
    skill_gap_lib::run()
}
