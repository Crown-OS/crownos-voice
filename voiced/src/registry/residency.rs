use std::sync::Arc;
use std::time::{Duration, Instant};

/// How long a loaded backend stays mapped after its last instance ends.
pub const IDLE_TTL: Duration = Duration::from_secs(60);

/// A lazily loaded, shared value that is released once nothing else has held it for a while.
pub struct Residency<T> {
    loaded: Option<Arc<T>>,
    idle_since: Option<Instant>,
}

impl<T> Default for Residency<T> {
    fn default() -> Self {
        Self {
            loaded: None,
            idle_since: None,
        }
    }
}

impl<T> Residency<T> {
    pub fn is_loaded(&self) -> bool {
        self.loaded.is_some()
    }

    pub fn get_or_try_load<E>(
        &mut self,
        load: impl FnOnce() -> Result<Arc<T>, E>,
    ) -> Result<&Arc<T>, E> {
        self.idle_since = None;
        match &mut self.loaded {
            Some(loaded) => Ok(loaded),
            empty => Ok(empty.insert(load()?)),
        }
    }

    /// Releases the value once only this residency has held it for `ttl`.
    pub fn sweep(&mut self, now: Instant, ttl: Duration) {
        let Some(loaded) = &self.loaded else {
            return;
        };
        if Arc::strong_count(loaded) > 1 {
            self.idle_since = None;
            return;
        }
        let idle_since = *self.idle_since.get_or_insert(now);
        if now.duration_since(idle_since) >= ttl {
            self.loaded = None;
            self.idle_since = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded() -> (Residency<()>, Arc<()>) {
        let mut residency = Residency::default();
        let held = residency
            .get_or_try_load(|| Ok::<_, ()>(Arc::new(())))
            .map(Arc::clone);
        (residency, held.unwrap_or_default())
    }

    #[test]
    fn stays_loaded_while_an_instance_holds_it() {
        let (mut residency, held) = loaded();
        let start = Instant::now();
        residency.sweep(start, IDLE_TTL);
        residency.sweep(start + IDLE_TTL * 2, IDLE_TTL);
        assert!(residency.is_loaded());
        drop(held);
    }

    #[test]
    fn unloads_a_full_ttl_after_the_last_instance_ends() {
        let (mut residency, held) = loaded();
        drop(held);
        let idle = Instant::now();
        residency.sweep(idle, IDLE_TTL);
        residency.sweep(idle + IDLE_TTL - Duration::from_secs(1), IDLE_TTL);
        assert!(residency.is_loaded());
        residency.sweep(idle + IDLE_TTL, IDLE_TTL);
        assert!(!residency.is_loaded());
    }

    #[test]
    fn reuse_restarts_the_idle_clock() {
        let (mut residency, held) = loaded();
        drop(held);
        let idle = Instant::now();
        residency.sweep(idle, IDLE_TTL);
        let reused = residency.get_or_try_load(|| Err(())).map(Arc::clone);
        assert!(reused.is_ok());
        drop(reused);
        residency.sweep(idle + IDLE_TTL, IDLE_TTL);
        assert!(
            residency.is_loaded(),
            "the idle clock restarts at the next sweep"
        );
        residency.sweep(idle + IDLE_TTL * 2, IDLE_TTL);
        assert!(!residency.is_loaded());
    }

    #[test]
    fn a_failed_load_leaves_it_unloaded() {
        let mut residency = Residency::<()>::default();
        assert!(residency.get_or_try_load(|| Err("missing")).is_err());
        assert!(!residency.is_loaded());
    }
}
