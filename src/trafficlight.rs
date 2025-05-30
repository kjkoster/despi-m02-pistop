#[derive(Debug)]
enum Phase {
    Stop,
    Attention,
    Go,
    Yield,
    ClearCrossing,
}

#[derive(Debug)]
pub struct TrafficLight {
    phase: Phase,
}

impl TrafficLight {
    pub fn new() -> Self {
        TrafficLight { phase: Phase::Stop }
    }

    pub fn to_next_phase(&mut self) -> () {
        self.phase = match self.phase {
            Phase::Stop => Phase::Attention,
            Phase::Attention => Phase::Go,
            Phase::Go => Phase::Yield,
            Phase::Yield => Phase::ClearCrossing,
            Phase::ClearCrossing => Phase::Stop,
        };
    }

    pub fn red(&self) -> bool {
        match self.phase {
            Phase::Stop | Phase::Attention | Phase::ClearCrossing => true,
            Phase::Yield | Phase::Go => false,
        }
    }

    pub fn amber(&self) -> bool {
        match self.phase {
            Phase::Yield | Phase::Attention => true,
            Phase::Stop | Phase::ClearCrossing | Phase::Go => false,
        }
    }

    pub fn green(&self) -> bool {
        match self.phase {
            Phase::Go => true,
            Phase::Stop | Phase::ClearCrossing | Phase::Yield | Phase::Attention => false,
        }
    }

    pub fn phase_time_seconds(&self) -> u64 {
        match self.phase {
            Phase::Stop => 10,
            Phase::Attention => 1,
            Phase::Go => 4,
            Phase::Yield => 3,
            Phase::ClearCrossing => 2,
        }
    }

    pub fn needs_permit(&self) -> bool {
        match self.phase {
            Phase::Attention | Phase::Yield | Phase::Go | Phase::ClearCrossing => true,
            Phase::Stop => false,
        }
    }
}
