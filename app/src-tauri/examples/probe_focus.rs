//! Diagnostic (not part of the app): print the target verdict for whatever
//! field has the focus, a few times. Reads no field content.
//!
//!   cargo run --example probe_focus -- [delay_seconds] [count]

#[cfg(windows)]
fn main() {
    use sambler_lib::platform::windows::UiaTargetProbe;
    use sambler_lib::ports::TargetProbe;
    let mut args = std::env::args().skip(1);
    let delay: u64 = args.next().and_then(|a| a.parse().ok()).unwrap_or(2);
    let count: u32 = args.next().and_then(|a| a.parse().ok()).unwrap_or(3);
    let probe = UiaTargetProbe::new();
    std::thread::sleep(std::time::Duration::from_secs(delay));
    for i in 0..count {
        let start = std::time::Instant::now();
        let verdict = probe.capture();
        println!("{i}: {verdict:?} ({} ms)", start.elapsed().as_millis());
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

#[cfg(not(windows))]
fn main() {}
