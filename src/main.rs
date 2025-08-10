#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::Spawner;
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
    semaphore::{FairSemaphore, Semaphore},
    signal::Signal,
};
use embassy_time::Timer;
use enum_ordinalize::Ordinalize;
use panic_halt as _;

mod timed_output_masker;
use timed_output_masker::{Pins, TimedOutputMasker};

const IO_INIT_ERROR: &str = "I/O init error";

#[derive(PartialEq, Eq, Copy, Clone)]
pub enum SystemMode {
    Normal,
    Flash,
    PriorityA,
    PriorityB,
}

struct TrafficLights {
    lights: &'static Mutex<ThreadModeRawMutex, TimedOutputMasker>,
    red: Pins,
    amber: Pins,
    green: Pins,
}

impl TrafficLights {
    const fn new(
        lights: &'static Mutex<ThreadModeRawMutex, TimedOutputMasker>,
        red: Pins,
        amber: Pins,
        green: Pins,
    ) -> Self {
        TrafficLights {
            lights: lights,
            red: red,
            amber: amber,
            green: green,
        }
    }

    async fn go_attention(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        lights.set_on_off3(self.red, true, self.amber, true, self.green, false);
    }
    async fn go_go(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        lights.set_on_off3(self.red, false, self.amber, false, self.green, true);
    }
    async fn go_flash(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        lights.set_on_off2(self.red, false, self.green, false);
        lights.set_pin(self.amber, true, true, false, false);
    }
    async fn go_yield(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        lights.set_on_off3(self.red, false, self.amber, true, self.green, false);
    }
    async fn go_yield_flash(&self) {
        self.go_yield().await;
    }
    async fn go_clear(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        lights.set_on_off3(self.red, true, self.amber, false, self.green, false);
    }
}

struct PedestrianLights {
    lights: &'static Mutex<ThreadModeRawMutex, TimedOutputMasker>,
    red: Pins,
    green: Pins,
    beeper: Pins,
    promise: Pins,
    old_promise: AtomicBool,
    active: AtomicBool,
    promise_made: AtomicBool,
}

impl PedestrianLights {
    const fn new(
        lights: &'static Mutex<ThreadModeRawMutex, TimedOutputMasker>,
        red: Pins,
        green: Pins,
        beeper: Pins,
        promise: Pins,
    ) -> Self {
        PedestrianLights {
            lights: lights,
            red: red,
            green: green,
            beeper: beeper,
            promise: promise,
            old_promise: AtomicBool::new(false),
            active: AtomicBool::new(false),
            promise_made: AtomicBool::new(false),
        }
    }

    async fn go_attention(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;

        lights.set_on_off2(self.red, true, self.green, false);

        self.active.store(true, Ordering::Relaxed);
    }
    async fn go_go(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        let active_promise =
            self.active.load(Ordering::Relaxed) && self.promise_made.load(Ordering::Relaxed);

        lights.set_on_off2(self.red, !active_promise, self.green, active_promise);
        lights.set_pin(self.beeper, active_promise, false, true, false);

        self.old_promise.store(active_promise, Ordering::Relaxed);
        self.promise_made.store(false, Ordering::Relaxed);
        lights.set_on_off(self.promise, false);
    }
    async fn go_flash(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;

        lights.set_on_off3(self.red, false, self.green, false, self.beeper, false);

        self.old_promise.store(false, Ordering::Relaxed);
        self.active.store(false, Ordering::Relaxed);
        self.promise_made.store(false, Ordering::Relaxed);
        lights.set_on_off(self.promise, false);
    }
    async fn go_yield_flash(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;

        lights.set_on_off3(self.red, false, self.green, false, self.beeper, false);

        self.old_promise.store(false, Ordering::Relaxed);
        self.active.store(false, Ordering::Relaxed);
        self.promise_made.store(false, Ordering::Relaxed);
        lights.set_on_off(self.promise, false);
    }
    async fn go_yield(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;
        let active_old_promise =
            self.active.load(Ordering::Relaxed) && self.old_promise.load(Ordering::Relaxed);

        lights.set_pin(self.beeper, active_old_promise, true, true, false);
        lights.set_on_off(self.red, !active_old_promise);
        lights.set_pin(self.green, active_old_promise, true, false, false);
    }
    async fn go_clear(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;

        lights.set_on_off2(self.red, true, self.green, false);
        lights.set_on_off(self.beeper, false);
    }

