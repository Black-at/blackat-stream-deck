//! Plugin « Infos PC » : charge du processeur et de la mémoire, pour une touche ou l'écran latéral.
//!
//! Deux appels Windows très légers, faits une fois par seconde seulement quand c'est affiché.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    /// Processeur, en % (moyenne de tous les cœurs depuis la mesure précédente).
    pub cpu: u8,
    /// Mémoire utilisée, en %.
    pub ram: u8,
    /// Mémoire utilisée et totale, en Go.
    pub ram_used_gb: f32,
    pub ram_total_gb: f32,
}

#[derive(Debug, Default)]
pub struct Sampler {
    /// (temps d'inactivité, temps total) de la mesure précédente, en unités de 100 ns.
    last: Option<(u64, u64)>,
    current: Option<Stats>,
}

impl Sampler {
    pub fn current(&self) -> Option<Stats> {
        self.current
    }

    /// Nouvelle mesure. Le processeur demande deux mesures : la première ne donne que la mémoire.
    pub fn sample(&mut self) {
        let Some((idle, total, ram)) = read() else { return };
        let cpu = match self.last {
            Some((idle0, total0)) => cpu_percent(idle0, total0, idle, total),
            None => 0,
        };
        self.last = Some((idle, total));
        self.current = Some(Stats { cpu, ..ram });
    }

    /// Plus affiché : on oublie la mesure (la prochaine repart de zéro).
    pub fn reset(&mut self) {
        self.last = None;
        self.current = None;
    }
}

fn cpu_percent(idle0: u64, total0: u64, idle: u64, total: u64) -> u8 {
    let dt = total.saturating_sub(total0);
    if dt == 0 {
        return 0;
    }
    let busy = dt.saturating_sub(idle.saturating_sub(idle0));
    ((busy as f64 / dt as f64) * 100.0).round().clamp(0.0, 100.0) as u8
}

#[cfg(windows)]
fn read() -> Option<(u64, u64, Stats)> {
    use windows::Win32::{
        Foundation::FILETIME,
        System::{
            SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX},
            Threading::GetSystemTimes,
        },
    };
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    let mut mem = MEMORYSTATUSEX { dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32, ..Default::default() };
    // SAFETY : pointeurs vers des structures locales valides pendant l'appel.
    unsafe {
        GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?;
        GlobalMemoryStatusEx(&mut mem).ok()?;
    }
    let ft = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    // Le temps « noyau » de Windows inclut déjà le temps d'inactivité.
    let total = ft(kernel) + ft(user);
    const GB: f32 = 1024.0 * 1024.0 * 1024.0;
    let stats = Stats {
        cpu: 0,
        ram: mem.dwMemoryLoad.min(100) as u8,
        ram_used_gb: (mem.ullTotalPhys - mem.ullAvailPhys) as f32 / GB,
        ram_total_gb: mem.ullTotalPhys as f32 / GB,
    };
    Some((ft(idle), total, stats))
}

#[cfg(not(windows))]
fn read() -> Option<(u64, u64, Stats)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pourcentage_processeur() {
        assert_eq!(cpu_percent(0, 0, 75, 100), 25);
        assert_eq!(cpu_percent(10, 10, 10, 10), 0);
        assert_eq!(cpu_percent(0, 0, 0, 100), 100);
    }

    #[test]
    #[cfg(windows)]
    fn mesure_reelle() {
        let mut s = Sampler::default();
        s.sample();
        let st = s.current().unwrap();
        assert!(st.ram > 0 && st.ram <= 100);
        assert!(st.ram_total_gb > 1.0);
    }
}
