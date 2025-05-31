pub mod semaphore;
use semaphore::{acquire_permit, release_permit};

#[derive(Debug)]
enum Phase {
    Stop,
    Attention,
    Go,
    Yield,
    ClearCrossing,
    FlashOn,
    FlashOff,
}

#[derive(Debug)]
pub struct TrafficLight {
    phase: Phase,
    have_permit: bool,
}

fn needs_permit(phase: &Phase) -> bool {
    match phase {
        Phase::Attention | Phase::Yield | Phase::Go | Phase::ClearCrossing => true,
        Phase::Stop | Phase::FlashOn | Phase::FlashOff => false,
    }
}

impl TrafficLight {
    pub fn new() -> Self {
        TrafficLight {
            phase: Phase::Stop,
            have_permit: false,
        }
    }

    /*
     * Determine the next phase, without changing the phase that we are in.
     */
    fn next_phase(&self, maintenance_mode: bool) -> Phase {
        match (&self.phase, maintenance_mode) {
            // free run mode...
            (Phase::Stop, false) => Phase::Attention,
            (Phase::Attention, false) => Phase::Go,
            (Phase::Go, false) => Phase::Yield,
            (Phase::Yield, false) => Phase::ClearCrossing,
            (Phase::ClearCrossing, false) => Phase::Stop,
            (Phase::FlashOn, false) => Phase::ClearCrossing,
            (Phase::FlashOff, false) => Phase::ClearCrossing,
            // maintenance mode...
            (Phase::Stop, true) => Phase::FlashOn,
            (Phase::Attention, true) => Phase::Go,
            (Phase::Go, true) => Phase::Yield,
            (Phase::Yield, true) => Phase::ClearCrossing,
            (Phase::ClearCrossing, true) => Phase::FlashOn,
            (Phase::FlashOn, true) => Phase::FlashOff,
            (Phase::FlashOff, true) => Phase::FlashOn,
        }
    }

    pub async fn go_to_next_phase(&mut self, maintenance_mode: bool) {
        let next_phase: Phase = self.next_phase(maintenance_mode);

        match (self.have_permit, needs_permit(&next_phase)) {
            (false, true) => {
                acquire_permit().await;
                self.have_permit = true;
            }
            (true, false) => {
                release_permit();
                self.have_permit = false;
            }
            (false, false) | (true, true) => {}
        }

        self.phase = next_phase;
    }

    pub fn red(&self) -> bool {
        match self.phase {
            Phase::Stop | Phase::Attention | Phase::ClearCrossing => true,
            Phase::Yield | Phase::Go | Phase::FlashOn | Phase::FlashOff => false,
        }
    }

    pub fn amber(&self) -> bool {
        match self.phase {
            Phase::Yield | Phase::Attention | Phase::FlashOn => true,
            Phase::Stop | Phase::ClearCrossing | Phase::Go | Phase::FlashOff => false,
        }
    }

    pub fn green(&self) -> bool {
        match self.phase {
            Phase::Go => true,
            Phase::Stop
            | Phase::ClearCrossing
            | Phase::Yield
            | Phase::Attention
            | Phase::FlashOn
            | Phase::FlashOff => false,
        }
    }

    pub fn phase_time_seconds(&self) -> u64 {
        match self.phase {
            Phase::Stop => 10,
            Phase::Attention => 1,
            Phase::Go => 4,
            Phase::Yield => 3,
            Phase::ClearCrossing => 2,
            Phase::FlashOn | Phase::FlashOff => 1,
        }
    }
}