    async fn make_promise(&self) {
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
            self.lights.lock().await;

        self.promise_made.store(true, Ordering::Relaxed);
        lights.set_on_off(self.promise, true);
        lights.set_pin(
            self.beeper,
            self.active.load(Ordering::Relaxed),
            false,
            false,
            true,
        );
    }
}

type CrossingSemaphore = FairSemaphore<ThreadModeRawMutex, 8>;

// When the system starts, we don't know what happened before the shutdown. We
// cannot trust the mode input, since it may be in debounce. Thus, we start in
// lockout mode, so that all traffic on the crossing is cleared and barred from
// entering. Maybe not efficient, but certainly safe.
static LOCKOUT: AtomicBool = AtomicBool::new(true);

#[embassy_executor::task(pool_size = 2)]
async fn normal_mode_task(
    semaphore: &'static CrossingSemaphore,
    traffic_lights: &'static TrafficLights,
    pedestrian_lights: &'static PedestrianLights,
) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // Attention Phase
        traffic_lights.go_attention().await;
        pedestrian_lights.go_attention().await;
        Timer::after_millis(3_000).await;

        // Go Phase, with pedestrian light handling
        traffic_lights.go_go().await;
        pedestrian_lights.go_go().await;
        Timer::after_millis(8_000).await;

        // Yield Phase
        traffic_lights.go_yield().await;
        pedestrian_lights.go_yield().await;
        Timer::after_millis(6_000).await;

        // Clear Crossing Phase
        traffic_lights.go_clear().await;
        pedestrian_lights.go_clear().await;
        Timer::after_millis(4_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn flash_mode_task(
    semaphore: &'static CrossingSemaphore,
    traffic_lights_a: &'static TrafficLights,
    traffic_lights_b: &'static TrafficLights,
    pedestrian_lights_a: &'static PedestrianLights,
    pedestrian_lights_b: &'static PedestrianLights,
    lockout: &'static AtomicBool,
) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for flashing run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // Flashing Phase
        traffic_lights_a.go_flash().await;
        traffic_lights_b.go_flash().await;
        pedestrian_lights_a.go_flash().await;
        pedestrian_lights_b.go_flash().await;

        while !lockout.load(Ordering::Relaxed) {
            Timer::after_millis(2_000).await;
        }

        // Yield Phase
        traffic_lights_a.go_yield_flash().await;
        traffic_lights_b.go_yield_flash().await;
        pedestrian_lights_a.go_yield_flash().await;
        pedestrian_lights_b.go_yield_flash().await;
        Timer::after_millis(3_000).await;

        // Clear Crossing Phase
        traffic_lights_a.go_clear().await;
        traffic_lights_b.go_clear().await;
        pedestrian_lights_a.go_clear().await;
        pedestrian_lights_b.go_clear().await;
        Timer::after_millis(4_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = 2)]
async fn priority_mode_task(
    semaphore: &'static CrossingSemaphore,
    traffic_lights: &'static TrafficLights,
    pedestrian_lights: &'static PedestrianLights,
    lockout: &'static AtomicBool,
) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // no pedestrians while emergency services pass
        pedestrian_lights.go_clear().await;

        // Attention Phase
        traffic_lights.go_attention().await;
        Timer::after_millis(1_500).await;

        // Go Phase
        traffic_lights.go_go().await;
        Timer::after_millis(4_000).await;

        // crude...
        while !lockout.load(Ordering::Relaxed) {
            Timer::after_millis(500).await;
        }

        // Yield Phase
        traffic_lights.go_yield().await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        traffic_lights.go_clear().await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = 1)]
