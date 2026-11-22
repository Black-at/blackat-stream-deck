//! Plugin « Compte à rebours » : un appui lance le décompte affiché sur la touche (anneau + mm:ss),
//! un nouvel appui l'annule. À zéro : petit son et « FINI » pendant quelques secondes.
//!
//! Chaque touche a son propre décompte ; il continue quand on change de page.

use std::collections::HashMap;

use tokio::time::{Duration, Instant};

/// Durée du « FINI » affiché à la fin.
const DONE_FLASH: Duration = Duration::from_secs(5);

/// Durées proposées dans l'éditeur (secondes).
pub const DURATIONS: [u32; 10] = [30, 60, 120, 180, 300, 600, 900, 1200, 1800, 3600];

/// Identifie une touche : (index de page dans la config, position).
pub type KeyId = (usize, usize);

#[derive(Debug, Clone, Copy)]
enum Run {
    Running { end: Instant, total: u32 },
    Done { until: Instant },
}

/// État affiché d'un décompte.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Idle,
    /// Secondes restantes, et part écoulée (0 → 1).
    Running { left: u64, progress: f32 },
    Done,
}

#[derive(Debug, Default)]
pub struct Countdowns {
    runs: HashMap<KeyId, Run>,
}

impl Countdowns {
    /// Appui : lance, ou annule si ça tourne déjà (ou efface le « FINI »).
    pub fn press(&mut self, id: KeyId, seconds: u32, now: Instant) {
        match self.runs.get(&id) {
            Some(Run::Running { .. }) | Some(Run::Done { .. }) => {
                self.runs.remove(&id);
            }
            None => {
                let total = seconds.max(1);
                self.runs.insert(id, Run::Running { end: now + Duration::from_secs(total as u64), total });
            }
        }
    }

    pub fn view(&self, id: KeyId, now: Instant) -> View {
        match self.runs.get(&id) {
            None => View::Idle,
            Some(Run::Done { .. }) => View::Done,
            Some(Run::Running { end, total }) => {
                let left_ms = end.saturating_duration_since(now).as_millis() as u64;
                let left = left_ms.div_ceil(1000);
                let progress = 1.0 - left_ms as f32 / (*total as f32 * 1000.0);
                View::Running { left, progress: progress.clamp(0.0, 1.0) }
            }
        }
    }

    /// Un décompte (ou un « FINI ») est-il visible ? Le démon a alors besoin du battement 1 Hz.
    pub fn active(&self) -> bool {
        !self.runs.is_empty()
    }

    /// Battement : passe les décomptes arrivés à zéro en « FINI », efface les « FINI » échus.
    /// Renvoie le nombre de décomptes qui viennent de se terminer (pour jouer le son).
    pub fn tick(&mut self, now: Instant) -> usize {
        let mut finished = 0;
        self.runs.retain(|_, run| match *run {
            Run::Running { end, .. } if now >= end => {
                *run = Run::Done { until: now + DONE_FLASH };
                finished += 1;
                true
            }
            Run::Done { until } => now < until,
            _ => true,
        });
        finished
    }

    /// Oublie les décomptes des touches qui ne sont plus des comptes à rebours (config changée).
    pub fn retain(&mut self, mut still: impl FnMut(KeyId) -> bool) {
        self.runs.retain(|id, _| still(*id));
    }
}

/// 30 → « 30 s », 300 → « 5 min », 90 → « 1 min 30 », 3600 → « 1 h ».
pub fn label(seconds: u32) -> String {
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    match (h, m, s) {
        (0, 0, s) => format!("{s} s"),
        (0, m, 0) => format!("{m} min"),
        (0, m, s) => format!("{m} min {s:02}"),
        (h, 0, 0) => format!("{h} h"),
        (h, m, _) => format!("{h} h {m:02}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lance_termine_puis_efface() {
        let mut c = Countdowns::default();
        let t0 = Instant::now();
        let id = (0, 3);
        c.press(id, 10, t0);
        assert_eq!(c.view(id, t0 + Duration::from_millis(2500)), View::Running { left: 8, progress: 0.25 });
        assert_eq!(c.tick(t0 + Duration::from_secs(9)), 0);
        assert_eq!(c.tick(t0 + Duration::from_secs(10)), 1);
        assert_eq!(c.view(id, t0 + Duration::from_secs(11)), View::Done);
        assert!(c.active());
        c.tick(t0 + Duration::from_secs(16));
        assert_eq!(c.view(id, t0 + Duration::from_secs(16)), View::Idle);
        assert!(!c.active());
    }

    #[test]
    fn second_appui_annule() {
        let mut c = Countdowns::default();
        let t0 = Instant::now();
        c.press((1, 0), 60, t0);
        c.press((1, 0), 60, t0 + Duration::from_secs(5));
        assert_eq!(c.view((1, 0), t0 + Duration::from_secs(5)), View::Idle);
    }

    #[test]
    fn libelles() {
        assert_eq!(label(30), "30 s");
        assert_eq!(label(300), "5 min");
        assert_eq!(label(90), "1 min 30");
        assert_eq!(label(3600), "1 h");
        assert_eq!(label(5400), "1 h 30");
    }
}
