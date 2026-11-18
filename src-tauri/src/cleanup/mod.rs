//! Limpeza de arquivos temporários e caches.
//!
//! - `fsops`: varredura/limpeza genérica de pastas (testável em qualquer SO).
//! - `windows`: onde fica cada pasta, Lixeira via API do shell e espaço livre do disco.
//! - aqui: orquestração (threads, relatório, progresso).

mod fsops;
#[cfg(windows)]
mod windows;

use std::collections::HashSet;
use std::time::{Duration, SystemTime};

use serde::{Deserialize, Serialize};

/// Reaproveitado pelas Correções (limpar o cache do Windows Update).
pub(crate) use fsops::clean_dir;
use fsops::scan_dir;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CleanupCategory {
    UserTemp,
    WindowsTemp,
    WindowsUpdateCache,
    ShaderCache,
    RecycleBin,
    /// Cache do Chrome, Edge, Brave e Firefox (só com o navegador fechado).
    BrowserCache,
    /// Relatórios de travamento de apps (CrashDumps e WER do usuário).
    CrashDumps,
    /// Minidumps de tela azul (pasta do Windows).
    SystemDumps,
    /// Cache de miniaturas e ícones do Explorador.
    ThumbnailCache,
}

impl CleanupCategory {
    pub const ALL: [CleanupCategory; 9] = [
        CleanupCategory::UserTemp,
        CleanupCategory::WindowsTemp,
        CleanupCategory::WindowsUpdateCache,
        CleanupCategory::ShaderCache,
        CleanupCategory::RecycleBin,
        CleanupCategory::BrowserCache,
        CleanupCategory::CrashDumps,
        CleanupCategory::SystemDumps,
        CleanupCategory::ThumbnailCache,
    ];

    /// Pastas do sistema: sem admin a leitura/remoção é negada.
    pub const fn requires_admin(self) -> bool {
        matches!(self, CleanupCategory::WindowsTemp | CleanupCategory::WindowsUpdateCache | CleanupCategory::SystemDumps)
    }