async fn system_mode_reader_task(
    serial: &'static Mutex<ThreadModeRawMutex, Option<Uart<'static, Async>>>,
    mode_inputs_option: &'static Mutex<ThreadModeRawMutex, Option<[Input<'static>; 3]>>,
    initial_mode: SystemMode,
    system_mode_signal: &'static Signal<ThreadModeRawMutex, SystemMode>,
) -> ! {
    let mode_inputs: [Input<'_>; 3] = mode_inputs_option.lock().await.take().expect(IO_INIT_ERROR);
    let mut current_mode: SystemMode = initial_mode;
    loop {
        print(
            serial,
            "mode reader:                     awaiting user action.\r\n",
        )
        .await;
        #[allow(unused_assignments)]
        let mut new_mode = current_mode;
        'await_change: loop {
            Timer::after_millis(200).await;
            new_mode = read_system_mode(&mode_inputs);
            if new_mode != current_mode {
                print(
                    serial,
                    "mode reader:                     breaking await user action.\r\n",
                )
                .await;
                break 'await_change;
            }
        }

        // So there was a change, but we don't want to signal that change just
        // yet. The rotary switch goes through all intermediate values and needs
        // serious debouncing before it can be read reliably. Also, the user may
        // have overshot the mode they want, so we want to give them a second to
        // check the setting before it becomes file. In fact, we will use a
        // literal second.

        print(
            serial,
            "mode reader:                     awaiting debounce.\r\n",
        )
        .await;
        'await_debounce: loop {
            Timer::after_millis(1_000).await;
            let debounced_mode: SystemMode = read_system_mode(&mode_inputs);
            if debounced_mode == new_mode {
                print(
                    serial,
                    "mode reader:                     breaking debounce.\r\n",
                )
                .await;
                break 'await_debounce;
            } else {
                new_mode = debounced_mode;
            }
        }

        // Finally, suppress signalling if there is no actual change. This
        // reduces the chance of glitches due to quick mode switches.

        if current_mode != new_mode {
            current_mode = new_mode;
            match current_mode {
                SystemMode::Normal => {
                    print(
                        serial,
                        "mode reader:                     signalling SystemMode::Normal.\r\n",
                    )
                    .await
                }
                SystemMode::Flash => {
                    print(
                        serial,
                        "mode reader:                     signalling SystemMode::Flash.\r\n",
                    )
                    .await
                }
                SystemMode::PriorityA => {
                    print(
                        serial,
                        "mode reader:                     signalling SystemMode::PriorityA.\r\n",
                    )
                    .await
                }
                SystemMode::PriorityB => {
                    print(
                        serial,
                        "mode reader:                     signalling SystemMode::PriorityB.\r\n",
                    )
                    .await
                }
            }
            system_mode_signal.signal(current_mode);
        }
    }
}

