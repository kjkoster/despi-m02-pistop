/*
 * The I/O module for the traffic lights.
 *
 * This module wraps the I/O ports into a set of higher level functions to
 * perform operations such as lighting traffic lights and reading the system
 * mode rotary switch.
 *
 * Ideally, all board- and hardware-specific code should be here. Practically,
 * this means that only this module should have STM32, HAL or PAC imports.
 */

use embassy_stm32::{
    bind_interrupts,
    gpio::{Input, Level, Output, Pin, Pull, Speed},
    mode::Async,
    peripherals::USART1,
    usart::{Config, InterruptHandler, Uart},
};
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    mutex::{Mutex, MutexGuard},
};

const IO_INIT_PANIC: &str = "I/O not initialised";

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum SystemMode {
    Normal,
    Flash,
    PriorityA,
    PriorityB,
}

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum Lane {
    A,
    B,
}

static OUTPUTS_A: Mutex<ThreadModeRawMutex, Option<[Output; 3]>> = Mutex::new(None);
static OUTPUTS_B: Mutex<ThreadModeRawMutex, Option<[Output; 3]>> = Mutex::new(None);
static POWER: Mutex<ThreadModeRawMutex, Option<Output>> = Mutex::new(None);
static LED4: Mutex<ThreadModeRawMutex, Option<Output>> = Mutex::new(None);
static LOCKOUT: Mutex<ThreadModeRawMutex, Option<Output>> = Mutex::new(None);
static MODE_INPUTS: Mutex<ThreadModeRawMutex, Option<[Input; 3]>> = Mutex::new(None);
static UART: Mutex<ThreadModeRawMutex, Option<Uart<'static, Async>>> = Mutex::new(None);

/// Set up the I/O channels and initialise inputs and outputs. This function
/// will also light the colours as specified by the input parameters. This
/// allows the code to initialise the lights in a safe state.
///
/// Call this function before any of the others on this module, otherwise the
/// system will panic.
pub async fn initialise_io(red: bool, amber: bool, green: bool, power: bool, lockout: bool) {
    let peripherals = embassy_stm32::init(Default::default());

    let outputs_a: [Output; 3] = [
        Output::new(peripherals.PE1.degrade(), Level::Low, Speed::Low),
        Output::new(peripherals.PB9.degrade(), Level::Low, Speed::Low),
        Output::new(peripherals.PB7.degrade(), Level::Low, Speed::Low),
    ];
    OUTPUTS_A.lock().await.replace(outputs_a);
    light_traffic_lights(Lane::A, red, amber, green).await;

    let outputs_b: [Output; 3] = [
        Output::new(peripherals.PB6.degrade(), Level::Low, Speed::Low),
        Output::new(peripherals.PB8.degrade(), Level::Low, Speed::Low),
        Output::new(peripherals.PE0.degrade(), Level::Low, Speed::Low),
    ];
    OUTPUTS_B.lock().await.replace(outputs_b);
    light_traffic_lights(Lane::B, red, amber, green).await;

    let power_led: Output = Output::new(peripherals.PE3.degrade(), Level::Low, Speed::Low);
    POWER.lock().await.replace(power_led);
    let onboard_led4 = Output::new(peripherals.PE12, Level::Low, Speed::Low);
    LED4.lock().await.replace(onboard_led4);
    light_power(power).await;

    let lockout_led: Output = Output::new(peripherals.PE5.degrade(), Level::Low, Speed::Low);
    LOCKOUT.lock().await.replace(lockout_led);
    light_lockout(lockout).await;

    let system_mode_inputs: [Input; 3] = [
        Input::new(peripherals.PB14.degrade(), Pull::Up),
        Input::new(peripherals.PB12.degrade(), Pull::Up),
        Input::new(peripherals.PB10.degrade(), Pull::Up),
    ];
    MODE_INPUTS.lock().await.replace(system_mode_inputs);

    bind_interrupts!(struct Irqs {
        USART1 => InterruptHandler<USART1>;
    });
    let uart: Uart<'static, Async> = Uart::new(
        peripherals.USART1,
        peripherals.PA10,
        peripherals.PA9,
        Irqs,
        peripherals.DMA1_CH4,
        peripherals.DMA1_CH5,
        Config::default(), // 115200 baud
    )
    .unwrap();
    UART.lock().await.replace(uart);
}