    /// Arquivos mais novos que isso ficam. Instaladores e apps abertos usam o TEMP
    /// enquanto rodam; 24 h é a mesma margem que a Limpeza de Disco do Windows usa
    /// por padrão para arquivos temporários.
    pub const fn min_age(self) -> Option<Duration> {
        match self {
            CleanupCategory::UserTemp | CleanupCategory::WindowsTemp => Some(Duration::from_secs(24 * 3600)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryScan {
    pub category: CleanupCategory,
    /// Caminho real (já expandido): o front mostra pro usuário saber o que será apagado.
    pub path: String,
    pub bytes: u64,
    pub files: u64,
    /// false = pasta não existe neste PC (ex.: sem cache de shaders ainda).
    pub available: bool,
    /// true = parte da pasta não pôde ser lida; o tamanho real pode ser maior.
    pub partial: bool,
    pub requires_admin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryResult {
    pub category: CleanupCategory,
    pub freed_bytes: u64,
    pub deleted_files: u64,
    pub skipped_files: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CleanupReport {
    pub results: Vec<CategoryResult>,
    /// Soma do tamanho dos arquivos efetivamente apagados.
    pub freed_bytes: u64,
    /// Diferença de espaço livre no disco do sistema, antes x depois. Pode diferir de
    /// `freed_bytes`: tamanho em disco ≠ tamanho do arquivo (clusters, compressão NTFS,
    /// hard links) e outros programas escrevem no disco ao mesmo tempo.
    pub disk_freed_bytes: Option<u64>,
}

// ---------------- Plataforma ----------------

#[cfg(windows)]
use windows::{blocked_by, folders, recycle_bin_empty, recycle_bin_query, system_drive_free};

#[cfg(not(windows))]
fn folders(_c: CleanupCategory) -> Vec<std::path::PathBuf> {
    Vec::new()
}
#[cfg(not(windows))]
fn blocked_by(_c: CleanupCategory) -> Option<&'static str> {
    None
}
#[cfg(not(windows))]
fn recycle_bin_query() -> crate::error::AppResult<(u64, u64)> {
    Err(crate::error::AppError::Unsupported)
}
#[cfg(not(windows))]
fn recycle_bin_empty() -> crate::error::AppResult<()> {
    Err(crate::error::AppError::Unsupported)
}
#[cfg(not(windows))]
fn system_drive_free() -> Option<u64> {
    None
}

// ---------------- Varredura ----------------

fn scan_one(category: CleanupCategory) -> CategoryScan {
    let mut scan = CategoryScan {
        category,
        path: String::new(),
        bytes: 0,
        files: 0,
        available: false,
        partial: false,
        requires_admin: category.requires_admin(),
    };

    if category == CleanupCategory::RecycleBin {
        scan.path = "Lixeira (todas as unidades)".into();
        if let Ok((bytes, items)) = recycle_bin_query() {
            scan.bytes = bytes;
            scan.files = items;
            scan.available = true;
        }
        return scan;
    }

    let dirs: Vec<_> = folders(category).into_iter().filter(|d| d.is_dir()).collect();
    scan.path = match dirs.as_slice() {
        [] => return scan,
        [one] => one.display().to_string(),
        many => format!("{} pastas (ex.: {})", many.len(), many[0].display()),
    };
    for dir in &dirs {
        let stats = scan_dir(dir);
        scan.bytes += stats.bytes;
        scan.files += stats.files;
        scan.partial |= stats.unreadable > 0;
    }
    scan.available = true;
    scan
}

/// Varre todas as categorias em paralelo, uma thread por categoria.
///
/// `thread::scope` garante que todas as threads terminam antes da função retornar,
/// então elas podem usar dados da pilha sem `Arc`. Aqui o ganho é real porque o
/// trabalho é esperar o disco (I/O), não CPU: enquanto uma thread espera a leitura
/// do %TEMP%, outra já está listando o cache do Windows Update.
pub fn scan() -> Vec<CategoryScan> {
    std::thread::scope(|s| {
        let handles: Vec<_> = CleanupCategory::ALL
            .into_iter()
            .map(|c| (c, s.spawn(move || scan_one(c))))
            .collect();

        handles
            .into_iter()
            .map(|(c, h)| {
                // Se uma thread entrar em pânico, as outras categorias continuam valendo.
                h.join().unwrap_or_else(|_| CategoryScan {
                    category: c,
                    path: String::new(),
                    bytes: 0,
                    files: 0,
                    available: false,
                    partial: true,
                    requires_admin: c.requires_admin(),
                })
            })
            .collect()
    })
}

// ---------------- Limpeza ----------------

pub fn clean_one(category: CleanupCategory) -> CategoryResult {
    let mut result = CategoryResult { category, freed_bytes: 0, deleted_files: 0, skipped_files: 0, error: None };

    if category == CleanupCategory::RecycleBin {
        // A Lixeira não é uma pasta comum (uma subpasta por usuário em cada unidade, com
        // metadados próprios). O jeito certo é pedir pro shell esvaziar.
        match recycle_bin_query() {
            Ok((0, _)) => {}
            Ok((bytes, items)) => match recycle_bin_empty() {
                Ok(()) => {
                    result.freed_bytes = bytes;
                    result.deleted_files = items;
                }
                Err(e) => result.error = Some(e.to_string()),
            },
            Err(e) => result.error = Some(e.to_string()),
        }
        return result;
    }

    if let Some(app) = blocked_by(category) {
        result.error = Some(format!("Feche o {app} antes: com ele aberto o cache está em uso."));
        return result;
    }
    let keep_newer_than = category.min_age().and_then(|age| SystemTime::now().checked_sub(age));
    for dir in folders(category).into_iter().filter(|d| d.is_dir()) {
        let stats = clean_dir(&dir, keep_newer_than);
        result.freed_bytes += stats.freed_bytes;
        result.deleted_files += stats.deleted_files;
        result.skipped_files += stats.skipped_files;
    }
    result
}

/// Limpa as categorias pedidas, uma por vez, chamando `on_progress` ao fim de cada uma.
///
/// Sequencial de propósito: apagar várias pastas ao mesmo tempo no mesmo disco não fica
/// mais rápido (o gargalo é o disco) e deixaria o progresso confuso pro usuário.
pub fn clean(categories: &[CleanupCategory], on_progress: impl FnMut(&CategoryResult)) -> CleanupReport {
    clean_with(categories, clean_one, on_progress)
}

/// Igual a `clean`, mas quem limpa cada categoria é `run_one`. Permite mandar as
/// categorias que exigem admin pro processo auxiliar elevado.
pub fn clean_with(
    categories: &[CleanupCategory],
    mut run_one: impl FnMut(CleanupCategory) -> CategoryResult,
    mut on_progress: impl FnMut(&CategoryResult),
) -> CleanupReport {
    let before = system_drive_free();

    let mut seen = HashSet::new();
    let results: Vec<CategoryResult> = categories
        .iter()
        .copied()
        .filter(|c| seen.insert(*c)) // ignora categoria repetida
        .map(|c| {
            let r = run_one(c);
            on_progress(&r);
            r
        })
        .collect();

    let after = system_drive_free();
    CleanupReport {
        freed_bytes: results.iter().map(|r| r.freed_bytes).sum(),
        disk_freed_bytes: match (before, after) {
            (Some(b), Some(a)) => Some(a.saturating_sub(b)),
            _ => None,
        },
        results,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializacao_para_o_front() {
        let c: CleanupCategory = serde_json::from_str("\"shaderCache\"").unwrap();
        assert_eq!(c, CleanupCategory::ShaderCache);
        let r = CategoryResult { category: c, freed_bytes: 1, deleted_files: 1, skipped_files: 0, error: None };
        let v = serde_json::to_value(&r).unwrap();
        assert_eq!(v["freedBytes"], 1);
        assert_eq!(v["category"], "shaderCache");
    }

    #[test]
    fn categorias_repetidas_rodam_uma_vez() {
        let mut calls = 0;
        clean(&[CleanupCategory::ShaderCache, CleanupCategory::ShaderCache], |_| calls += 1);
        assert_eq!(calls, 1);
    }

    #[test]
    fn varredura_devolve_todas_as_categorias() {
        let s = scan();
        assert_eq!(s.len(), CleanupCategory::ALL.len());
        for (scan, cat) in s.iter().zip(CleanupCategory::ALL) {
            assert_eq!(scan.category, cat);
        }
    }
}
