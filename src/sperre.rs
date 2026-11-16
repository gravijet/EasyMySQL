// Sperren, die einen abgestuerzten Hintergrund-Thread ueberstehen. Stuerzt ein Thread ab, waehrend er eine
// Sperre haelt, ist sie "vergiftet": `lock().unwrap()` wuerde dann auch die Oberflaeche abstuerzen lassen.
// Die Daten dahinter sind in dieser App immer ein in sich gueltiger Stand (Texte, Listen, Zustaende), daher
// arbeiten wir mit ihnen weiter, statt die ganze App zu verlieren.

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub trait Sperre<T: ?Sized> {
    /// Wie `lock().unwrap()`, aber ohne Absturz bei einer vergifteten Sperre
    fn lock_ok(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> Sperre<T> for Mutex<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub trait LeseSperre<T: ?Sized> {
    /// Wie `read().unwrap()`, ohne Absturz bei einer vergifteten Sperre
    fn read_ok(&self) -> RwLockReadGuard<'_, T>;
    /// Wie `write().unwrap()`, ohne Absturz bei einer vergifteten Sperre
    fn write_ok(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> LeseSperre<T> for RwLock<T> {
    fn read_ok(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }
    fn write_ok(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn vergiftete_sperren_bringen_die_oberflaeche_nicht_zum_absturz() {
        let m = Arc::new(Mutex::new(vec![1, 2]));
        let r = Arc::new(RwLock::new(String::from("ok")));
        let (m2, r2) = (Arc::clone(&m), Arc::clone(&r));
        let _ = std::thread::spawn(move || {
            let _g = m2.lock().unwrap();
            let _w = r2.write().unwrap();
            panic!("Hintergrund-Thread stuerzt ab");
        })
        .join();
        assert!(m.is_poisoned() && r.is_poisoned());
        m.lock_ok().push(3);
        assert_eq!(*m.lock_ok(), vec![1, 2, 3]);
        r.write_ok().push('!');
        assert_eq!(*r.read_ok(), "ok!");
    }
}
