//! Configurações do app (aba Configurações): abrir minimizado ao iniciar com o
//! Windows e atalhos globais. O "iniciar com o Windows" em si mora no Windows
//! (plugin autostart), não aqui: assim não dá pra ficar dessincronizado.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// O que um atalho global pode fazer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutAction {
    /// Mostra e põe na frente a janela do Skill Gap.
    OpenApp,
    /// Liga/desliga o overlay.
    ToggleOverlay,
    /// Liga/desliga o Modo Jogo automático.
    ToggleGameMode,
    /// Liga/desliga a segunda tela (celular).
    ToggleRemote,
    /// Abre o app numa tela específica.
    OpenMonitor,
    OpenCleanup,
    OpenTweaks,
    OpenFixes,
}

impl ShortcutAction {
    #[cfg(test)]
    pub const ALL: [ShortcutAction; 8] = [
        Self::OpenApp,
        Self::ToggleOverlay,
        Self::ToggleGameMode,
        Self::ToggleRemote,
        Self::OpenMonitor,
        Self::OpenCleanup,
        Self::OpenTweaks,
        Self::OpenFixes,
    ];

    /// Tela que a ação abre (rota do front), quando é uma ação de "abrir".
    pub fn route(self) -> Option<&'static str> {
        match self {
            Self::OpenApp => Some(""),
            Self::OpenMonitor => Some("/monitor"),
            Self::OpenCleanup => Some("/limpeza"),
            Self::OpenTweaks => Some("/ajustes"),
            Self::OpenFixes => Some("/correcoes"),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Ao abrir junto com o Windows, começar minimizado (o Modo Jogo segue vigiando).
    pub start_minimized: bool,
    /// Ação → atalho no formato do Tauri ("Ctrl+Alt+O").
    pub shortcuts: BTreeMap<ShortcutAction, String>,
}

const MODIFIERS: [&str; 4] = ["Ctrl", "Alt", "Shift", "Super"];

fn key_ok(k: &str) -> bool {
    let b = k.as_bytes();
    match b.len() {
        1 => b[0].is_ascii_uppercase() || b[0].is_ascii_digit(),
        2..=3 if b[0] == b'F' => k[1..].parse::<u8>().is_ok_and(|n| (1..=24).contains(&n)),
        _ => matches!(k, "Space" | "Home" | "End" | "PageUp" | "PageDown" | "Insert" | "Delete"),
    }
}

/// Atalho aceito: 1+ modificadores na ordem Ctrl, Alt, Shift, Super, e UMA tecla
/// (letra, número, F1–F24 ou tecla de navegação). Sem modificador não: uma tecla
/// solta seria "roubada" de todos os programas, inclusive do jogo.
pub fn normalize(accel: &str) -> AppResult<String> {
    let parts: Vec<&str> = accel.split('+').map(str::trim).filter(|p| !p.is_empty()).collect();
    let bad = || AppError::InvalidInput(format!("atalho inválido: {accel}"));
    let (key, mods) = parts.split_last().ok_or_else(bad)?;
    if mods.is_empty() {
        return Err(AppError::InvalidInput("use ao menos Ctrl, Alt, Shift ou Win junto da tecla".into()));
    }
    let mut seen = Vec::new();
    for m in mods {
        let m = match m.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => "Ctrl",
            "alt" => "Alt",
            "shift" => "Shift",
            "super" | "win" | "meta" => "Super",
            _ => return Err(bad()),
        };
        if seen.contains(&m) {
            return Err(bad());
        }
        seen.push(m);
    }
    let key = key.to_ascii_uppercase();
    let key = match key.as_str() {
        "SPACE" => "Space".to_string(),
        "HOME" => "Home".into(),
        "END" => "End".into(),
        "PAGEUP" => "PageUp".into(),
        "PAGEDOWN" => "PageDown".into(),
        "INSERT" => "Insert".into(),
        "DELETE" => "Delete".into(),
        _ => key,
    };
    if !key_ok(&key) {
        return Err(AppError::InvalidInput(format!("tecla não suportada em atalho: {key}")));
    }
    let mut out: Vec<&str> = MODIFIERS.iter().copied().filter(|m| seen.contains(m)).collect();
    out.push(&key);
    Ok(out.join("+"))
}

pub struct SettingsStore {
    path: PathBuf,
    current: Mutex<Settings>,
}

