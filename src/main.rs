#![no_std]
#![no_main]

// https://dev.to/theembeddedrustacean/embedded-rust-embassy-gpio-button-controlled-blinking-3ee6
// https://www.youtube.com/watch?v=dab_vzVDr_M

mod io;
use io::{CHANNEL_CAPACITY, Lane, Rag, debounce_task, io_task};

use atomic_enum::atomic_enum;
use core::sync::atomic::Ordering;
use embassy_executor::Spawner;
use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    channel::{Channel, Receiver, Sender},
    semaphore::{FairSemaphore, Semaphore},
};
use embassy_time::{Duration, Timer};
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
static NORMAL_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
static FLASH_MODE_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
static PRIORITY_A_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);
static PRIORITY_B_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(0);

#[atomic_enum]
#[derive(PartialEq, Eq)]
enum SystemMode {
    Normal,
    Flash,
    PriorityA,
    PriorityB,
}
static SYSTEM_MODE: AtomicSystemMode = AtomicSystemMode::new(SystemMode::Normal);
impl AtomicSystemMode {
    fn to_next_mode(&self) {
        match self.load(Ordering::Relaxed) {
            SystemMode::Normal => self.store(SystemMode::Flash, Ordering::Relaxed),
            SystemMode::Flash => self.store(SystemMode::PriorityA, Ordering::Relaxed),
            SystemMode::PriorityA => self.store(SystemMode::PriorityB, Ordering::Relaxed),
            SystemMode::PriorityB => self.store(SystemMode::Normal, Ordering::Relaxed),
        }
    }
}

static RAGS: Channel<ThreadModeRawMutex, Rag, CHANNEL_CAPACITY> = Channel::new();
static BLINKY: Channel<ThreadModeRawMutex, bool, CHANNEL_CAPACITY> = Channel::new();
static ONBOARD_BUTTON_RAW: Channel<ThreadModeRawMutex, bool, CHANNEL_CAPACITY> = Channel::new();
static ONBOARD_BUTTON: Channel<ThreadModeRawMutex, bool, CHANNEL_CAPACITY> = Channel::new();

#[embassy_executor::task(pool_size = NUM_NORMAL_MODE_TASKS)]
async fn normal_mode_task(
    lane: Lane,
    semaphore: &'static CrossingSemaphore,
    rags: Sender<'static, ThreadModeRawMutex, Rag, CHANNEL_CAPACITY>,
) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // Attention Phase
        rags.send(Rag::new(lane, true, true, false)).await;
        Timer::after_millis(1_500).await;

        // Go Phase
        rags.send(Rag::new(lane, false, false, true)).await;
        Timer::after_millis(4_000).await;

        // Yield Phase
        rags.send(Rag::new(lane, false, true, false)).await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        rags.send(Rag::new(lane, true, false, false)).await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_MODE_TASKS)]
async fn flash_mode_task(
    semaphore: &'static CrossingSemaphore,
    system_mode: &'static AtomicSystemMode,
    rags: Sender<'static, ThreadModeRawMutex, Rag, CHANNEL_CAPACITY>,
) -> ! {
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        while system_mode.load(Ordering::Relaxed) == SystemMode::Flash {
            // Flash On Phase
            rags.send(Rag::new(Lane::A, false, true, false)).await;
            rags.send(Rag::new(Lane::B, false, true, false)).await;
            Timer::after_millis(1_000).await;

            // Flash Off Phase
            rags.send(Rag::new(Lane::A, false, false, false)).await;
            rags.send(Rag::new(Lane::B, false, false, false)).await;
            Timer::after_millis(1_000).await;
        }

        // Yield Phase
        rags.send(Rag::new(Lane::A, false, true, false)).await;
        rags.send(Rag::new(Lane::B, false, true, false)).await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        rags.send(Rag::new(Lane::A, true, false, false)).await;
        rags.send(Rag::new(Lane::B, true, false, false)).await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = NUM_PRIORITY_TASKS)]
async fn priority_mode_task(
    lane: Lane,
    semaphore: &'static CrossingSemaphore,
    system_mode: &'static AtomicSystemMode,
    rags: Sender<'static, ThreadModeRawMutex, Rag, CHANNEL_CAPACITY>,
) -> ! {
    let my_mode = if lane == Lane::A {
        SystemMode::PriorityA
    } else {
        SystemMode::PriorityB
    };
    loop {
        // we use this scope to safely hold the permit from the semaphore
        // for normal run mode.
        let _permit = semaphore.acquire(1).await.unwrap();

        // Attention Phase
        rags.send(Rag::new(lane, true, true, false)).await;
        Timer::after_millis(1_500).await;

        // Go Phase
        rags.send(Rag::new(lane, false, false, true)).await;
        Timer::after_millis(4_000).await;

        // crude...
        while system_mode.load(Ordering::Relaxed) == my_mode {
            Timer::after_millis(500).await;
        }

        // Yield Phase
        rags.send(Rag::new(lane, false, true, false)).await;
        Timer::after_millis(3_000).await;

        // Clear Crossring Phase
        rags.send(Rag::new(lane, true, false, false)).await;
        Timer::after_millis(2_000).await;

        // _permit is released here...
    }
}