// Read the raw value from the system mode rotary switch. The result of this
// value has to be debounced before it can be used reliably.
fn read_system_mode(mode_inputs: &[Input; 3]) -> SystemMode {
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

#[embassy_executor::task(pool_size = 1)]
async fn system_mode_task(
    serial: &'static Mutex<ThreadModeRawMutex, Option<Uart<'static, Async>>>,
    start_mode: SystemMode,
    system_mode_signal: &'static Signal<ThreadModeRawMutex, SystemMode>,
    normal_mode_semaphore: &'static CrossingSemaphore,
    flash_mode_semaphore: &'static CrossingSemaphore,
    priority_a_semaphore: &'static CrossingSemaphore,
    priority_b_semaphore: &'static CrossingSemaphore,
    lockout: &'static AtomicBool,
) -> ! {
    // As we start, we hold all the permits. This effectively blocks the traffic
    // light tasks from running, as they will be waiting for a permit to become
    // available. Permits are represented as boolean values, since we can only
    // ever have or have not one.
    let mut have_normal_permit: bool = true;
    let mut have_flash_permit: bool = true;
    let mut have_priority_a_permit: bool = true;
    let mut have_priority_b_permit: bool = true;

    let mut mode: SystemMode = start_mode;
    loop {
        // When we hold every single permit we can release the lockout and then
        // release the permit associated with the current system mode.
        print(serial, "sem handler: releasing lockout.\r\n").await;
        lockout.store(false, Ordering::Relaxed);

        // Collecting semaphores can take quite a bit of time and the user may
        // have changed the value of the system mode while we were busy. Make
        // sure that we are entering the most recently requested mode, so we
        // don't have to quickly cycle through an older one.
        if system_mode_signal.signaled() {
            mode = system_mode_signal.wait().await;
        }

        match mode {
            SystemMode::Normal => {
                print(serial, "sem handler: releasing SystemMode::Normal.\r\n").await;
                ensure_released(&mut have_normal_permit, normal_mode_semaphore);
            }
            SystemMode::Flash => {
                print(serial, "sem handler: releasing SystemMode::Flash.\r\n").await;
                ensure_released(&mut have_flash_permit, flash_mode_semaphore);
            }
            SystemMode::PriorityA => {
                print(serial, "sem handler: releasing SystemMode::PriorityA.\r\n").await;
                ensure_released(&mut have_priority_a_permit, priority_a_semaphore);
            }
            SystemMode::PriorityB => {
                print(serial, "sem handler: releasing SystemMode::PriorityB.\r\n").await;
                ensure_released(&mut have_priority_b_permit, priority_b_semaphore);
            }
        }

        print(serial, "sem handler: awaiting new mode.\r\n").await;
        mode = system_mode_signal.wait().await;

        // When there is a new pending, first signal everyone that we want to go
        // to the lockout state, clearing traffic from the crossing. We then
        // claim all the permits so that we know all tasks are at rest.
        //
        // Some tasks have a simple loop. They just need a semaphore that they
        // release every cycle. Some tasks have a second, inner loop. They need
        // a second trigger to be able to safely break out of the inner loop.
        //
        // It might be tempting to just make the system status into a global
        // variable and use that to break out of the inner loops. Unfortunately,
        // that may leave the semaphore handler task in a deadlocked state. The
        // steps to reach that deadlock are that the user switches to a new
        // state, then switches back while the permits are being collected. The
        // tasks then see that the system mode is as they expected and will not
        // release their permits, while the semaphore handler won't accept new
        // states until all semaphores have been collected.

        print(serial, "sem handler: locking out.\r\n").await;
        lockout.store(true, Ordering::Relaxed);

        print(serial, "sem handler: collecting semaphores...\r\n").await;
        ensure_aquired(&mut have_normal_permit, normal_mode_semaphore).await;
        ensure_aquired(&mut have_flash_permit, flash_mode_semaphore).await;
        ensure_aquired(&mut have_priority_a_permit, priority_a_semaphore).await;
        ensure_aquired(&mut have_priority_b_permit, priority_b_semaphore).await;
    }
}

async fn ensure_aquired(permit: &mut bool, semaphore: &'static CrossingSemaphore) {
    if !*permit {
        semaphore.acquire(1).await.unwrap().disarm();
        *permit = true;
    }
}
fn ensure_released(permit: &mut bool, semaphore: &'static CrossingSemaphore) {
    if !*permit {
        panic!("double free of permit");
    }
    semaphore.release(1);
    *permit = false;
}

#[embassy_executor::task(pool_size = 2)]
async fn promise_input_task(
    input_option: &'static Mutex<ThreadModeRawMutex, Option<Input<'static>>>,
    pedestrian_lights: &'static PedestrianLights,
) -> ! {
    let input: Input = input_option.lock().await.take().expect(IO_INIT_ERROR);
    loop {
        Timer::after_millis(10).await;
        if input.is_low() {
            pedestrian_lights.make_promise().await;
        }
    }
}

pub async fn print(
    uart: &'static Mutex<ThreadModeRawMutex, Option<Uart<'static, Async>>>,
    message: &str,
) {
    uart.lock()
        .await
        .as_mut()
        .expect(IO_INIT_ERROR)
        .write(message.as_bytes())
        .await
        .unwrap();
}

/*
 * The main task defines all of the semaphores and global state, then spawns all
 * of the tasks and finally runs the primary output loop.
 */
#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    // The power led is active-high and `LED4` is active-low.
    static ACTIVE_LOWS: [bool; Pins::VARIANT_COUNT] = {
        let mut active_lows = [false; Pins::VARIANT_COUNT];
        active_lows[ 5 /* Pins::APromise.ordinal() */] = true;
        active_lows[12 /* Pins::BPromise.ordinal() */] = true;
        active_lows[14 /* Pins::OnBoardPower.ordinal() */] = true;
        active_lows[15 /* Pins::Power.ordinal() */] = true;
        active_lows[16 /* Pins::SwitchingMode.ordinal() */] = true;
        active_lows
    };
    static LIGHTS: Mutex<ThreadModeRawMutex, TimedOutputMasker> =
        Mutex::new(TimedOutputMasker::new(ACTIVE_LOWS));

    static TRAFFIC_LIGHTS_A: TrafficLights =
        TrafficLights::new(&LIGHTS, Pins::ARed, Pins::AAmber, Pins::AGreen);
    static TRAFFIC_LIGHTS_B: TrafficLights =
        TrafficLights::new(&LIGHTS, Pins::BRed, Pins::BAmber, Pins::BGreen);

    static PEDESTRIAN_LIGHTS_A: PedestrianLights = PedestrianLights::new(
        &LIGHTS,
        Pins::APedestrianRed,
        Pins::APedestrianGreen,
        Pins::ABeeper,
        Pins::APromise,
    );
    static PEDESTRIAN_LIGHTS_B: PedestrianLights = PedestrianLights::new(
        &LIGHTS,
        Pins::BPedestrianRed,
        Pins::BPedestrianGreen,
        Pins::BBeeper,
        Pins::BPromise,
    );

    const START_MODE: SystemMode = SystemMode::Flash;
    static SYSTEM_MODE_SIGNAL: Signal<ThreadModeRawMutex, SystemMode> = Signal::new();

    static NORMAL_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
    static FLASH_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
    static PRIORITY_A_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
    static PRIORITY_B_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);

    let peripherals = embassy_stm32::init(Default::default());

    static SERIAL: Mutex<ThreadModeRawMutex, Option<Uart<'static, Async>>> =
        Mutex::new(Option::None);
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
    SERIAL.lock().await.replace(uart);

    // The USB serial port takes about 3 seconds to connect when there is
    // traffic. To troubleshoot startup problems it is a good idea to `print()`
    // some messages at startup. We don't do that so that the control loop
    // starts quickly, which makes the system feel fast and reliable.

    let mut outputs: [Output<'_>; Pins::VARIANT_COUNT] = [
        // Left-right lane outputs.
        //
        // Pins::ARed - crossing ribbon / white
        Output::new(peripherals.PE1.degrade(), Level::Low, Speed::Low),
        // Pins::AAmber - crossing ribbon / grey
        Output::new(peripherals.PB9.degrade(), Level::Low, Speed::Low),
        // Pins::AGreen - crossing ribbon / purple
        Output::new(peripherals.PB7.degrade(), Level::Low, Speed::Low),
        // Pins::APedestrianRed - crossing ribbon / brown
        Output::new(peripherals.PD5.degrade(), Level::Low, Speed::Low),
        // Pins::APedestrianGreen - crossing ribbon / black
        Output::new(peripherals.PD7.degrade(), Level::Low, Speed::Low),
        // Pins::APromise - status leds ribbon / orange
        Output::new(peripherals.PE5.degrade(), Level::Low, Speed::Low),
        // Pins::ABeeper - crossing ribbon / purple
        Output::new(peripherals.PD2.degrade(), Level::Low, Speed::Low),
        //
        // Up-down lane outputs.
        //
        // Pins::BRed - crossing ribbon / blue
        Output::new(peripherals.PB6.degrade(), Level::Low, Speed::Low),
        // Pins::BAmber - crossing ribbon / green
        Output::new(peripherals.PB8.degrade(), Level::Low, Speed::Low),
        // Pins::BGreen - crossing ribbon / yellow
        Output::new(peripherals.PE0.degrade(), Level::Low, Speed::Low),
        // Pins::BPedestrianRed - crossing ribbon / amber
        Output::new(peripherals.PB5.degrade(), Level::Low, Speed::Low),
        // Pins::BPedestrianGreen - crossing ribbon / red
        Output::new(peripherals.PD6.degrade(), Level::Low, Speed::Low),
        // Pins::BPromise - status leds ribbon / red
        Output::new(peripherals.PE4.degrade(), Level::Low, Speed::Low),
        // Pins::BBeeper - not connected
        Output::new(peripherals.PC1.degrade(), Level::Low, Speed::Low),
        //
        // Common
        //
        // As an aside: While `LED4` is controllable, Leds `LED1` (power),
        // `LED2` (serial RX) and `LED3` (serial TX) cannot be controlled from
        // code. They have been hardwired on the PCB.
        //
        // Pins::Power - PCB mounted / LED4
        Output::new(peripherals.PE12, Level::Low, Speed::Low),
        // Pins::Power - status leds ribbon / white
        Output::new(peripherals.PE2.degrade(), Level::Low, Speed::Low),
        // Pins::SwitchingMode - status leds ribbon / purple
        Output::new(peripherals.PE3.degrade(), Level::Low, Speed::Low),
    ];

    {
        // scope for the mutex guard...
        let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> = LIGHTS.lock().await;

        lights.set_on_off3(Pins::ARed, true, Pins::AAmber, false, Pins::AGreen, false);
        lights.set_on_off3(Pins::BRed, true, Pins::BAmber, false, Pins::BGreen, false);
        lights.set_on_off2(Pins::APedestrianRed, true, Pins::APedestrianGreen, false);
        lights.set_on_off2(Pins::BPedestrianRed, true, Pins::BPedestrianGreen, false);

        // Make the power leds blink with short bips
        lights.set_pin(Pins::OnBoardPower, true, false, false, true);
        lights.set_pin(Pins::Power, true, false, false, true);
    }

    static SYSTEM_MODE_INPUTS: Mutex<ThreadModeRawMutex, Option<[Input<'static>; 3]>> =
        Mutex::new(Option::None);
    let system_mode_inputs: [Input; 3] = [
        // status rotary ribbon / blue
        Input::new(peripherals.PB14.degrade(), Pull::Up),
        // status rotary ribbon / green
        Input::new(peripherals.PB12.degrade(), Pull::Up),
        // status rotary ribbon / yellow
        Input::new(peripherals.PB10.degrade(), Pull::Up),
    ];
    {
        // scope for the mutex guard...
        SYSTEM_MODE_INPUTS.lock().await.replace(system_mode_inputs);
    }

    static PROMISE_INPUT_A: Mutex<ThreadModeRawMutex, Option<Input<'static>>> = Mutex::new(None);
    static PROMISE_INPUT_B: Mutex<ThreadModeRawMutex, Option<Input<'static>>> = Mutex::new(None);
    // crossing ribbon / gray
    let promise_input_a: Input = Input::new(peripherals.PD3.degrade(), Pull::Up);
    // crossing ribbon / white
    let promise_input_b: Input = Input::new(peripherals.PD4.degrade(), Pull::Up);
    {
        // scope for the mutex guard...
        PROMISE_INPUT_A.lock().await.replace(promise_input_a);
        PROMISE_INPUT_B.lock().await.replace(promise_input_b);
    }

    spawner.must_spawn(normal_mode_task(
        &NORMAL_MODE_SEMAPHORE,
        &TRAFFIC_LIGHTS_A,
        &PEDESTRIAN_LIGHTS_A,
    ));
    spawner.must_spawn(normal_mode_task(
        &NORMAL_MODE_SEMAPHORE,
        &TRAFFIC_LIGHTS_B,
        &PEDESTRIAN_LIGHTS_B,
    ));
    spawner.must_spawn(flash_mode_task(
        &FLASH_MODE_SEMAPHORE,
        &TRAFFIC_LIGHTS_A,
        &TRAFFIC_LIGHTS_B,
        &PEDESTRIAN_LIGHTS_A,
        &PEDESTRIAN_LIGHTS_B,
        &LOCKOUT,
    ));
    spawner.must_spawn(priority_mode_task(
        &PRIORITY_A_SEMAPHORE,
        &TRAFFIC_LIGHTS_A,
        &PEDESTRIAN_LIGHTS_A,
        &LOCKOUT,
    ));
    spawner.must_spawn(priority_mode_task(
        &PRIORITY_B_SEMAPHORE,
        &TRAFFIC_LIGHTS_B,
        &PEDESTRIAN_LIGHTS_B,
        &LOCKOUT,
    ));
    spawner.must_spawn(system_mode_task(
        &SERIAL,
        START_MODE,
        &SYSTEM_MODE_SIGNAL,
        &NORMAL_MODE_SEMAPHORE,
        &FLASH_MODE_SEMAPHORE,
        &PRIORITY_A_SEMAPHORE,
        &PRIORITY_B_SEMAPHORE,
        &LOCKOUT,
    ));
    spawner.must_spawn(system_mode_reader_task(
        &SERIAL,
        &SYSTEM_MODE_INPUTS,
        START_MODE,
        &SYSTEM_MODE_SIGNAL,
    ));
    spawner.must_spawn(promise_input_task(&PROMISE_INPUT_A, &PEDESTRIAN_LIGHTS_A));
    spawner.must_spawn(promise_input_task(&PROMISE_INPUT_B, &PEDESTRIAN_LIGHTS_B));

    loop {
        let output_values: [bool; Pins::VARIANT_COUNT] = {
            // scope for the mutex guard...
            let mut lights: MutexGuard<'_, ThreadModeRawMutex, TimedOutputMasker> =
                LIGHTS.lock().await;

            lights.set_pin(
                Pins::SwitchingMode,
                LOCKOUT.load(Ordering::Relaxed),
                false,
                true,
                false,
            );
            lights.call_at_100_hz()
        };

        for i in 0..Pins::VARIANT_COUNT {
            outputs[i].set_level(if output_values[i] {
                Level::High
            } else {
                Level::Low
            });
        }

        Timer::after_millis(10).await;
    }
}
