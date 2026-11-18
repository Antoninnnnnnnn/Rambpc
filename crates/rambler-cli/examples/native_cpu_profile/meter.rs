//! Example-only synchronous poll scopes on the one current-thread executor.
use rambler_engine::{AudioSource, SourceError, SourceFuture};
use std::{cell::Cell, ffi::c_void, future::Future};

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GetCurrentThread() -> *mut c_void;
    fn GetCurrentProcess() -> *mut c_void;
    fn QueryThreadCycleTime(thread: *mut c_void, cycles: *mut u64) -> i32;
    fn GetProcessTimes(
        process: *mut c_void,
        created: *mut [u32; 2],
        exited: *mut [u32; 2],
        kernel: *mut [u32; 2],
        user: *mut [u32; 2],
    ) -> i32;
}

pub fn cycles() -> u64 {
    let mut cycles = 0;
    // SAFETY: The pseudo-handle names only the calling thread and requires no
    // close; the address is writable for the duration of the Windows call.
    let success = unsafe { QueryThreadCycleTime(GetCurrentThread(), &mut cycles) };
    assert_ne!(success, 0, "QueryThreadCycleTime failed");
    cycles
}

pub fn process_cpu_100ns() -> u64 {
    let (mut created, mut exited, mut kernel, mut user) = ([0; 2], [0; 2], [0; 2], [0; 2]);
    // SAFETY: The pseudo-handle names only this process; four distinct output
    // addresses have FILETIME's two-DWORD layout and lifetime.
    let success = unsafe {
        GetProcessTimes(
            GetCurrentProcess(),
            &mut created,
            &mut exited,
            &mut kernel,
            &mut user,
        )
    };
    assert_ne!(success, 0, "GetProcessTimes failed");
    let ticks = |[lo, hi]: [u32; 2]| (u64::from(hi) << 32) | u64::from(lo);
    ticks(kernel) + ticks(user)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Session,
    Decimator,
    OpusOgg,
    Ingress,
}

#[derive(Clone, Copy)]
struct Ledger {
    stage: Stage,
    last: u64,
    totals: [u64; 4],
    entries: [u64; 4],
}

impl Ledger {
    fn new(start: u64) -> Self {
        Self {
            stage: Stage::Session,
            last: start,
            totals: [0; 4],
            entries: [0; 4],
        }
    }

    fn switch(&mut self, next: Stage, now: u64) -> Stage {
        self.totals[self.stage as usize] += now.checked_sub(self.last).unwrap();
        self.last = now;
        std::mem::replace(&mut self.stage, next)
    }
}

thread_local! {
    static LEDGER: Cell<Option<Ledger>> = const { Cell::new(None) };
}

pub fn start(now: u64) {
    assert!(LEDGER.replace(Some(Ledger::new(now))).is_none());
}

pub fn finish(now: u64) -> ([u64; 4], [u64; 4]) {
    let mut ledger = LEDGER.take().unwrap();
    assert_eq!(ledger.switch(Stage::Session, now), Stage::Session);
    (ledger.totals, ledger.entries)
}

fn scope<T>(stage: Stage, f: impl FnOnce() -> T) -> T {
    let mut ledger = LEDGER.get().unwrap();
    let prior = ledger.switch(stage, cycles());
    ledger.entries[stage as usize] += 1;
    LEDGER.set(Some(ledger));
    let out = f();
    let mut ledger = LEDGER.get().unwrap();
    assert_eq!(ledger.switch(prior, cycles()), stage);
    LEDGER.set(Some(ledger));
    out
}

// Count running brackets only; a pending future is stopped before its waker
// returns to Tokio, so waiting and unrelated tasks are not assigned to it.
pub async fn measured<T>(stage: Stage, future: impl Future<Output = T>) -> T {
    let mut future = std::pin::pin!(future);
    std::future::poll_fn(|cx| scope(stage, || future.as_mut().poll(cx))).await
}

pub struct Measured<S>(pub Stage, pub S);

impl<S: AudioSource> AudioSource for Measured<S> {
    fn next(&mut self) -> SourceFuture<'_, Result<Option<bytes::Bytes>, SourceError>> {
        let stage = self.0;
        let future = scope(stage, || self.1.next());
        Box::pin(measured(stage, future))
    }

    fn close(&mut self) -> SourceFuture<'_, Result<(), SourceError>> {
        let stage = self.0;
        let future = scope(stage, || self.1.close());
        Box::pin(measured(stage, future))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_poll_accounting_is_exclusive_and_complete() {
        let mut ledger = Ledger::new(10);
        assert_eq!(ledger.switch(Stage::OpusOgg, 20), Stage::Session);
        assert_eq!(ledger.switch(Stage::Decimator, 30), Stage::OpusOgg);
        assert_eq!(ledger.switch(Stage::Ingress, 40), Stage::Decimator);
        ledger.switch(Stage::Decimator, 45);
        ledger.switch(Stage::OpusOgg, 51);
        ledger.switch(Stage::Session, 60);
        ledger.switch(Stage::Session, 71);
        assert_eq!(ledger.totals, [21, 16, 19, 5]);
    }

    #[test]
    fn windows_counters_are_monotonic() {
        let (thread, process) = (cycles(), process_cpu_100ns());
        assert!(cycles() >= thread && process_cpu_100ns() >= process);
    }
}
