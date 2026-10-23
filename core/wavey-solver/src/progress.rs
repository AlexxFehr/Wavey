//! Progress of a solve and the time left.
//!
//! The geometrical stages and the wave solver run at the same time and start
//! together, so the run is as far along as its slowest stream. The bar
//! follows that stream (it shows the text of what that stream is doing), and
//! the time left is extrapolated from it.

/// Share of the bar kept for what follows the two streams (the late field's
/// vertical coherence, blending and packing the result).
const TAIL: f64 = 0.02;

/// A stream is not extrapolated before it has done this much.
const MIN_FRACTION: f64 = 0.02;

/// Runs shorter than this are not extrapolated.
const MIN_ELAPSED_S: f64 = 3.0;

#[derive(Debug, Clone, Default)]
struct Stream {
    fraction: f64,
    stage: String,
}

impl Stream {
    fn set(&mut self, fraction: f64, stage: &str) {
        // A stream never goes back, whatever order its steps report in.
        self.fraction = self.fraction.max(fraction.clamp(0.0, 1.0));
        self.stage = stage.to_string();
    }
}

/// The bar of a run with a geometrical stream and, optionally, a wave solver.
#[derive(Debug, Clone)]
pub struct Tracker {
    geometry: Stream,
    wave: Option<Stream>,
    last: f64,
}

impl Tracker {
    pub fn new(with_wave: bool) -> Self {
        Self { geometry: Stream::default(), wave: with_wave.then(Stream::default), last: 0.0 }
    }

    /// The geometrical stream reached `fraction` (0…1). Returns the bar and
    /// the text to show.
    pub fn geometry(&mut self, fraction: f64, stage: &str) -> (f64, String) {
        self.geometry.set(fraction, stage);
        self.bar()
    }

    /// The wave solver reached `fraction` (0…1). Returns the bar and the text.
    pub fn wave(&mut self, fraction: f64, stage: &str) -> (f64, String) {
        if let Some(w) = &mut self.wave {
            w.set(fraction, stage);
        }
        self.bar()
    }

    /// The bar (never decreasing) and the text of the stream it follows.
    pub fn bar(&mut self) -> (f64, String) {
        let (slowest, other) = match &self.wave {
            Some(w) if w.fraction < self.geometry.fraction => (w, &self.geometry),
            _ => (&self.geometry, self.wave.as_ref().unwrap_or(&self.geometry)),
        };
        self.last = self.last.max(slowest.fraction * (1.0 - TAIL));
        // A stream that has not said what it is doing yet shows the other's text.
        let stage = if slowest.stage.is_empty() { &other.stage } else { &slowest.stage };
        (self.last, stage.clone())
    }
}

/// Seconds left after `elapsed_s` at overall `fraction` of the bar, if the
/// run has gone far enough to tell. The bar is as far as the slowest stream,
/// so this is that stream's own extrapolation.
pub fn seconds_left(elapsed_s: f64, fraction: f64) -> Option<f64> {
    let streams_done = (fraction / (1.0 - TAIL)).clamp(0.0, 1.0);
    if elapsed_s < MIN_ELAPSED_S || streams_done < MIN_FRACTION {
        return None;
    }
    Some(elapsed_s * (1.0 - streams_done) / streams_done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} vs {b}");
    }

    #[test]
    fn the_bar_follows_the_slower_stream_and_shows_its_text() {
        let mut t = Tracker::new(true);
        // Geometry finishes quickly; the wave solver has barely started.
        let (bar, stage) = t.geometry(1.0, "S1: ray tracing");
        close(bar, 0.0);
        assert_eq!(stage, "S1: ray tracing");
        let (bar, stage) = t.wave(0.1, "S1: wave solver 1/4");
        close(bar, 0.1 * 0.98);
        assert_eq!(stage, "S1: wave solver 1/4");
        // The geometry no longer drives the bar, whatever it reports.
        let (bar, _) = t.geometry(1.0, "waiting");
        close(bar, 0.1 * 0.98);
        let (bar, _) = t.wave(1.0, "S8: wave solver 4/4");
        close(bar, 0.98);
    }

    #[test]
    fn the_bar_never_goes_back() {
        let mut t = Tracker::new(true);
        t.geometry(0.9, "a");
        let (b1, _) = t.wave(0.8, "b");
        // Geometry's next stage reports a lower fraction than its last one.
        let (b2, _) = t.geometry(0.85, "c");
        assert!(b2 >= b1);
        // And a stream that was the slow one and the other overtakes it.
        let (b3, _) = t.wave(0.9, "d");
        assert!(b3 >= b2);
    }

    #[test]
    fn without_a_wave_solver_the_geometry_drives_the_bar() {
        let mut t = Tracker::new(false);
        let (bar, _) = t.geometry(0.5, "tracing");
        close(bar, 0.49);
        // Wave progress is ignored.
        let (bar, _) = t.wave(0.1, "ignored");
        close(bar, 0.49);
    }

    #[test]
    fn a_run_that_is_all_wave_solver_reports_the_wave_solver() {
        let mut t = Tracker::new(true);
        t.geometry(1.0, "no geometry needed");
        let (bar, stage) = t.wave(0.5, "S1: wave solver");
        close(bar, 0.49);
        assert_eq!(stage, "S1: wave solver");
    }

    #[test]
    fn time_left_is_scripted_over_a_run_with_a_long_wave_solver() {
        let mut t = Tracker::new(true);
        // 5 s: geometry done, wave solver 1 % through. Too early to tell.
        t.geometry(1.0, "geometry");
        let (bar, _) = t.wave(0.01, "wave");
        assert_eq!(seconds_left(5.0, bar), None);
        // 30 s: wave solver 10 % through. The old bar read 90 % here.
        let (bar, _) = t.wave(0.10, "wave");
        close(bar, 0.098);
        let left = seconds_left(30.0, bar).unwrap();
        close(left, 30.0 * 0.9 / 0.1);
        // 150 s: half way. Another 150 s.
        let (bar, _) = t.wave(0.5, "wave");
        close(seconds_left(150.0, bar).unwrap(), 150.0);
        // Done with the streams: nothing left but the tail.
        let (bar, _) = t.wave(1.0, "wave");
        close(seconds_left(300.0, bar).unwrap(), 0.0);
    }

    #[test]
    fn no_estimate_for_a_short_run_or_before_enough_progress() {
        assert_eq!(seconds_left(2.0, 0.5), None);
        assert_eq!(seconds_left(60.0, 0.01), None);
        assert_eq!(seconds_left(60.0, 0.0), None);
        close(seconds_left(10.0, 0.49).unwrap(), 10.0);
    }
}
