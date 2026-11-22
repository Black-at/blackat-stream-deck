//! Heure locale (fuseau et heure d'été gérés par Windows), pour l'écran latéral.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub hour: u16,
    pub minute: u16,
    /// 0 = dimanche … 6 = samedi.
    pub weekday: u16,
    pub day: u16,
    /// 1 = janvier … 12 = décembre.
    pub month: u16,
}

#[cfg(windows)]
pub fn now() -> LocalTime {
    // SAFETY : GetLocalTime remplit simplement la structure renvoyée.
    let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
    LocalTime { year: t.wYear, hour: t.wHour, minute: t.wMinute, weekday: t.wDayOfWeek, day: t.wDay, month: t.wMonth }
}

#[cfg(not(windows))]
pub fn now() -> LocalTime {
    LocalTime { year: 2000, hour: 0, minute: 0, weekday: 0, day: 1, month: 1 }
}

impl LocalTime {
    /// « 18:42 »
    pub fn hhmm(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// « jeu. 12 nov. »
    pub fn short_date(&self) -> String {
        const DAYS: [&str; 7] = ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."];
        const MONTHS: [&str; 12] =
            ["janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.", "déc."];
        let day = DAYS.get(self.weekday as usize).copied().unwrap_or("");
        let month = MONTHS.get((self.month as usize).wrapping_sub(1)).copied().unwrap_or("");
        format!("{day} {} {month}", self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_francais() {
        let t = LocalTime { year: 2026, hour: 8, minute: 5, weekday: 4, day: 12, month: 11 };
        assert_eq!(t.hhmm(), "08:05");
        assert_eq!(t.short_date(), "jeu. 12 nov.");
    }
}
