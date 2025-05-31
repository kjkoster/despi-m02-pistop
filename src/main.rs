#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::Spawner;
use embassy_stm32::{
    exti::{AnyChannel, Channel, ExtiInput},
    gpio::{AnyPin, Level, Output, Pin, Pull, Speed},
};
use embassy_time::Timer;
use panic_halt as _;

mod trafficlight;
use trafficlight::TrafficLight;
use trafficlight::semaphore::{NUM_TRAFFICLIGHTS, acquire_permit, release_permit};

static MAINTENANCE_MODE: AtomicBool = AtomicBool::new(false);

// Deal with active-high or active-low, so that the state machine can just use
// easy to understand `true` for on logic.
fn light(led: &mut Output, on: bool) {
    led.set_level(if on { Level::High } else { Level::Low });
}

#[embassy_executor::task(pool_size = NUM_TRAFFICLIGHTS)]
async fn trafficlight_task(pin_red: AnyPin, pin_amber: AnyPin, pin_green: AnyPin) {
    let mut red = Output::new(pin_red, Level::High, Speed::Low);
    let mut amber = Output::new(pin_amber, Level::Low, Speed::Low);
    let mut green = Output::new(pin_green, Level::Low, Speed::Low);

    let mut trafficlight = TrafficLight::new();
    loop {
        Timer::after_millis(trafficlight.phase_time_seconds() * 1000).await;

        trafficlight
            .go_to_next_phase(MAINTENANCE_MODE.load(Ordering::Relaxed))
            .await;

        light(&mut red, trafficlight.red());
        light(&mut amber, trafficlight.amber());
        light(&mut green, trafficlight.green());
    }
}

#[embassy_executor::task]
async fn maintenance_button_task(pin_button: AnyPin, interrupt_channel: AnyChannel) {
    let mut button = ExtiInput::new(pin_button, interrupt_channel, Pull::Up);

    loop {
        button.wait_for_low().await;

        // XXX no: both lights need to be in `Phase::Stop` before I can enable maintenance mode.
        //
        MAINTENANCE_MODE.fetch_not(Ordering::Relaxed);
        if MAINTENANCE_MODE.load(Ordering::Relaxed) {
            acquire_permit().await;
        } else {
            release_permit();
        }

        // debounce....
        Timer::after_millis(200).await;
        button.wait_for_high().await;
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
    spawner
        .spawn(maintenance_button_task(
            peripherals.PE11.degrade(),
            peripherals.EXTI11.degrade(),
        ))
        .unwrap();

    // Show and help count seconds by flashing the on-board LED roughly once
    // every second.
    let mut led4 = Output::new(peripherals.PE12, Level::Low, Speed::Low);
    loop {
        led4.set_low();
        if MAINTENANCE_MODE.load(Ordering::Relaxed) {
            Timer::after_millis(500).await;
        } else {
            Timer::after_millis(15).await;
        }
        led4.set_high();
        if MAINTENANCE_MODE.load(Ordering::Relaxed) {
            Timer::after_millis(500).await;
        } else {
            Timer::after_millis(985).await;
        }
    }
}
