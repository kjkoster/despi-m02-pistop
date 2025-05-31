#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

use embassy_executor::Spawner;
use embassy_stm32::gpio::{AnyPin, Level, Output, Pin, Speed};
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    semaphore::{FairSemaphore, Semaphore},
};
use embassy_time::Timer;
use panic_halt as _;

mod trafficlight;
use trafficlight::TrafficLight;

const NUM_TRAFFICLIGHTS: usize = 2;

type CrossingSemaphore = FairSemaphore<ThreadModeRawMutex, NUM_TRAFFICLIGHTS>;
static CROSSING_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(1);

// Deal with active-high or active-low, so that the state machine can just use
// easy to understand `true` for on logic.
fn light(led: &mut Output, on: bool) {
    led.set_level(if on { Level::High } else { Level::Low });
}

#[embassy_executor::task(pool_size = NUM_TRAFFICLIGHTS)]
async fn trafficlight_task(pin_red: AnyPin, pin_amber: AnyPin, pin_green: AnyPin) {
    let mut red = Output::new(pin_red, Level::Low, Speed::Low);
    let mut amber = Output::new(pin_amber, Level::Low, Speed::Low);
    let mut green = Output::new(pin_green, Level::Low, Speed::Low);

    let mut trafficlight = TrafficLight::new();
    let mut holds_permit = false;
    loop {
        light(&mut red, trafficlight.red());
        light(&mut amber, trafficlight.amber());
        light(&mut green, trafficlight.green());

        Timer::after_millis(trafficlight.phase_time_seconds() * 1000).await;

        trafficlight.to_next_phase();
        match (holds_permit, trafficlight.needs_permit()) {
            (false, true) => {
                CROSSING_SEMAPHORE.acquire(1).await.unwrap().disarm();
                holds_permit = true;
            }
            (true, false) => {
                CROSSING_SEMAPHORE.release(1);
                holds_permit = false;
            }
            (false, false) | (true, true) => {}
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = embassy_stm32::init(Default::default());

    spawner
        .spawn(trafficlight_task(
            peripherals.PE1.degrade(),
            peripherals.PB9.degrade(),
            peripherals.PB7.degrade(),
        ))
        .unwrap();
    spawner
        .spawn(trafficlight_task(
            peripherals.PB6.degrade(),
            peripherals.PB8.degrade(),
            peripherals.PE0.degrade(),
        ))
        .unwrap();

    // Show and help count seconds by flashing the on-board LED roughly once
    // every second.
    let mut led4 = Output::new(peripherals.PE12, Level::Low, Speed::Low);
    loop {
        led4.set_low();
        Timer::after_millis(15).await;
        led4.set_high();
        Timer::after_millis(985).await;
    }
}
