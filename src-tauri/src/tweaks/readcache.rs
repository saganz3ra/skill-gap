//! Cache de leituras caras (PowerShell, powercfg, schtasks) que vive só durante uma
//! "rodada de leitura", como montar a lista de ajustes.
//!
//! Por que só durante a rodada: a lista pergunta a mesma coisa várias vezes (o valor
//! existe? já está aplicado?), e cada pergunta abria um processo. Fora da rodada nada
//! é guardado, então a foto do valor original (tirada ao aplicar) sempre lê o Windows
//! de verdade, nunca um valor velho.

use std::any::Any;
use std::collections::BTreeMap;
use std::sync::Mutex;

struct State {
    /// Rodadas abertas agora (a lista roda em várias threads ao mesmo tempo).
    open: usize,
    values: BTreeMap<String, Box<dyn Any + Send>>,
}

static STATE: Mutex<State> = Mutex::new(State { open: 0, values: BTreeMap::new() });

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

/// Abre uma rodada; quando a última rodada aberta termina, o cache é apagado.
pub fn scope<R>(f: impl FnOnce() -> R) -> R {
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            let mut s = state();
            s.open -= 1;
            if s.open == 0 {
                s.values.clear();
            }
        }
    }
    state().open += 1;
    let _g = Guard;
    f()
}

/// Devolve o valor guardado nesta rodada ou calcula com `f`. Fora de rodada, só calcula.
/// O cálculo roda sem segurar a trava, para outras threads seguirem lendo.
pub fn get<T: Clone + Send + 'static>(key: &str, f: impl FnOnce() -> T) -> T {
    {
        let s = state();
        if s.open == 0 {
            drop(s);
            return f();
        }
        if let Some(v) = s.values.get(key).and_then(|b| b.downcast_ref::<T>()) {
            return v.clone();
        }
    }
    let v = f();
    let mut s = state();
    if s.open > 0 {
        s.values.insert(key.to_string(), Box::new(v.clone()));
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    // Os testes dividem o estado global: um teste só, em sequência.
    #[test]
    fn caches_only_inside_scope_and_clears_after() {
        let calls = AtomicUsize::new(0);
        let read = || {
            get("k-test", || {
                calls.fetch_add(1, Ordering::SeqCst);
                7u32
            })
        };
        assert_eq!(read(), 7);
        assert_eq!(read(), 7);
        assert_eq!(calls.load(Ordering::SeqCst), 2, "fora da rodada não guarda");

        scope(|| {
            assert_eq!(read(), 7);
            assert_eq!(read(), 7);
            scope(|| assert_eq!(read(), 7));
        });
        assert_eq!(calls.load(Ordering::SeqCst), 3, "dentro da rodada lê uma vez só");

        read();
        assert_eq!(calls.load(Ordering::SeqCst), 4, "rodada fechada apaga o cache");
    }
}
