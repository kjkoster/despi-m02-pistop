#[derive(Debug)]
enum Phase {
    Stop,
    Attention,
    Go,
    Yield,
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
            Phase::Yield => Phase::Stop,
        };
    }

    pub fn red(&self) -> bool {
        match self.phase {
            Phase::Stop | Phase::Attention => true,
            Phase::Yield | Phase::Go => false,
        }
    }

    pub fn amber(&self) -> bool {
        match self.phase {
            Phase::Yield | Phase::Attention => true,
            Phase::Stop | Phase::Go => false,
        }
    }

    pub fn green(&self) -> bool {
        match self.phase {
            Phase::Go => true,
            Phase::Stop | Phase::Yield | Phase::Attention => false,
        }
    }

    pub fn phase_time_seconds(&self) -> u64 {
        match self.phase {
            Phase::Stop => 10,
            Phase::Attention => 1,
            Phase::Yield => 3,
            Phase::Go => 4,
        }
    }
}
