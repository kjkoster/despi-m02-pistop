/*
 * The I/O module for the traffic lights.
 *
 * This module implements a task that is responsible for controlling the actual
 * I/O pins on the device. The intention is for this module to be the only part
 * of the program that is device-specific.
 *
 * This module exports a few types and an Embassy I/O task that provides shared
 * access to the GPIO on the device, plus a debouncer task. Other tasks can
 * control the outputs and read input using channels.
 */

use embassy_futures::select::{Either, Either3, select, select3};
use embassy_stm32::{
    exti::{Channel, ExtiInput},
    gpio::{Level, Output, Pin, Pull, Speed},
};
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Receiver, Sender},
};
use embassy_time::{Duration, Timer};

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum Leg {
    A,
    B,
}

#[derive(Copy, Clone)]
pub struct Rag {
    leg: Leg,
    red: bool,
    amber: bool,
    green: bool,
}

impl Rag {
    pub fn new(leg: Leg, red: bool, amber: bool, green: bool) -> Self {
        Self {
            leg,
            red,
            amber,
            green,
        }
    }
}

pub const CHANNEL_CAPACITY: usize = 4;

#[embassy_executor::task]
pub async fn io_task(
    rags: Receiver<'static, ThreadModeRawMutex, Rag, CHANNEL_CAPACITY>,
    blinky: Receiver<'static, ThreadModeRawMutex, bool, CHANNEL_CAPACITY>,
    onboard_button_raw: Sender<'static, ThreadModeRawMutex, bool, CHANNEL_CAPACITY>,
) -> ! {
    let peripherals = embassy_stm32::init(Default::default());

    let mut outputs_a: [Output; 3] = [
        Output::new(peripherals.PE1.degrade(), Level::High, Speed::Low),
        Output::new(peripherals.PB9.degrade(), Level::Low, Speed::Low),
        Output::new(peripherals.PB7.degrade(), Level::Low, Speed::Low),
    ];
    let mut outputs_b: [Output; 3] = [
        Output::new(peripherals.PB6.degrade(), Level::High, Speed::Low),
        Output::new(peripherals.PB8.degrade(), Level::Low, Speed::Low),
        Output::new(peripherals.PE0.degrade(), Level::Low, Speed::Low),
    ];

    let mut onboard_led = Output::new(peripherals.PE12, Level::Low, Speed::Low);

    let mut onboard_button = ExtiInput::new(
        peripherals.PE11.degrade(),
        peripherals.EXTI11.degrade(),
        Pull::Up,
    );

    loop {
        match select3(
            rags.receive(),
            blinky.receive(),
            onboard_button.wait_for_falling_edge(),
        )
        .await
        {
            Either3::First(rag @ Rag { leg: Leg::A, .. }) => light(&mut outputs_a, &rag),
            Either3::First(rag @ Rag { leg: Leg::B, .. }) => light(&mut outputs_b, &rag),
            Either3::Second(blinky_on) => {
                // the on-board LED is active-low
                onboard_led.set_level(if blinky_on { Level::Low } else { Level::High })
            }
            Either3::Third(_) => onboard_button_raw.send(true).await,
        }
    }
}

fn light(outputs: &mut [Output; 3], rag: &Rag) {
    outputs[0].set_level(if rag.red { Level::High } else { Level::Low });
    outputs[1].set_level(if rag.amber { Level::High } else { Level::Low });
    outputs[2].set_level(if rag.green { Level::High } else { Level::Low });
}

// The debouncer can be put into a channel, effectively transforming it into a
// delayed `Watch` that buffers the last message until the debounce timeout
// expires. Imagine a rotary switch: as the user rotates the switch, it makes
// brief contact on all intermediate states. Likewise, a pushbutton that marches
// a system through various modes. After each button press, you'd like a short
// delay before the button press takes effect. On top of that, humans make
// mistakes. They want to switch to mode X ... no Y. Yes, Y is good.
//
// If the system were to respond to each of these events, a single mode change
// may cause the system to quickly cycle through a handful of modes. At best
// this is a waste of effort, at worst this causes upset. In any case, it may
// cause confusing flickering or other confusing output.
#[embassy_executor::task]
pub async fn debounce_task(
    input: Receiver<'static, ThreadModeRawMutex, bool, CHANNEL_CAPACITY>,
    output: Sender<'static, ThreadModeRawMutex, bool, CHANNEL_CAPACITY>,
    debounce: Duration,
) -> ! {
    loop {
        let mut value: bool = input.receive().await;

        'debounce_loop: loop {
            match select(input.receive(), Timer::after(debounce)).await {
                Either::First(new_value) => value = new_value,
                Either::Second(_) => break 'debounce_loop,
            }
        }

        output.send(value).await;
    }
}