/// Light lamps on both traffic lights in the specified lane. The leds are wired
/// to be active-high, but this function handles that. Pass in `true` for a
/// colour to turn it on and `false` to turn it off.
pub async fn light_traffic_lights(lane: Lane, red: bool, amber: bool, green: bool) {
    let mut outputs_guard: MutexGuard<'_, ThreadModeRawMutex, Option<[Output; 3]>> = match lane {
        Lane::A => OUTPUTS_A.lock(),
        Lane::B => OUTPUTS_B.lock(),
    }
    .await;
    let outputs: &mut [Output; 3] = outputs_guard.as_mut().expect(IO_INIT_PANIC);

    outputs[0].set_level(if red { Level::High } else { Level::Low });
    outputs[1].set_level(if amber { Level::High } else { Level::Low });
    outputs[2].set_level(if green { Level::High } else { Level::Low });
}

async fn light_led(led: &Mutex<ThreadModeRawMutex, Option<Output<'_>>>, on: bool) {
    let mut led_guard: MutexGuard<'_, ThreadModeRawMutex, Option<Output>> = led.lock().await;
    let led: &mut Output = led_guard.as_mut().expect(IO_INIT_PANIC);

    led.set_level(if on { Level::High } else { Level::Low });
}

/// Light the power status led as well as the on-board  led, named `LED4` in the
/// schematic and the silkscreen on the PCB. The two leds are mirrored. Pass in
/// `true` to turn the leds on and `false` to switch them off.
///
/// As an aside: Leds `LED1` (power), `LED2` (serial RX) and `LED3` (serial TX)
/// cannot be controlled from code. They have been hardwired on the PCB.
///
/// The power led is active-high and `LED4` is active-low, but this function
/// handles that.
pub async fn light_power(on: bool) {
    light_led(&POWER, on).await;
    light_led(&LED4, !on).await;
}

/// Control the state of the lockout indicator led.
pub async fn light_lockout(on: bool) {
    light_led(&LOCKOUT, on).await;
}

/// Toggle the state of the lockout indicator led.
pub async fn toggle_lockout() {
    let mut led_guard: MutexGuard<'_, ThreadModeRawMutex, Option<Output>> = LOCKOUT.lock().await;
    let lockout: &mut Output = led_guard.as_mut().expect(IO_INIT_PANIC);

    lockout.toggle();
}

/// Read the raw value from the system mode rotary switch. The result of this
/// value has to be deounced before it can be used reliably.
pub async fn read_system_mode() -> SystemMode {
    let mode_inputs_guard: MutexGuard<'_, ThreadModeRawMutex, Option<[Input; 3]>> =
        MODE_INPUTS.lock().await;
    let mode_inputs: &[Input; 3] = mode_inputs_guard.as_ref().expect(IO_INIT_PANIC);

    match (
        mode_inputs[0].is_low(),
        mode_inputs[1].is_low(),
        mode_inputs[2].is_low(),
    ) {
        (false, false, false) => SystemMode::Normal,
        (true, _, _) => SystemMode::Flash,
        (_, true, _) => SystemMode::PriorityA,
        (_, _, true) => SystemMode::PriorityB,
    }
}

/// Print a trace message to the serial console. This function does not add line
/// endings, so end each line with `\r\n`.
pub async fn print(message: &str) {
    let mut uart_guard: MutexGuard<'_, ThreadModeRawMutex, Option<Uart<'static, Async>>> =
        UART.lock().await;
    let uart: &mut Uart<'static, Async> = uart_guard.as_mut().expect(IO_INIT_PANIC);

    uart.write(message.as_bytes()).await.unwrap();
}
