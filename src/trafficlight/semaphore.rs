/*
 * A module that handles all synchonisation and semaphones for the traffic lights.
 */

use embassy_sync::{
    blocking_mutex::raw::ThreadModeRawMutex,
    semaphore::{FairSemaphore, Semaphore},
};

pub const NUM_TRAFFICLIGHTS: usize = 2;
pub const NUM_BUTTON_TASKS: usize = 1;
const NUM_TASKS: usize = NUM_TRAFFICLIGHTS + NUM_BUTTON_TASKS;

type CrossingSemaphore = FairSemaphore<ThreadModeRawMutex, NUM_TASKS>;
static CROSSING_SEMAPHORE: CrossingSemaphore = CrossingSemaphore::new(1);

pub async fn acquire_permit() {
    CROSSING_SEMAPHORE.acquire(1).await.unwrap().disarm();
}

pub fn release_permit() {
    CROSSING_SEMAPHORE.release(1);
}
