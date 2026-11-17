//! Sistema falso em memória para testar o motor sem Windows.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};

use super::ops::{GameSys, Hive, Sys, NORMAL_PRIORITY};
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
