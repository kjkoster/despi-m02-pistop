/*
 * Traffic lights use quite a few timers. If we programmed those into our main
 * program using explicit waits, we run into a few problems. First, the code is
 * hard to read, because of all of the waiting that happens. Second, this means
 * that many parts of the code need to await more than one event, adding
 * complexity. Third (and maybe not immediately obvious), all timers have to be
 * synchronised. For example, when the lights blink amber at night, all amber
 * lights change state at the exact same time. Finally, the cycles of the lights
 * have to be synchronised to the timer too, so that there are no weird super
 * short states in edge cases.
 *
 * Interestingly, in an analog or discrete-digital world this kind of timer
 * logic is much simpler than in the world of free-running tasks. We would just
 * have the timer buses toggling on- and off all the time and we would `AND` and
 * `OR` that into our control logic.
 *
 * The intention of this module is to bring the simplicity of the analog world
 * to our free-running task world. It does this by separating setting the output
 * pin states from setting the desired pin state. The module can then control
 * the pin state based on what our control logic asks via an output state
 * descriptor, which precisely captures the desired pin state and what timers
 * the pin is subject to. This way, the control logic can specify the desired
 * pin state regardless of timing and wait intervals.
 *
 * This module exposes a collection of output pins, each of which can be on or
 * off, but also subject to one or more timers.
 */

use core::sync::atomic::{AtomicBool, Ordering};
use enum_ordinalize::Ordinalize;

#[derive(Ordinalize, Clone, Copy)]
#[repr(usize)]
pub enum Pins {
    // Left-right lane, lights A, pedestrian lights D, promise F and beeper.
    ARed,
    AAmber,
    AGreen,
    APedestrianRed,
    APedestrianGreen,
    APromise,
    ABeeper,

    // Up-down lane: lights B, pedestrian lists C and promise E.
    BRed,
    BAmber,
    BGreen,
    BPedestrianRed,
    BPedestrianGreen,
    BPromise,
    // The PCB does not have a beeper for the up-down lane. We have a mock value
    // here to keep the code orthogonal. It is simply mapped to an unused output
    // pin.
    BBeeper,

    // common
    OnBoardPower,
    Power,
    SwitchingMode,
}

#[derive(Copy, Clone)]
struct OutputStateDescriptor {
    on: bool,
    subject_to_slow_cycle: bool,
    subject_to_fast_cycle: bool,
    subject_to_pip_timer: bool,
}

impl OutputStateDescriptor {
    const fn new() -> Self {
        OutputStateDescriptor {
            on: false,
            subject_to_slow_cycle: false,
            subject_to_fast_cycle: false,
            subject_to_pip_timer: false,
        }
    }
}

pub struct TimedOutputMasker {
    output_descriptors: [OutputStateDescriptor; Pins::VARIANT_COUNT],
    active_lows: [bool; Pins::VARIANT_COUNT],
    tick_count: u8,
    slow_cycle_value: AtomicBool,
    fast_cycle_value: AtomicBool,
    pip_timer_value: AtomicBool,
}

static TICKS_PER_CYCLE: u8 = 100;
impl TimedOutputMasker {
    pub const fn new(active_lows: [bool; Pins::VARIANT_COUNT]) -> Self {
        TimedOutputMasker {
            output_descriptors: [OutputStateDescriptor::new(); Pins::VARIANT_COUNT],
            active_lows: active_lows,
            tick_count: TICKS_PER_CYCLE - 1,
            slow_cycle_value: AtomicBool::new(false),
            fast_cycle_value: AtomicBool::new(false),
            pip_timer_value: AtomicBool::new(false),
        }
    }

    /*
     * In order to keep this module testable we keep all time and delay
     * functions outside the module. We could have made this function into its
     * own task, but testing it would be difficult.
     *
     * XXX Consider exposing a task, which only calls this function at 100Hz.
     */
    pub fn call_at_100_hz(&mut self) -> [bool; Pins::VARIANT_COUNT] {
        self.advance_timers();
        self.mask_output_pins()
    }

    fn advance_timers(&mut self) {
        self.tick_count = (self.tick_count + 1) % TICKS_PER_CYCLE;

        self.slow_cycle_value
            .store(self.tick_count < 50, Ordering::Relaxed);
        self.fast_cycle_value
            .store((self.tick_count / 10) % 2 == 1, Ordering::Relaxed);
        self.pip_timer_value
            .store(self.tick_count == 0, Ordering::Relaxed);
    }

    fn mask_output_pins(&mut self) -> [bool; Pins::VARIANT_COUNT] {
        let mut outputs = [false; Pins::VARIANT_COUNT];
        for i in 0..Pins::VARIANT_COUNT {
            let output_descriptor: &OutputStateDescriptor = &self.output_descriptors[i];
            outputs[i] = output_descriptor.on;

            if output_descriptor.subject_to_slow_cycle {
                outputs[i] = outputs[i] & self.slow_cycle_value.load(Ordering::Relaxed);
            }
            if output_descriptor.subject_to_fast_cycle {
                outputs[i] = outputs[i] & self.fast_cycle_value.load(Ordering::Relaxed);
            }
            if output_descriptor.subject_to_pip_timer {
                outputs[i] = outputs[i] & self.pip_timer_value.load(Ordering::Relaxed);
            }

            if self.active_lows[i] {
                outputs[i] = !outputs[i];
            }
        }

        outputs
    }

    pub fn set_on_off3(
        &mut self,
        pin0: Pins,
        on0: bool,
        pin1: Pins,
        on1: bool,
        pin2: Pins,
        on2: bool,
    ) {
        self.set_pin(pin0, on0, false, false, false);
        self.set_pin(pin1, on1, false, false, false);
        self.set_pin(pin2, on2, false, false, false);
    }

    pub fn set_on_off2(&mut self, pin0: Pins, on0: bool, pin1: Pins, on1: bool) {
        self.set_pin(pin0, on0, false, false, false);
        self.set_pin(pin1, on1, false, false, false);
    }

    pub fn set_on_off(&mut self, pin: Pins, on: bool) {
        self.set_pin(pin, on, false, false, false);
    }

    pub fn set_pin(
        &mut self,
        pin: Pins,
        on: bool,
        subject_to_slow_cycle: bool,
        subject_to_fast_cycle: bool,
        subject_to_pip_timer: bool,
    ) {
        self.output_descriptors[pin.ordinal()] = OutputStateDescriptor {
            on: on,
            subject_to_slow_cycle: subject_to_slow_cycle,
            subject_to_fast_cycle: subject_to_fast_cycle,
            subject_to_pip_timer: subject_to_pip_timer,
        }
    }
}
