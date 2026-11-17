//! Sistema falso em memória para testar o motor sem Windows.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::ops::{GameSys, Hive, NetKey, Sys, NORMAL_PRIORITY};
use crate::error::{AppError, AppResult};
use crate::power::PowerPlan;

#[derive(Debug)]
struct State {
    values: HashMap<(Hive, String, String), Value>,
    keys: HashSet<(Hive, String)>,
    plan: PowerPlan,
    mouse: [i32; 3],
    mouse_speed: u32,
    memory_compression: bool,
    fail_on: Option<String>,
    deny_hklm: bool,
    /// pid -> (exe, prioridade)
    procs: Vec<(u32, String, u32)>,
    services: HashMap<String, bool>,
    /// (sub, config) -> valor no plano; chave ausente = PC não tem a configuração.
    /// O valor é guardado por (plano, sub, config) pra testar o "desfazer no mesmo plano".
    power: HashMap<(String, String, String), u32>,
    tasks: HashMap<String, bool>,
    page_combining: bool,
    net: HashMap<NetKey, String>,
    /// (env, arquivo) -> objeto JSON.
    json: HashMap<(String, String), serde_json::Map<String, serde_json::Value>>,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Dword(u32),
    Str(String),
}

impl Default for State {
    fn default() -> Self {
        Self {
            values: HashMap::new(),
            keys: HashSet::new(),
            plan: PowerPlan::Balanced,
            mouse: [6, 10, 1],
            mouse_speed: 14,
            memory_compression: true,
            fail_on: None,
            deny_hklm: false,
            procs: Vec::new(),
            services: HashMap::new(),
            power: HashMap::new(),
            tasks: HashMap::new(),
            page_combining: true,
            net: HashMap::new(),
            json: HashMap::new(),
        }
    }
}

/// Clone compartilha o mesmo estado: o teste guarda uma cópia pra inspecionar o
/// "registro" enquanto o motor usa a outra.
#[derive(Debug, Clone, Default)]
pub struct FakeSys(Arc<Mutex<State>>);

fn k(hive: Hive, path: &str, name: &str) -> (Hive, String, String) {
    (hive, path.to_lowercase(), name.to_lowercase())
}

impl FakeSys {
    pub fn fail_on(&self, value_name: &str) {
        self.0.lock().unwrap().fail_on = Some(value_name.to_lowercase());
    }
    pub fn spawn(&self, pid: u32, exe: &str) {
        self.0.lock().unwrap().procs.push((pid, exe.to_string(), NORMAL_PRIORITY));
    }
    pub fn kill(&self, pid: u32) {
        self.0.lock().unwrap().procs.retain(|p| p.0 != pid);
    }
    pub fn prio(&self, pid: u32) -> Option<u32> {
        self.0.lock().unwrap().procs.iter().find(|p| p.0 == pid).map(|p| p.2)
    }
    pub fn set_service(&self, name: &str, running: bool) {
        self.0.lock().unwrap().services.insert(name.to_string(), running);
    }
    pub fn service(&self, name: &str) -> Option<bool> {
        self.0.lock().unwrap().services.get(name).copied()
    }
    /// Faz o PC "ter" uma configuração de energia, com o valor no plano ativo.
    pub fn add_power_setting(&self, sub: &str, setting: &str, value: u32) {
        let plan = self.active_plan().unwrap().0;
        self.0.lock().unwrap().power.insert((plan, sub.into(), setting.into()), value);
    }
    pub fn power_in(&self, plan: &str, sub: &str, setting: &str) -> Option<u32> {
        self.0.lock().unwrap().power.get(&(plan.to_string(), sub.to_string(), setting.to_string())).copied()
    }
    pub fn add_task(&self, path: &str, enabled: bool) {
        self.0.lock().unwrap().tasks.insert(path.into(), enabled);
    }
    pub fn task(&self, path: &str) -> Option<bool> {
        self.0.lock().unwrap().tasks.get(path).copied()
    }
    pub fn set_net(&self, key: NetKey, value: &str) {
        self.0.lock().unwrap().net.insert(key, value.into());
    }
    pub fn add_json(&self, env: &str, file: &str, content: serde_json::Value) {
        let map = content.as_object().cloned().unwrap_or_default();
        self.0.lock().unwrap().json.insert((env.into(), file.into()), map);
    }
    pub fn json(&self, env: &str, file: &str) -> Option<serde_json::Value> {
        self.0.lock().unwrap().json.get(&(env.to_string(), file.to_string())).map(|m| serde_json::Value::Object(m.clone()))
    }
    pub fn deny_hklm(&self) {
        self.0.lock().unwrap().deny_hklm = true;
    }

