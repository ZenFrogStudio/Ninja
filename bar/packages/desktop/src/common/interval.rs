use std::time::{Duration, Instant};

/// Lower bound for any provider's refresh interval.
///
/// A zero interval makes the catch-up loop in `SyncInterval::tick` spin
/// forever and causes `tokio::time::interval` to panic, so every refresh
/// interval that reaches a timer is clamped to at least this value.
pub const MIN_REFRESH_INTERVAL_MS: u64 = 100;

/// An interval timer for synchronous contexts using crossbeam.
///
/// For use with crossbeam's `select!` macro.
pub struct SyncInterval {
  interval: Duration,
  next_tick: Instant,
  is_first: bool,
}

impl SyncInterval {
  /// Creates a new `SyncInterval`.
  ///
  /// The interval is clamped to `MIN_REFRESH_INTERVAL_MS`.
  pub fn new(interval_ms: u64) -> Self {
    Self {
      interval: Duration::from_millis(
        interval_ms.max(MIN_REFRESH_INTERVAL_MS),
      ),
      next_tick: Instant::now(),
      is_first: true,
    }
  }

  /// Returns a receiver that will get a message at the next tick time.
  pub fn tick(&mut self) -> crossbeam::channel::Receiver<Instant> {
    if self.is_first {
      // Emit immediately on the first tick.
      self.is_first = false;
      crossbeam::channel::after(Duration::from_secs(0))
    } else if let Some(wait_duration) =
      self.next_tick.checked_duration_since(Instant::now())
    {
      // Wait normally until the next tick.
      let timer = crossbeam::channel::after(wait_duration);
      self.next_tick += self.interval;
      timer
    } else {
      // We're behind - skip missed ticks to catch up.
      while self.next_tick <= Instant::now() {
        self.next_tick += self.interval;
      }

      crossbeam::channel::after(self.next_tick - Instant::now())
    }
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// A zero interval must not spin in the catch-up loop.
  #[test]
  fn sync_interval_clamps_zero() {
    let mut interval = SyncInterval::new(0);

    // First tick fires immediately.
    let _ = interval.tick();

    // Second tick takes the `checked_duration_since` branch; a third is
    // needed to reach the catch-up loop that would spin on a zero
    // interval.
    let _ = interval.tick();
    let _ = interval.tick();

    assert_eq!(
      interval.interval,
      Duration::from_millis(MIN_REFRESH_INTERVAL_MS)
    );
  }
}
