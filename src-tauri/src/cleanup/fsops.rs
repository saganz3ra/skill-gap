//! Varredura e limpeza de pastas. Código 100% `std`, sem nada específico do Windows
//! além da checagem de reparse point, então dá pra testar em qualquer sistema.
//!
//! Regras de segurança (as mais importantes do app inteiro):
//! 1. Nunca seguir links simbólicos ou junctions. Um junction dentro do %TEMP% apontando
//!    para C:\Users seria seguido por um "apaga tudo recursivo" ingênuo.
//! 2. Nunca apagar a pasta raiz, só o conteúdo.
//! 3. Arquivo em uso ou sem permissão é pulado e contado, nunca interrompe a limpeza.
//! 4. "Espaço liberado" = soma do tamanho dos arquivos que REALMENTE foram apagados.

use std::fs::{self, Metadata};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirStats {
    pub bytes: u64,
    pub files: u64,
    /// Pastas que não puderam ser lidas (permissão negada).
    pub unreadable: u64,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CleanStats {
    pub freed_bytes: u64,
    pub deleted_files: u64,
    /// Em uso, sem permissão ou mais novos que o limite de idade.
    pub skipped_files: u64,
}

/// Link simbólico ou junction/reparse point: não seguimos nem apagamos.
pub fn is_link(md: &Metadata) -> bool {
    if md.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        if md.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    false
}

/// Percorre a árvore sem recursão (pilha explícita: pastas muito profundas não
/// estouram a pilha da thread). Chama `on_file` para cada arquivo e devolve as
/// subpastas visitadas, da mais rasa para a mais funda.
fn walk(root: &Path, unreadable: &mut u64, mut on_file: impl FnMut(&Path, &Metadata)) -> Vec<PathBuf> {
    let mut stack = vec![root.to_path_buf()];
    let mut dirs = Vec::new();

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => {
                *unreadable += 1;
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            // symlink_metadata NÃO segue links: é isso que garante a regra 1.
            let Ok(md) = fs::symlink_metadata(&path) else { continue };
            if is_link(&md) {
                continue;
            }
            if md.is_dir() {
                dirs.push(path.clone());
                stack.push(path);
            } else if md.is_file() {
                on_file(&path, &md);
            }
        }
    }
    dirs
}

pub fn scan_dir(root: &Path) -> DirStats {
    let mut stats = DirStats::default();
    let mut unreadable = 0;
    walk(root, &mut unreadable, |_, md| {
        stats.bytes += md.len();
        stats.files += 1;
    });
    stats.unreadable = unreadable;
    stats
}

fn remove_file(path: &Path, md: &Metadata) -> io::Result<()> {
    match fs::remove_file(path) {
        // No Windows, arquivo somente leitura não pode ser apagado: tira o atributo e tenta de novo.
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied && md.permissions().readonly() => {
            let mut perms = md.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            fs::set_permissions(path, perms)?;
            fs::remove_file(path)
        }
        other => other,
    }
}

/// Apaga o conteúdo de `root`. `keep_newer_than`: arquivos modificados depois desse
/// instante são preservados (instaladores em andamento costumam usar o %TEMP%).
pub fn clean_dir(root: &Path, keep_newer_than: Option<SystemTime>) -> CleanStats {
    let mut stats = CleanStats::default();
    let mut unreadable = 0;

    let dirs = walk(root, &mut unreadable, |path, md| {
        let too_new = match (keep_newer_than, md.modified()) {
            (Some(limit), Ok(modified)) => modified > limit,
            _ => false,
        };
        if too_new {
            stats.skipped_files += 1;
            return;
        }
        match remove_file(path, md) {
            Ok(()) => {
                stats.freed_bytes += md.len();
                stats.deleted_files += 1;
            }
            // Em uso (ERROR_SHARING_VIOLATION) ou acesso negado: pula e segue.
            Err(_) => stats.skipped_files += 1,
        }
    });

    // Remove pastas que ficaram vazias, da mais funda pra mais rasa.
    // remove_dir só apaga pasta vazia, então uma pasta com arquivo pulado continua lá.
    for dir in dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }

    stats
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use std::time::{Duration, UNIX_EPOCH};

    struct TempDir(PathBuf);
    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
            let p = std::env::temp_dir().join(format!("skillgap-test-{name}-{nanos}"));
            fs::create_dir_all(&p).unwrap();
            TempDir(p)
        }
    }
    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn write(path: &Path, bytes: usize) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        File::create(path).unwrap().write_all(&vec![b'x'; bytes]).unwrap();
    }

    fn set_old(path: &Path) {
        let old = SystemTime::now() - Duration::from_secs(3 * 24 * 3600);
        File::options().write(true).open(path).unwrap().set_modified(old).unwrap();
    }

    #[test]
    fn varredura_soma_arquivos_aninhados() {
        let t = TempDir::new("scan");
        write(&t.0.join("a.tmp"), 100);
        write(&t.0.join("sub/b.tmp"), 50);
        write(&t.0.join("sub/deep/c.tmp"), 25);
        let s = scan_dir(&t.0);
        assert_eq!(s.files, 3);
        assert_eq!(s.bytes, 175);
    }

    #[test]
    fn pasta_inexistente_nao_quebra() {
        let s = scan_dir(Path::new("/caminho/que/nao/existe/skillgap"));
        assert_eq!((s.files, s.bytes, s.unreadable), (0, 0, 1));
    }

    #[test]
    fn limpeza_apaga_conteudo_e_preserva_raiz() {
        let t = TempDir::new("clean");
        write(&t.0.join("a.tmp"), 100);
        write(&t.0.join("sub/deep/b.tmp"), 40);
        let s = clean_dir(&t.0, None);
        assert_eq!(s.deleted_files, 2);
        assert_eq!(s.freed_bytes, 140);
        assert!(t.0.exists(), "a pasta raiz nunca pode ser apagada");
        assert_eq!(fs::read_dir(&t.0).unwrap().count(), 0, "subpastas vazias devem sumir");
    }

    #[test]
    fn arquivos_recentes_sao_preservados() {
        let t = TempDir::new("age");
        let old = t.0.join("velho.tmp");
        let new = t.0.join("novo.tmp");
        write(&old, 10);
        write(&new, 10);
        set_old(&old);
        let limit = SystemTime::now() - Duration::from_secs(24 * 3600);
        let s = clean_dir(&t.0, Some(limit));
        assert_eq!((s.deleted_files, s.skipped_files, s.freed_bytes), (1, 1, 10));
        assert!(!old.exists());
        assert!(new.exists());
    }

    #[test]
    fn pasta_com_arquivo_pulado_nao_e_removida() {
        let t = TempDir::new("keepdir");
        let new = t.0.join("sub/novo.tmp");
        write(&new, 5);
        let limit = SystemTime::now() - Duration::from_secs(3600);
        clean_dir(&t.0, Some(limit));
        assert!(new.exists());
    }

    #[cfg(unix)]
    #[test]
    fn nunca_segue_link_simbolico() {
        let alvo = TempDir::new("alvo");
        let importante = alvo.0.join("importante.txt");
        write(&importante, 10);

        let t = TempDir::new("link");
        write(&t.0.join("lixo.tmp"), 10);
        std::os::unix::fs::symlink(&alvo.0, t.0.join("atalho")).unwrap();

        let scan = scan_dir(&t.0);
        assert_eq!(scan.files, 1, "o conteúdo do link não entra na conta");

        clean_dir(&t.0, None);
        assert!(importante.exists(), "arquivo fora da pasta alvo foi apagado através do link!");
    }
}