    fn check_write(&self, hive: Hive, name: &str) -> AppResult<()> {
        let s = self.0.lock().unwrap();
        if s.deny_hklm && hive == Hive::Hklm {
            return Err(AppError::RequiresAdmin);
        }
        if s.fail_on.as_deref() == Some(&name.to_lowercase()) {
            return Err(AppError::CommandFailed { command: "fake".into(), detail: "falha simulada".into() });
        }
        Ok(())
    }

    fn add_key_chain(s: &mut State, hive: Hive, path: &str) {
        let lower = path.to_lowercase();
        let mut acc = String::new();
        for part in lower.split('\\') {
            if !acc.is_empty() {
                acc.push('\\');
            }
            acc.push_str(part);
            s.keys.insert((hive, acc.clone()));
        }
    }
}

const GUIDS: [(PowerPlan, &str); 3] = [
    (PowerPlan::PowerSaver, "a1841308-3541-4fab-bc81-f71556f20b4a"),
    (PowerPlan::Balanced, "381b4222-f694-41f0-9685-ff5bb260df2e"),
    (PowerPlan::HighPerformance, "8c5e7fda-e8bf-4a96-9a85-a6e23a8c635c"),
];

impl Sys for FakeSys {
    fn dword(&self, hive: Hive, path: &str, name: &str) -> Option<u32> {
        match self.0.lock().unwrap().values.get(&k(hive, path, name)) {
            Some(Value::Dword(v)) => Some(*v),
            _ => None,
        }
    }

    fn set_dword(&self, hive: Hive, path: &str, name: &str, value: Option<u32>) -> AppResult<()> {
        self.check_write(hive, name)?;
        let mut s = self.0.lock().unwrap();
        match value {
            Some(v) => {
                Self::add_key_chain(&mut s, hive, path);
                s.values.insert(k(hive, path, name), Value::Dword(v));
            }
            None => {
                s.values.remove(&k(hive, path, name));
            }
        }
        Ok(())
    }

    fn string(&self, hive: Hive, path: &str, name: &str) -> Option<String> {
        match self.0.lock().unwrap().values.get(&k(hive, path, name)) {
            Some(Value::Str(v)) => Some(v.clone()),
            _ => None,
        }
    }

    fn set_string(&self, hive: Hive, path: &str, name: &str, value: Option<&str>) -> AppResult<()> {
        self.check_write(hive, name)?;
        let mut s = self.0.lock().unwrap();
        match value {
            Some(v) => {
                Self::add_key_chain(&mut s, hive, path);
                s.values.insert(k(hive, path, name), Value::Str(v.to_string()));
            }
            None => {
                s.values.remove(&k(hive, path, name));
            }
        }
        Ok(())
    }

    fn key_exists(&self, hive: Hive, path: &str) -> bool {
        self.0.lock().unwrap().keys.contains(&(hive, path.to_lowercase()))
    }

    fn delete_key_tree(&self, hive: Hive, path: &str) -> AppResult<()> {
        let mut s = self.0.lock().unwrap();
        let prefix = path.to_lowercase();
        let inside = |p: &str| p == prefix || p.starts_with(&format!("{prefix}\\"));
        s.keys.retain(|(h, p)| !(*h == hive && inside(p)));
        s.values.retain(|(h, p, _), _| !(*h == hive && inside(p)));
        Ok(())
    }

    fn stop_service(&self, _name: &str) {}
    fn start_service(&self, _name: &str) {}

    fn active_plan(&self) -> AppResult<(String, Option<PowerPlan>)> {
        let plan = self.0.lock().unwrap().plan;
        let guid = GUIDS.iter().find(|(p, _)| *p == plan).unwrap().1;
        Ok((guid.to_string(), Some(plan)))
    }

    fn set_plan(&self, plan: PowerPlan) -> AppResult<()> {
        self.0.lock().unwrap().plan = plan;
        Ok(())
    }

    fn set_plan_guid(&self, guid: &str) -> AppResult<()> {
        let plan = GUIDS.iter().find(|(_, g)| *g == guid).map(|(p, _)| *p).ok_or(AppError::NotFound(guid.into()))?;
        self.set_plan(plan)
    }

    fn mouse(&self) -> AppResult<[i32; 3]> {
        Ok(self.0.lock().unwrap().mouse)
    }

    fn set_mouse(&self, values: [i32; 3]) -> AppResult<()> {
        self.0.lock().unwrap().mouse = values;
        Ok(())
    }

    fn mouse_speed(&self) -> AppResult<u32> {
        Ok(self.0.lock().unwrap().mouse_speed)
    }

