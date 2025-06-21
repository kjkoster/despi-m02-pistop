#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

mod io;
use io::{
    Lane, SystemMode, initialise_io, light_lockout, light_pedestrian_lights, light_power,
    light_traffic_lights, print, read_system_mode, toggle_lockout,
};

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    semaphore::{FairSemaphore, Semaphore},
    signal::Signal,
};
use embassy_time::Timer;
use panic_halt as _;

const NUM_NORMAL_MODE_TASKS: usize = 2;
const NUM_FLASH_MODE_TASKS: usize = 1;
const NUM_PRIORITY_TASKS: usize = 2;
const NUM_FLASH_BUTTON_TASKS: usize = 1;
// Strictly speaking, the queue in this type is too large for the actual number
// of tasks, but I'd have to calculate the max of the two num_somethings. Maybe
// I'll get round to that some other time,
const NUM_TASKS: usize =
    NUM_NORMAL_MODE_TASKS + NUM_FLASH_MODE_TASKS + NUM_PRIORITY_TASKS + NUM_FLASH_BUTTON_TASKS;
type CrossingSemaphore = FairSemaphore<ThreadModeRawMutex, NUM_TASKS>;

// When the system starts, we don't know what happened before the shutdown. We
// cannot trust the mode input, since it may be in debounce. Thus, we start in
// lockout mode, so that all traffic on the crossing is cleared and barred from
// entering. Maybe not efficient, but certainly safe.
static LOCKOUT: AtomicBool = AtomicBool::new(true);

#[embassy_executor::task(pool_size = NUM_NORMAL_MODE_TASKS)]
async fn normal_mode_task(lane: Lane, semaphore: &'static CrossingSemaphore) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // Attention Phase
        light_traffic_lights(lane, true, true, false).await;
        light_pedestrian_lights(lane, false, true).await;
        Timer::after_millis(1_500).await;

        // Go Phase
        light_traffic_lights(lane, false, false, true).await;
        Timer::after_millis(4_000).await;

        // Yield Phase
        light_traffic_lights(lane, false, true, false).await;
        light_pedestrian_lights(lane, true, false).await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        light_traffic_lights(lane, true, false, false).await;
        light_pedestrian_lights(lane, true, false).await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_MODE_TASKS)]
async fn flash_mode_task(semaphore: &'static CrossingSemaphore, lockout: &'static AtomicBool) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        light_pedestrian_lights(Lane::A, false, false).await;
        light_pedestrian_lights(Lane::B, false, false).await;

        while !lockout.load(Ordering::Relaxed) {
            // Flash On Phase
            light_traffic_lights(Lane::A, false, true, false).await;
            light_traffic_lights(Lane::B, false, true, false).await;
            Timer::after_millis(1_000).await;

            // Flash Off Phase
            light_traffic_lights(Lane::A, false, false, false).await;
            light_traffic_lights(Lane::B, false, false, false).await;
            Timer::after_millis(1_000).await;
        }

        // Yield Phase
        light_traffic_lights(Lane::A, false, true, false).await;
        light_traffic_lights(Lane::B, false, true, false).await;
        light_pedestrian_lights(Lane::A, true, false).await;
        light_pedestrian_lights(Lane::B, true, false).await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        light_traffic_lights(Lane::A, true, false, false).await;
        light_traffic_lights(Lane::B, true, false, false).await;
        light_pedestrian_lights(Lane::A, true, false).await;
        light_pedestrian_lights(Lane::B, true, false).await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = NUM_PRIORITY_TASKS)]
async fn priority_mode_task(
    lane: Lane,
    semaphore: &'static CrossingSemaphore,
    lockout: &'static AtomicBool,
) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // no pedestrians while emergency services pass
        light_pedestrian_lights(lane, true, false).await;

        // Attention Phase
        light_traffic_lights(lane, true, true, false).await;
        Timer::after_millis(1_500).await;

        // Go Phase
        light_traffic_lights(lane, false, false, true).await;
        Timer::after_millis(4_000).await;

        // crude...
        while !lockout.load(Ordering::Relaxed) {
            Timer::after_millis(500).await;
        }

        // Yield Phase
        light_traffic_lights(lane, false, true, false).await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        light_traffic_lights(lane, true, false, false).await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_BUTTON_TASKS)]
