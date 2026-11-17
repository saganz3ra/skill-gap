//! "Isto é um jogo?" só pelo nome e pelo CAMINHO do executável.
//!
//! O caminho vem de `NtQuerySystemInformation(SystemProcessIdInformation)`, que devolve
//! o arquivo de imagem de um PID SEM abrir handle nenhum para o processo (regra do
//! projeto: nunca abrir o processo do jogo, por causa dos anticheats).

/// Pastas padrão das lojas e de alguns jogos sem loja.
const GAME_FOLDERS: &[&str] = &[
    r"\steamapps\common\",
    r"\epic games\",
    r"\riot games\",
    r"\xboxgames\",
    r"\ea games\",
    r"\ubisoft game launcher\games\",
    r"\gog galaxy\games\",
    r"\gog games\",
    r"\battlestate games\",
    r"\rockstar games\",
    r"\call of duty\",
];

/// Jogos populares que costumam ficar fora dessas pastas.
const KNOWN_GAMES: &[&str] = &[
    "cs2.exe",
    "valorant-win64-shipping.exe",
    "r5apex.exe",
    "r5apex_dx12.exe",
    "fortniteclient-win64-shipping.exe",
    "league of legends.exe",
    "overwatch.exe",
    "gta5.exe",
    "gta5_enhanced.exe",
    "cod.exe",
    "rocketleague.exe",
    "rainbowsix.exe",
    "eldenring.exe",
];

/// Nunca é o jogo: launchers, auxiliares, instaladores, Java.
const IGNORED_EXACT: &[&str] = &[
    "steam.exe",
    "steamwebhelper.exe",
    "steamservice.exe",
    "epicgameslauncher.exe",
    "epicwebhelper.exe",
    "riotclientservices.exe",
    "riotclientux.exe",
    "riotclientuxrender.exe",
    "eadesktop.exe",
    "eabackgroundservice.exe",
    "upc.exe",
    "ubisoftconnect.exe",
    "galaxyclient.exe",
    "battle.net.exe",
    "java.exe",
    "javaw.exe",
];

const IGNORED_PARTS: &[&str] = &[
    "setup", "install", "unins", "redist", "prereq", "crash", "report", "launcher", "update", "helper", "anticheat",
    "easyanticheat", "battleye", "beservice", "cefprocess", "webview",
];

pub fn is_ignored(exe: &str) -> bool {
    let e = exe.to_ascii_lowercase();
    IGNORED_EXACT.contains(&e.as_str()) || IGNORED_PARTS.iter().any(|p| e.contains(p)) || super::watcher::is_protected(&e)
}

pub fn looks_like_game(exe: &str, path: Option<&str>) -> bool {
    if is_ignored(exe) {
        return false;
    }
    let e = exe.to_ascii_lowercase();
    if KNOWN_GAMES.contains(&e.as_str()) {
        return true;
    }
    path.is_some_and(|p| {
        let p = p.to_ascii_lowercase();
        GAME_FOLDERS.iter().any(|f| p.contains(f))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn por_pasta_e_por_nome() {
        assert!(looks_like_game("Hades.exe", Some(r"\Device\HarddiskVolume3\SteamLibrary\steamapps\common\Hades\x64\Hades.exe")));
        assert!(looks_like_game("cs2.exe", None));
        assert!(!looks_like_game("notepad.exe", Some(r"C:\Windows\notepad.exe")));
    }

    #[test]
    fn ignora_launcher_instalador_anticheat_e_java() {
        let steam = Some(r"C:\Program Files (x86)\Steam\steamapps\common\Game\x.exe");
        for exe in ["steamwebhelper.exe", "UnityCrashHandler64.exe", "UE4PrereqSetup_x64.exe", "EasyAntiCheat_EOS.exe", "javaw.exe", "vgc.exe"] {
            assert!(!looks_like_game(exe, steam), "{exe}");
        }
    }
}