#[embassy_executor::task(pool_size = NUM_FLASH_BUTTON_TASKS)]
async fn flash_button_task(
    normal_mode_semaphore: &'static CrossingSemaphore,
    flash_mode_semaphore: &'static CrossingSemaphore,
    priority_a_semaphore: &'static CrossingSemaphore,
    priority_b_semaphore: &'static CrossingSemaphore,
    system_mode: &'static AtomicSystemMode,
    onboard_button: Receiver<'static, ThreadModeRawMutex, bool, CHANNEL_CAPACITY>,
) -> ! {
    // As we start, we hold all the permits. This effectively blocks the traffic
    // light tasks from running, as they will be waiting for a permit to become
    // available. Permits are represented as boolean values, since we can only
    // ever have or have not one.
    let mut have_normal_permit: bool = true;
    let mut have_flash_permit: bool = true;
    let mut have_priority_a_permit: bool = true;
    let mut have_priority_b_permit: bool = true;

    loop {
        // At the program start, or after the mode is switched, we have to
        // re-juggle the permits so that we block and unblock the right tasks.
        // In the process of doing so, we have to be careful not to lose any
        // permits (which is why they are represented as boolean values), or to
        // allow tasks to overlap. To avoid overlap, we first try to collect any
        // missing permits before releasing anything.

        match system_mode.load(Ordering::Relaxed) {
            SystemMode::Normal => {
                ensure_aquired(&mut have_flash_permit, flash_mode_semaphore).await;
                ensure_aquired(&mut have_priority_a_permit, priority_a_semaphore).await;
                ensure_aquired(&mut have_priority_b_permit, priority_b_semaphore).await;
                ensure_released(&mut have_normal_permit, normal_mode_semaphore);
            }
            SystemMode::Flash => {
                ensure_aquired(&mut have_normal_permit, normal_mode_semaphore).await;
                ensure_aquired(&mut have_priority_a_permit, priority_a_semaphore).await;
                ensure_aquired(&mut have_priority_b_permit, priority_b_semaphore).await;
                ensure_released(&mut have_flash_permit, flash_mode_semaphore);
            }
            SystemMode::PriorityA => {
                ensure_aquired(&mut have_normal_permit, normal_mode_semaphore).await;
                ensure_aquired(&mut have_flash_permit, flash_mode_semaphore).await;
                ensure_aquired(&mut have_priority_b_permit, priority_b_semaphore).await;
                ensure_released(&mut have_priority_a_permit, priority_a_semaphore);
            }
            SystemMode::PriorityB => {
                ensure_aquired(&mut have_normal_permit, normal_mode_semaphore).await;
                ensure_aquired(&mut have_flash_permit, flash_mode_semaphore).await;
                ensure_aquired(&mut have_priority_a_permit, priority_a_semaphore).await;
                ensure_released(&mut have_priority_b_permit, priority_b_semaphore);
            }
        }

        _ = onboard_button.receive().await;
        system_mode.to_next_mode();
    }
}

async fn ensure_aquired(permit: &mut bool, semaphore: &'static CrossingSemaphore) {
    if !*permit {
        semaphore.acquire(1).await.unwrap().disarm();
        *permit = true;
    }
}
fn ensure_released(permit: &mut bool, semaphore: &'static CrossingSemaphore) {
    if *permit {
        semaphore.release(1);
        *permit = false;
    }
}

#[embassy_executor::main]
async fn main(spawner: Spawner) -> ! {
    spawner
        .spawn(io_task(
            RAGS.receiver(),
            BLINKY.receiver(),
            ONBOARD_BUTTON_RAW.sender(),
        ))
        .unwrap();
    spawner
        .spawn(debounce_task(
            ONBOARD_BUTTON_RAW.receiver(),
            ONBOARD_BUTTON.sender(),
            Duration::from_millis(500),
        ))
        .unwrap();
    spawner
        .spawn(normal_mode_task(
            Lane::A,
            &NORMAL_MODE_SEMAPHORE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(normal_mode_task(
            Lane::B,
            &NORMAL_MODE_SEMAPHORE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(priority_mode_task(
            Lane::A,
            &PRIORITY_A_SEMAPHORE,
            &SYSTEM_MODE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(priority_mode_task(
            Lane::B,
            &PRIORITY_B_SEMAPHORE,
            &SYSTEM_MODE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(flash_mode_task(
            &FLASH_MODE_SEMAPHORE,
            &SYSTEM_MODE,
            RAGS.sender(),
        ))
        .unwrap();
    spawner
        .spawn(flash_button_task(
            &NORMAL_MODE_SEMAPHORE,
            &FLASH_MODE_SEMAPHORE,
            &PRIORITY_A_SEMAPHORE,
            &PRIORITY_B_SEMAPHORE,
            &SYSTEM_MODE,
            ONBOARD_BUTTON.receiver(),
        ))
        .unwrap();

    // Help count seconds by flashing the on-board LED roughly once every
    // second. In normal mode we just flash, in maintenance mode we blink on and
    // off.
    loop {
        BLINKY.send(true).await;
        match SYSTEM_MODE.load(Ordering::Relaxed) {
            SystemMode::Normal => {
                Timer::after_millis(15).await;
            }
            SystemMode::Flash => {
                Timer::after_millis(500).await;
            }
            SystemMode::PriorityA => {
                Timer::after_millis(200).await;
            }
            SystemMode::PriorityB => {
                Timer::after_millis(50).await;
            }
        }

        BLINKY.send(false).await;
        match SYSTEM_MODE.load(Ordering::Relaxed) {
            SystemMode::Normal => {
                Timer::after_millis(985).await;
            }
            SystemMode::Flash => {
                Timer::after_millis(500).await;
            }
            SystemMode::PriorityA => {
                Timer::after_millis(200).await;
            }
            SystemMode::PriorityB => {
                Timer::after_millis(50).await;
            }
        }
    }
}