async fn system_mode_reader_task(
    initial_mode: SystemMode,
    system_mode_signal: &'static Signal<ThreadModeRawMutex, SystemMode>,
) -> ! {
    let mut current_mode: SystemMode = initial_mode;
    loop {
        print("mode rdr:                     awaiting user action.\r\n").await;
        #[allow(unused_assignments)]
        let mut new_mode = current_mode;
        'await_change: loop {
            Timer::after_millis(200).await;
            new_mode = read_system_mode().await;
            if new_mode != current_mode {
                print("mode rdr:                     breaking await user action.\r\n").await;
                break 'await_change;
            }
        }

        print("mode rdr:                     awaiting debounce.\r\n").await;
        'await_debounce: loop {
            Timer::after_millis(1_000).await;
            let debounced_mode: SystemMode = read_system_mode().await;
            if debounced_mode == new_mode {
                print("mode rdr:                     breaking debounce.\r\n").await;
                break 'await_debounce;
            } else {
                new_mode = debounced_mode;
            }
        }

        // suppress signalling if there is no actual change. This reduces the
        // chance of glitches due to quick mode switches.
        if current_mode != new_mode {
            current_mode = new_mode;
            match current_mode {
                SystemMode::Normal => {
                    print("mode rdr:                     signalling SystemMode::Normal.\r\n").await
                }
                SystemMode::Flash => {
                    print("mode rdr:                     signalling SystemMode::Flash.\r\n").await
                }
                SystemMode::PriorityA => {
                    print("mode rdr:                     signalling SystemMode::PriorityA.\r\n")
                        .await
                }
                SystemMode::PriorityB => {
                    print("mode rdr:                     signalling SystemMode::PriorityB.\r\n")
                        .await
                }
            }
            system_mode_signal.signal(current_mode);
        }
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_BUTTON_TASKS)]
async fn system_mode_task(
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
        print("sem hand: releasing lockout.\r\n").await;
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
                print("sem hand: releasing SystemMode::Normal.\r\n").await;
                ensure_released(&mut have_normal_permit, normal_mode_semaphore);
            }
            SystemMode::Flash => {
                print("sem hand: releasing SystemMode::Flash.\r\n").await;
                ensure_released(&mut have_flash_permit, flash_mode_semaphore);
            }
            SystemMode::PriorityA => {
                print("sem hand: releasing SystemMode::PriorityA.\r\n").await;
                ensure_released(&mut have_priority_a_permit, priority_a_semaphore);
            }
            SystemMode::PriorityB => {
                print("sem hand: releasing SystemMode::PriorityB.\r\n").await;
                ensure_released(&mut have_priority_b_permit, priority_b_semaphore);
            }
        }

        print("sem hand: awaiting new mode.\r\n").await;
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

        print("sem hand: locking out.\r\n").await;
        lockout.store(true, Ordering::Relaxed);

        print("sem hand: collecting semaphores.\r\n").await;
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

#[embassy_executor::task]
async fn lockout_led_task(lockout: &'static AtomicBool) -> ! {
    loop {
        Timer::after_millis(50).await;

        match lockout.load(Ordering::Relaxed) {
            false => light_lockout(false).await,
            true => toggle_lockout().await,
        }
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    initialise_io(true, false, false, false, false).await;
    print("I/O initialised.\r\n").await;

    const START_MODE: SystemMode = SystemMode::Normal;
    static SYSTEM_MODE_SIGNAL: Signal<ThreadModeRawMutex, SystemMode> = Signal::new();

    static NORMAL_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
    static FLASH_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
    static PRIORITY_A_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
    static PRIORITY_B_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);

    spawner
        .spawn(normal_mode_task(Lane::A, &NORMAL_MODE_SEMAPHORE))
        .unwrap();
    spawner
        .spawn(normal_mode_task(Lane::B, &NORMAL_MODE_SEMAPHORE))
        .unwrap();
    spawner
        .spawn(priority_mode_task(Lane::A, &PRIORITY_A_SEMAPHORE, &LOCKOUT))
        .unwrap();
    spawner
        .spawn(priority_mode_task(Lane::B, &PRIORITY_B_SEMAPHORE, &LOCKOUT))
        .unwrap();
    spawner
        .spawn(flash_mode_task(&FLASH_MODE_SEMAPHORE, &LOCKOUT))
        .unwrap();
    spawner
        .spawn(system_mode_task(
            START_MODE,
            &SYSTEM_MODE_SIGNAL,
            &NORMAL_MODE_SEMAPHORE,
            &FLASH_MODE_SEMAPHORE,
            &PRIORITY_A_SEMAPHORE,
            &PRIORITY_B_SEMAPHORE,
            &LOCKOUT,
        ))
        .unwrap();
    spawner
        .spawn(system_mode_reader_task(START_MODE, &SYSTEM_MODE_SIGNAL))
        .unwrap();
    spawner.spawn(lockout_led_task(&LOCKOUT)).unwrap();

    // Help count seconds by flashing the power and on-board leds roughly once
    // every second. This demonstrates liveness of the system as a whole.
    loop {
        light_power(true).await;
        Timer::after_millis(15).await;

        light_power(false).await;
        Timer::after_millis(985).await;
    }
}
