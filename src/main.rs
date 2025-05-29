#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

use embassy_executor::Spawner;
use embassy_stm32::gpio::{Level, Output, Speed};
use embassy_stm32::usart::{Config, Uart};
use embassy_stm32::{bind_interrupts, peripherals, usart};
use embassy_time::Timer;
use panic_halt as _;

mod trafficlight;
use trafficlight::TrafficLight;

// Deal with active-high or active-low, so that the state machine can just use
// easy to understand `true` for on logic.
fn light(led: &mut Output, on: bool) -> () {
    led.set_level(if on { Level::High } else { Level::Low });
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = embassy_stm32::init(Default::default());

    bind_interrupts!(struct Irqs {
        USART1 => usart::InterruptHandler<peripherals::USART1>;
    });
    let mut usart = Uart::new(
        peripherals.USART1,
        peripherals.PA10,
        peripherals.PA9,
        Irqs,
        peripherals.DMA1_CH4,
        peripherals.DMA1_CH5,
        Config::default(), // 115200 baud
    )
    .unwrap();

    let mut led_red = Output::new(peripherals.PB10, Level::Low, Speed::Low);
    let mut led_amber = Output::new(peripherals.PB12, Level::Low, Speed::Low);
    let mut led_green = Output::new(peripherals.PB14, Level::Low, Speed::Low);

    let mut trafficlight = TrafficLight::new();

    loop {
        usart.write(b"start of phase...\n").await.unwrap();
        light(&mut led_red, trafficlight.red());
        light(&mut led_amber, trafficlight.amber());
        light(&mut led_green, trafficlight.green());

        Timer::after_millis(trafficlight.phase_time_seconds() * 1000).await;
        trafficlight.to_next_phase();
    }
}