    fn set_mouse_speed(&self, value: u32) -> AppResult<()> {
        self.0.lock().unwrap().mouse_speed = value;
        Ok(())
    }

    fn memory_compression(&self) -> AppResult<bool> {
        Ok(self.0.lock().unwrap().memory_compression)
    }

    fn set_memory_compression(&self, enabled: bool) -> AppResult<()> {
        self.0.lock().unwrap().memory_compression = enabled;
        Ok(())
    }

    fn power_value(&self, sub: &str, setting: &str) -> AppResult<Option<(String, u32)>> {
        let plan = self.active_plan()?.0;
        let s = self.0.lock().unwrap();
        // "Existe no PC" = existe em algum plano; o valor é o do plano ativo (0 se nunca mudou).
        let exists = s.power.keys().any(|(_, a, b)| a == sub && b == setting);
        Ok(exists.then(|| {
            let v = s.power.get(&(plan.clone(), sub.to_string(), setting.to_string())).copied().unwrap_or(0);
            (plan, v)
        }))
    }

    fn set_power_value(&self, plan: &str, sub: &str, setting: &str, ac: u32) -> AppResult<()> {
        self.check_write(Hive::Hklm, setting)?;
        self.0.lock().unwrap().power.insert((plan.into(), sub.into(), setting.into()), ac);
        Ok(())
    }

    fn task_enabled(&self, path: &str) -> Option<bool> {
        self.task(path)
    }

    fn set_task_enabled(&self, path: &str, enabled: bool) -> AppResult<()> {
        self.check_write(Hive::Hklm, path)?;
        let mut s = self.0.lock().unwrap();
        let t = s.tasks.get_mut(path).ok_or(AppError::NotFound(path.into()))?;
        *t = enabled;
        Ok(())
    }

    fn page_combining(&self) -> AppResult<bool> {
        Ok(self.0.lock().unwrap().page_combining)
    }

    fn set_page_combining(&self, enabled: bool) -> AppResult<()> {
        self.0.lock().unwrap().page_combining = enabled;
        Ok(())
    }

    fn net_setting(&self, key: NetKey) -> AppResult<String> {
        self.0.lock().unwrap().net.get(&key).cloned().ok_or(AppError::Unsupported)
    }

    fn set_net_setting(&self, key: NetKey, value: &str) -> AppResult<()> {
        let v = key.canonical(value).ok_or_else(|| AppError::InvalidInput(value.into()))?;
        self.0.lock().unwrap().net.insert(key, v.into());
        Ok(())
    }

    fn json_file_exists(&self, env: &str, file: &str) -> bool {
        self.0.lock().unwrap().json.contains_key(&(env.to_string(), file.to_string()))
    }

    fn json_key(&self, env: &str, file: &str, key: &str) -> AppResult<Option<String>> {
        let s = self.0.lock().unwrap();
        let map = s.json.get(&(env.to_string(), file.to_string())).ok_or(AppError::NotFound(file.into()))?;
        Ok(map.get(key).map(|v| v.to_string()))
    }

    fn set_json_key(&self, env: &str, file: &str, key: &str, value: Option<&str>) -> AppResult<()> {
        let mut s = self.0.lock().unwrap();
        let map = s.json.get_mut(&(env.to_string(), file.to_string())).ok_or(AppError::NotFound(file.into()))?;
        match value {
            Some(v) => {
                map.insert(key.into(), serde_json::from_str(v).map_err(|e| AppError::InvalidInput(e.to_string()))?);
            }
            None => {
                map.remove(key);
            }
        }
        Ok(())
    }
}

impl GameSys for FakeSys {
    fn processes(&self) -> Vec<(u32, String)> {
        self.0.lock().unwrap().procs.iter().map(|(p, n, _)| (*p, n.clone())).collect()
    }
    fn priority(&self, pid: u32) -> Option<u32> {
        self.prio(pid)
    }
    fn set_priority(&self, pid: u32, class: u32) -> AppResult<()> {
        let mut s = self.0.lock().unwrap();
        let p = s.procs.iter_mut().find(|p| p.0 == pid).ok_or(AppError::NotFound(pid.to_string()))?;
        p.2 = class;
        Ok(())
    }
    fn service_running(&self, name: &str) -> Option<bool> {
        self.service(name)
    }
    fn pause_service(&self, name: &str) -> AppResult<()> {
        self.set_service(name, false);
        Ok(())
    }
    fn resume_service(&self, name: &str) -> AppResult<()> {
        self.set_service(name, true);
        Ok(())
    }
}
