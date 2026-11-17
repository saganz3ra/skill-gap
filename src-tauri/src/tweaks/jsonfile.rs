//! Ler/gravar UMA chave de primeiro nível num JSON de configurações de app, sem
//! estragar o resto: mantém a ordem das chaves, grava em UTF-8 sem BOM e de forma
//! atômica (temporário + rename), pra o app dono do arquivo nunca ler um arquivo pela metade.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::error::{AppError, AppResult};

/// `%APPDATA%` + `discord\settings.json`. Só variáveis de pasta do usuário.
pub fn resolve(env: &str, file: &str) -> Option<PathBuf> {
    if !matches!(env, "APPDATA" | "LOCALAPPDATA") || file.contains("..") || Path::new(file).is_absolute() {
        return None;
    }
    std::env::var_os(env).map(|base| PathBuf::from(base).join(file))
}

fn read_object(path: &Path) -> AppResult<Map<String, Value>> {
    let bytes = fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let text = text.trim_start_matches('\u{feff}');
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(m)) => Ok(m),
        Ok(_) => Err(AppError::InvalidInput("o arquivo de configurações não é um objeto JSON".into())),
        Err(e) => Err(AppError::InvalidInput(format!("o arquivo de configurações está corrompido: {e}"))),
    }
}

pub fn get(path: &Path, key: &str) -> AppResult<Option<String>> {
    Ok(read_object(path)?.get(key).map(Value::to_string))
}

/// `value`: literal JSON (`false`, `"texto"`); `None` remove a chave.
pub fn set(path: &Path, key: &str, value: Option<&str>) -> AppResult<()> {
    let mut map = read_object(path)?;
    match value {
        Some(v) => {
            let parsed: Value = serde_json::from_str(v).map_err(|e| AppError::InvalidInput(format!("valor JSON inválido: {e}")))?;
            map.insert(key.to_string(), parsed);
        }
        None => {
            map.remove(key);
        }
    }
    let tmp = path.with_extension("json.skillgap-tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&Value::Object(map)).expect("serializável"))?;
    fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mexe_so_na_chave_e_preserva_o_resto() {
        let dir = std::env::temp_dir().join(format!("sg-json-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        // Com BOM, como alguns apps gravam.
        fs::write(&path, "\u{feff}{\"zeta\": 1, \"OPEN_ON_STARTUP\": true, \"alpha\": {\"x\": [1,2]}}").unwrap();

        assert_eq!(get(&path, "OPEN_ON_STARTUP").unwrap().as_deref(), Some("true"));
        set(&path, "OPEN_ON_STARTUP", Some("false")).unwrap();
        set(&path, "NOVA", Some("\"x\"")).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(!text.starts_with('\u{feff}'), "grava sem BOM");
        assert!(text.find("zeta").unwrap() < text.find("alpha").unwrap(), "ordem das chaves mantida");
        assert_eq!(get(&path, "OPEN_ON_STARTUP").unwrap().as_deref(), Some("false"));
        set(&path, "NOVA", None).unwrap();
        assert_eq!(get(&path, "NOVA").unwrap(), None);
        assert_eq!(get(&path, "alpha").unwrap().as_deref(), Some(r#"{"x":[1,2]}"#));

        fs::write(&path, "{ quebrado").unwrap();
        assert!(set(&path, "a", Some("1")).is_err(), "arquivo corrompido não é sobrescrito");
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ quebrado");
        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn so_pastas_do_usuario() {
        assert!(resolve("SystemRoot", "x.json").is_none());
        assert!(resolve("APPDATA", r"..\..\Windows\x.json").is_none());
    }
}
