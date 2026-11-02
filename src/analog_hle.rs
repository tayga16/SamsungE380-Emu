//! Opt-in synthetic power measurement, not recovered analog hardware.
//! The original ARM driver sends packed address 3b (FIFO word 0ec0), then
//! reads a 12-bit sample at serial +18 through its normal IRQ9 completion.
//! Only this observed single-word transaction is supported. A raw sample
//! of 1000 is a declared environment assumption: the observed channel-1
//! calibration converts it to 3,704,836 (consistent with ~3.7 V in microvolts).

#[derive(Default)]
pub(crate) struct AnalogHle {
    pub(crate) enabled: bool,
    pending: Option<u16>,
    result: Option<u16>,
}

impl AnalogHle {
    pub(crate) fn new(enabled: bool) -> Self {
        Self { enabled, pending: None, result: None }
    }

    pub(crate) fn reconfigure(&mut self) {
        self.pending = None;
        self.result = None;
    }

    pub(crate) fn control(&mut self, active: bool, size: usize, value: u64,
        addresses: u32, extension: u32, fifo: u32)
    {
        // A new submission invalidates the previous result. The observed
        // ISR's halfword stop must retain it for the subsequent ARM read.
        if !active || size != 2 || value != 0 {
            self.result = None;
        }
        self.pending = (self.enabled && active && size == 2 && value == 4
            && addresses == 0x3b && extension == 0 && fifo == 0x0ec0)
            .then_some(1000);
    }

    pub(crate) fn complete(&mut self, active: bool) -> Option<u16> {
        let sample = self.pending.take();
        if self.enabled && active {
            if sample.is_some() { self.result = sample; }
            sample
        } else {
            self.result = None;
            None
        }
    }

    pub(crate) fn sample(&self, active: bool, addresses: u32, extension: u32) -> Option<u16> {
        if self.enabled && active && addresses == 0x3b && extension == 0 {
            self.result
        } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::AnalogHle;

    #[test]
    fn exact_observed_transaction_completes_once() {
        let mut model = AnalogHle::new(true);
        model.control(true, 2, 4, 0x3b, 0, 0x0ec0);
        assert_eq!(model.complete(true), Some(1000));
        assert_eq!(model.complete(true), None);
        assert_eq!(1000 * 4126 - 421164, 3704836);
    }

    #[test]
    fn unknown_partial_and_multichannel_requests_are_not_answered() {
        for (size, control, addresses, extension, fifo) in [
            (1, 4, 0x3b, 0, 0x0ec0), (4, 4, 0x3b, 0, 0x0ec0),
            (2, 1, 0x3b, 0, 0x0ec0), (2, 4, 0x3d, 0, 0x0ec0),
            (2, 4, 0x3b, 1, 0x0ec0), (2, 4, 0x3b, 0, 0x0f40),
            (2, 4, 0x3c3b, 0, 0x0ec0),
        ] {
            let mut model = AnalogHle::new(true);
            model.control(true, size, control, addresses, extension, fifo);
            assert_eq!(model.complete(true), None);
        }
    }

    #[test]
    fn default_off_and_runtime_gates_do_not_leak_pending_work() {
        let mut off = AnalogHle::default();
        off.control(true, 2, 4, 0x3b, 0, 0x0ec0);
        assert_eq!(off.complete(true), None);
        let mut model = AnalogHle::new(true);
        model.control(false, 2, 4, 0x3b, 0, 0x0ec0);
        assert_eq!(model.complete(true), None);
        model.control(true, 2, 4, 0x3b, 0, 0x0ec0);
        assert_eq!(model.complete(false), None);
        assert_eq!(model.complete(true), None);
    }

    #[test]
    fn stop_or_replacement_cancels_an_unfinished_measurement() {
        let mut model = AnalogHle::new(true);
        for replacement in [0, 1, 7] {
            model.control(true, 2, 4, 0x3b, 0, 0x0ec0);
            model.control(true, 2, replacement, 0x3b, 0, 0x0ec0);
            assert_eq!(model.complete(true), None);
        }
    }

    #[test]
    fn completed_sample_survives_stop_but_not_an_unrelated_transaction() {
        let mut model = AnalogHle::new(true);
        model.control(true, 2, 4, 0x3b, 0, 0x0ec0);
        assert_eq!(model.sample(true, 0x3b, 0), None); // not yet complete
        model.complete(true);
        model.control(true, 2, 0, 0x3b, 0, 0x0ec0);
        assert_eq!(model.sample(true, 0x3b, 0), Some(1000));
        assert_eq!(model.sample(true, 0x3d, 0), None);
        assert_eq!(model.sample(true, 0x3b, 1), None);
        assert_eq!(model.sample(false, 0x3b, 0), None);
        model.control(true, 2, 4, 0x3d, 0, 0x0f40);
        model.complete(true);
        assert_eq!(model.sample(true, 0x3b, 0), None); // no stale battery result
    }

    #[test]
    fn rewritten_setup_cancels_pending_and_completed_measurements() {
        let mut model = AnalogHle::new(true);
        model.control(true, 2, 4, 0x3b, 0, 0x0ec0);
        model.reconfigure();
        assert_eq!(model.complete(true), None);
        model.control(true, 2, 4, 0x3b, 0, 0x0ec0);
        model.complete(true);
        model.reconfigure();
        assert_eq!(model.sample(true, 0x3b, 0), None);
    }
}
