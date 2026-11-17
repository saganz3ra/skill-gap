//! Quanto o boot e o login demoram, e quem atrasa.
//!
//! Fonte: o log "Diagnostics-Performance" do próprio Windows. O evento 100 registra a
//! duração de cada boot; os eventos 101 (aplicativo) e 103 (serviço) registram quem
//! demorou mais que o normal pra iniciar e quanto isso atrasou o boot.
//! Ler esse log exige administrador.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootTime {
    pub at: String,
    /// Do ligar até a área de trabalho pronta.
    pub total_ms: u64,
    /// Até a tela de login/área de trabalho aparecer.
    pub main_ms: u64,
    /// Depois do login, até o PC ficar utilizável: é o "atraso no login".
    pub post_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlowItem {
    /// Nome do executável/serviço, ex.: OneDrive.exe
    pub name: String,
    pub friendly: String,
    /// "app" ou "service"
    pub kind: &'static str,
    pub total_ms: u64,
    /// Quanto atrasou além do normal pra ele.
    pub degradation_ms: u64,
    pub at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootReport {
    /// Mais recente primeiro.
    pub boots: Vec<BootTime>,
    /// Um por nome (o mais recente), do que mais atrasa pro que menos atrasa.
    pub slow: Vec<SlowItem>,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
struct RawEvent {
    id: u32,
    time: String,
    data: HashMap<String, Option<String>>,
}

fn num(data: &HashMap<String, Option<String>>, key: &str) -> u64 {
    data.get(key).and_then(|v| v.as_deref()).and_then(|v| v.trim().parse().ok()).unwrap_or(0)
}

fn text(data: &HashMap<String, Option<String>>, key: &str) -> String {
    data.get(key).and_then(|v| v.clone()).unwrap_or_default().trim().to_string()
}

/// Marcador que o script devolve quando o log existe mas o acesso foi negado.
pub const ADMIN_MARKER: &str = "\"__admin__\"";

pub fn parse(json: &str) -> AppResult<BootReport> {
    let json = json.trim();
    if json == ADMIN_MARKER {
        return Err(AppError::RequiresAdmin);
    }
    let raw: Vec<Option<RawEvent>> = if json.is_empty() || json == "null" {
        Vec::new()
    } else if json.starts_with('[') {
        serde_json::from_str(json).map_err(|e| AppError::CommandFailed { command: "log de boot".into(), detail: e.to_string() })?
    } else {
        vec![serde_json::from_str(json).ok()]
    };

    let mut boots = Vec::new();
    let mut slow: Vec<SlowItem> = Vec::new();
    for e in raw.into_iter().flatten() {
        match e.id {
            100 => boots.push(BootTime {
                total_ms: num(&e.data, "BootTime"),
                main_ms: num(&e.data, "MainPathBootTime"),
                post_ms: num(&e.data, "BootPostBootTime"),
                at: e.time,
            }),
            101 | 103 => {
                let name = text(&e.data, "Name");
                if name.is_empty() {
                    continue;
                }
                let item = SlowItem {
                    friendly: text(&e.data, "FriendlyName"),
                    kind: if e.id == 101 { "app" } else { "service" },
                    total_ms: num(&e.data, "TotalTime"),
                    degradation_ms: num(&e.data, "DegradationTime"),
                    at: e.time,
                    name,
                };
                // Mantém só o registro mais recente de cada nome.
                match slow.iter_mut().find(|s| s.name.eq_ignore_ascii_case(&item.name)) {
                    Some(existing) if existing.at < item.at => *existing = item,
                    Some(_) => {}
                    None => slow.push(item),
                }
            }
            _ => {}
        }
    }
    boots.sort_by(|a, b| b.at.cmp(&a.at));
    boots.truncate(10);
    slow.sort_by(|a, b| b.degradation_ms.cmp(&a.degradation_ms));
    Ok(BootReport { boots, slow })
}

/// Nome do executável dentro de um comando de inicialização:
/// "\"C:\\Program Files\\Steam\\steam.exe\" -silent" → "steam.exe".
pub fn exe_of(command: &str) -> Option<String> {
    let c = command.trim();
    let path = if let Some(rest) = c.strip_prefix('"') {
        rest.split('"').next()?
    } else {
        let lower = c.to_ascii_lowercase();
        let end = lower.find(".exe").map(|i| i + 4).unwrap_or(c.len());
        &c[..end]
    };
    let file = path.rsplit(['\\', '/']).next()?.trim();
    (!file.is_empty()).then(|| file.to_string())
}

#[cfg(windows)]
pub fn report() -> AppResult<BootReport> {
    let out = crate::system::powershell(
        "try { \
           $e = Get-WinEvent -FilterHashtable @{ LogName = 'Microsoft-Windows-Diagnostics-Performance/Operational'; Id = 100,101,103 } -MaxEvents 300 -ErrorAction Stop | \
             ForEach-Object { $x = [xml]$_.ToXml(); $d = @{}; foreach ($n in $x.Event.EventData.Data) { $d[$n.Name] = $n.'#text' }; \
               [pscustomobject]@{ Id = $_.Id; Time = $_.TimeCreated.ToString('yyyy-MM-ddTHH:mm:ss'); Data = $d } }; \
           ConvertTo-Json -Depth 4 -Compress -InputObject @($e) \
         } catch { \
           if ($_.FullyQualifiedErrorId -match 'NoMatchingEventsFound') { '[]' } \
           elseif ($_.FullyQualifiedErrorId -match 'Unauthorized' -or $_.Exception -is [System.UnauthorizedAccessException]) { '\"__admin__\"' } \
           else { throw } \
         }",
    )?;
    parse(&out)
}

#[cfg(not(windows))]
pub fn report() -> AppResult<BootReport> {
    Err(AppError::Unsupported)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {"Id":100,"Time":"2026-11-16T08:01:10","Data":{"BootTime":"41230","MainPathBootTime":"19020","BootPostBootTime":"22210"}},
      {"Id":100,"Time":"2026-11-17T08:00:02","Data":{"BootTime":"35100","MainPathBootTime":"18000","BootPostBootTime":"17100"}},
      {"Id":101,"Time":"2026-11-16T08:01:00","Data":{"Name":"OneDrive.exe","FriendlyName":"Microsoft OneDrive","TotalTime":"4100","DegradationTime":"2900"}},
      {"Id":101,"Time":"2026-11-17T08:00:00","Data":{"Name":"onedrive.exe","FriendlyName":"Microsoft OneDrive","TotalTime":"3000","DegradationTime":"1800"}},
      {"Id":103,"Time":"2026-11-17T08:00:01","Data":{"Name":"SysMain","FriendlyName":"SysMain","TotalTime":"5200","DegradationTime":"3100"}},
      {"Id":101,"Time":"2026-11-17T08:00:01","Data":{"Name":null}},
      null
    ]"#;

    #[test]
    fn interpreta_boots_e_atrasos() {
        let r = parse(SAMPLE).unwrap();
        assert_eq!(r.boots.len(), 2);
        assert_eq!(r.boots[0].at, "2026-11-17T08:00:02", "mais recente primeiro");
        assert_eq!(r.boots[0].post_ms, 17100);
        assert_eq!(r.slow.len(), 2, "OneDrive aparece uma vez só");
        assert_eq!(r.slow[0].name, "SysMain", "ordenado pelo atraso");
        assert_eq!(r.slow[0].kind, "service");
        let od = &r.slow[1];
        assert_eq!(od.degradation_ms, 1800, "fica o registro mais recente");
    }

    #[test]
    fn formatos_vazios_e_admin() {
        assert!(parse("[]").unwrap().boots.is_empty());
        assert!(parse("").unwrap().slow.is_empty());
        assert!(matches!(parse(ADMIN_MARKER), Err(AppError::RequiresAdmin)));
        let one = r#"{"Id":100,"Time":"2026-11-17T08:00:02","Data":{"BootTime":"1000","MainPathBootTime":"500","BootPostBootTime":"500"}}"#;
        assert_eq!(parse(one).unwrap().boots.len(), 1);
    }

    #[test]
    fn executavel_do_comando() {
        assert_eq!(exe_of(r#""C:\Program Files (x86)\Steam\steam.exe" -silent"#).as_deref(), Some("steam.exe"));
        assert_eq!(exe_of(r"C:\Users\x\AppData\Local\Discord\Update.exe --processStart Discord.exe").as_deref(), Some("Update.exe"));
        assert_eq!(exe_of(r"C:\Windows\System32\RtkAudUService64.exe -background").as_deref(), Some("RtkAudUService64.exe"));
        assert_eq!(exe_of("").as_deref(), None);
    }
}