impl SettingsStore {
    /// Arquivo corrompido não impede o app de abrir: começa com o padrão.
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("configuracoes.json");
        let current = fs::read_to_string(&path).ok().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Self { path, current: Mutex::new(current) }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().map(|s| s.clone()).unwrap_or_default()
    }

    /// Muda e grava (temporário + rename). Se gravar falhar, nada muda na memória.
    pub fn update(&self, f: impl FnOnce(&mut Settings) -> AppResult<()>) -> AppResult<Settings> {
        let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let mut next = guard.clone();
        f(&mut next)?;
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(&next).expect("configurações serializáveis"))?;
        fs::rename(&tmp, &self.path)?;
        *guard = next.clone();
        Ok(next)
    }

    /// Define (ou remove, com `None`) o atalho de uma ação. Um atalho só pode ter
    /// uma ação: repetir o de outra ação é recusado com o nome dela.
    pub fn set_shortcut(&self, action: ShortcutAction, accel: Option<&str>) -> AppResult<Settings> {
        let accel = accel.map(normalize).transpose()?;
        self.update(|s| {
            if let Some(a) = &accel {
                if let Some((other, _)) = s.shortcuts.iter().find(|(k, v)| **k != action && *v == a) {
                    return Err(AppError::InvalidInput(format!("{a} já está em uso por outra ação ({other:?})")));
                }
                s.shortcuts.insert(action, a.clone());
            } else {
                s.shortcuts.remove(&action);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_orders_modifiers_and_rejects_bare_keys() {
        assert_eq!(normalize("alt+ctrl+o").unwrap(), "Ctrl+Alt+O");
        assert_eq!(normalize("Win + Shift + f9").unwrap(), "Shift+Super+F9");
        assert_eq!(normalize("Ctrl+Alt+space").unwrap(), "Ctrl+Alt+Space");
        assert_eq!(normalize("Ctrl+1").unwrap(), "Ctrl+1");
        assert!(normalize("O").is_err(), "tecla solta roubaria a tecla do jogo");
        assert!(normalize("F5").is_err());
        assert!(normalize("Ctrl+Ctrl+O").is_err());
        assert!(normalize("Ctrl+Alt").is_err(), "falta a tecla");
        assert!(normalize("Ctrl+Hyper+O").is_err());
        assert!(normalize("Ctrl+F25").is_err());
        assert!(normalize("Ctrl+Tab").is_err());
        assert!(normalize("").is_err());
    }

    #[test]
    fn store_persists_and_refuses_duplicates() {
        let dir = std::env::temp_dir().join(format!("sg-settings-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let st = SettingsStore::load(&dir);
        assert_eq!(st.get(), Settings::default());

        st.set_shortcut(ShortcutAction::ToggleOverlay, Some("ctrl+alt+o")).unwrap();
        let err = st.set_shortcut(ShortcutAction::OpenApp, Some("Ctrl+Alt+O")).unwrap_err();
        assert!(err.to_string().contains("já está em uso"));
        // A mesma ação pode receber o mesmo atalho de novo.
        st.set_shortcut(ShortcutAction::ToggleOverlay, Some("Alt+Ctrl+O")).unwrap();
        st.update(|s| {
            s.start_minimized = true;
            Ok(())
        })
        .unwrap();

        let reopened = SettingsStore::load(&dir);
        assert_eq!(reopened.get().shortcuts.get(&ShortcutAction::ToggleOverlay).map(String::as_str), Some("Ctrl+Alt+O"));
        assert!(reopened.get().start_minimized);

        reopened.set_shortcut(ShortcutAction::ToggleOverlay, None).unwrap();
        assert!(reopened.get().shortcuts.is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_file_starts_with_defaults() {
        let dir = std::env::temp_dir().join(format!("sg-settings-bad-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("configuracoes.json"), "{ não é json").unwrap();
        assert_eq!(SettingsStore::load(&dir).get(), Settings::default());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_action_serializes_and_open_actions_have_routes() {
        for a in ShortcutAction::ALL {
            let s = serde_json::to_string(&a).unwrap();
            assert_eq!(serde_json::from_str::<ShortcutAction>(&s).unwrap(), a);
        }
        assert_eq!(ShortcutAction::OpenMonitor.route(), Some("/monitor"));
        assert_eq!(ShortcutAction::ToggleOverlay.route(), None);
    }
}
