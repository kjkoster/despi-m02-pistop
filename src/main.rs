#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

mod io;
use io::{CHANNEL_CAPACITY, Leg, Rag, io_task};

use core::sync::atomic::{AtomicBool, Ordering};
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Channel, Sender},
    semaphore::{FairSemaphore, Semaphore},
    signal::Signal,
};
use embassy_time::Timer;
use panic_halt as _;

const NUM_NORMAL_MODE_TASKS: usize = 2;
const NUM_FLASH_MODE_TASKS: usize = 1;
const NUM_FLASH_BUTTON_TASKS: usize = 1;
// Strictly speaking, the queue in this type is too large for the actual number
// of tasks, but I'd have to calculate the max of the two num_somethings. Maybe
// I'll get round to that some other time,
const NUM_TASKS: usize = NUM_NORMAL_MODE_TASKS + NUM_FLASH_MODE_TASKS + NUM_FLASH_BUTTON_TASKS;

type CrossingSemaphore = FairSemaphore<ThreadModeRawMutex, NUM_TASKS>;
static NORMAL_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(1);
static FLASH_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
static IN_FLASH_MODE: AtomicBool = AtomicBool::new(false);

static RAGS: Channel<ThreadModeRawMutex, Rag, CHANNEL_CAPACITY> = Channel::new();
static BLINKY: Channel<ThreadModeRawMutex, bool, CHANNEL_CAPACITY> = Channel::new();
static ONBOARD_BUTTON_RAW: Signal<ThreadModeRawMutex, bool> = Signal::new();

#[embassy_executor::task(pool_size = NUM_NORMAL_MODE_TASKS)]
async fn normal_mode_task(
    leg: Leg,
    semaphore: &'static CrossingSemaphore,
    rags: Sender<'static, ThreadModeRawMutex, Rag, CHANNEL_CAPACITY>,
) {
    loop {
        // Red Phase
        rags.send(Rag::new(leg, true, false, false)).await;
        Timer::after_millis(10_000).await;

        {
            // we use this scope to safely hold the permit from the semaphore
            // for normal run mode.
            let _permit = semaphore.acquire(1).await.unwrap();

            // Attention Phase
            rags.send(Rag::new(leg, true, true, false)).await;
            Timer::after_millis(1_500).await;

            // Go Phase
            rags.send(Rag::new(leg, false, false, true)).await;
            Timer::after_millis(4_000).await;

            // Yield Phase
            rags.send(Rag::new(leg, false, true, false)).await;
            Timer::after_millis(3_000).await;

            // Clear Crossring Phase
            rags.send(Rag::new(leg, true, false, false)).await;
            Timer::after_millis(2_000).await;

            // _permit is released here...
        }
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_MODE_TASKS)]
async fn flash_mode_task(
    semaphore: &'static CrossingSemaphore,
    in_flash_mode: &'static AtomicBool,
    rags: Sender<'static, ThreadModeRawMutex, Rag, CHANNEL_CAPACITY>,
) {
    loop {
        // Red Phase
        rags.send(Rag::new(Leg::A, true, false, false)).await;
        rags.send(Rag::new(Leg::B, true, false, false)).await;
        Timer::after_millis(1_000).await;

        {
            // we use this scope to safely hold the permit from the semaphore
            // for normal run mode.
            let _permit = semaphore.acquire(1).await.unwrap();

            while in_flash_mode.load(Ordering::Relaxed) {
                // Flash On Phase
                rags.send(Rag::new(Leg::A, false, true, false)).await;
                rags.send(Rag::new(Leg::B, false, true, false)).await;
                Timer::after_millis(1_000).await;

                // Flash Off Phase
                rags.send(Rag::new(Leg::A, false, false, false)).await;
                rags.send(Rag::new(Leg::B, false, false, false)).await;
                Timer::after_millis(1_000).await;
            }

            // Clear Crossring Phase
            rags.send(Rag::new(Leg::A, true, false, false)).await;
            rags.send(Rag::new(Leg::B, true, false, false)).await;
            Timer::after_millis(2_000).await;

            // _permit is released here...
        }
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_BUTTON_TASKS)]
async fn flash_button_task(
    normal_mode_semaphore: &'static CrossingSemaphore,
    flash_mode_semaphore: &'static CrossingSemaphore,
    in_flash_mode: &'static AtomicBool,
    onboard_button_raw: &'static Signal<ThreadModeRawMutex, bool>,
) {
    loop {
        onboard_button_raw.wait().await;

        in_flash_mode.fetch_not(Ordering::Relaxed);

        if in_flash_mode.load(Ordering::Relaxed) {
            normal_mode_semaphore.acquire(1).await.unwrap().disarm();
            flash_mode_semaphore.release(1);
        } else {
            flash_mode_semaphore.acquire(1).await.unwrap().disarm();
            normal_mode_semaphore.release(1);
        }

        // debounce....
        Timer::after_millis(200).await;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) {
    spawner
        .spawn(io_task(
            RAGS.receiver(),
            BLINKY.receiver(),
            &ONBOARD_BUTTON_RAW,
        ))
        .unwrap();
    spawner
        .spawn(normal_mode_task(
            Leg::A,
            &NORMAL_MODE_SEMAPHORE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(normal_mode_task(
            Leg::B,
            &NORMAL_MODE_SEMAPHORE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(flash_mode_task(
            &FLASH_MODE_SEMAPHORE,
            &IN_FLASH_MODE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(flash_button_task(
            &NORMAL_MODE_SEMAPHORE,
            &FLASH_MODE_SEMAPHORE,
            &IN_FLASH_MODE,
            &ONBOARD_BUTTON_RAW,
        ))
        .unwrap();

    // Show and help count seconds by flashing the on-board LED roughly once
    // every second. In normal mode we just flash, in maintenance mode we blink
    // on an off.
    loop {
        BLINKY.send(true).await;
        if IN_FLASH_MODE.load(Ordering::Relaxed) {
            Timer::after_millis(500).await;
        } else {
            Timer::after_millis(15).await;
        }
        BLINKY.send(false).await;
        if IN_FLASH_MODE.load(Ordering::Relaxed) {
            Timer::after_millis(500).await;
        } else {
            Timer::after_millis(985).await;
        }
    }
}
