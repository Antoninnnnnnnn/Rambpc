//! Voluntary two-second microphone test. Stores no audio.

#[cfg(windows)]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use sambler_lib::{
        model::MicChoice,
        platform::windows::WasapiMicrophone,
        ports::{Microphone, PcmSink},
    };
    use std::{sync::mpsc, thread, time::Duration};

    #[derive(Default)]
    struct Counts {
        frames: usize,
        callbacks: usize,
        smallest: usize,
        largest: usize,
        peak: u16,
    }
    struct Counter(Counts, mpsc::Sender<Option<Counts>>);
    impl PcmSink for Counter {
        fn push(&mut self, samples: &[i16]) {
            let len = samples.len();
            let counts = &mut self.0;
            counts.frames += len;
            counts.smallest = if counts.callbacks == 0 {
                len
            } else {
                counts.smallest.min(len)
            };
            counts.largest = counts.largest.max(len);
            counts.callbacks += 1;
            counts.peak = counts
                .peak
                .max(samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0));
        }
        fn finish(self: Box<Self>) {
            let _ = self.1.send(Some(self.0));
        }
        fn abort(self: Box<Self>) {
            let _ = self.1.send(None);
        }
    }

    let mic = WasapiMicrophone::new();
    let inputs = mic.inputs()?;
    println!("capture endpoints: {}", inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        // The identifiers are usable by the application but unnecessary in logs.
        println!(
            "  {}. {}{}",
            i + 1,
            input.name.escape_default(),
            if input.is_default { " [default]" } else { "" }
        );
    }
    let (counts, summary) = mpsc::channel();
    let (fault_tx, fault_rx) = mpsc::channel();
    let (stream, format) = mic.open_with_format(
        &MicChoice::SystemDefault,
        Box::new(move |_| Box::new(Counter(Counts::default(), counts))),
        Box::new(move |error| {
            let _ = fault_tx.send(error);
        }),
    )?;
    println!(
        "shared format: {} Hz, {} ch, {}; output: {} Hz mono PCM16",
        format.sample_rate,
        format.channels,
        format.sample_format,
        format.output_rate.hz()
    );
    thread::sleep(Duration::from_secs(2));
    stream.stop();
    if let Ok(error) = fault_rx.try_recv() {
        return Err(error.into());
    }
    let counts = summary
        .recv_timeout(Duration::from_secs(1))?
        .ok_or("capture aborted")?;
    println!(
        "frames: {}; sink callbacks: {}; callback frames: {}..={}; peak: {}/32768",
        counts.frames, counts.callbacks, counts.smallest, counts.largest, counts.peak
    );
    assert!(counts.frames > 0, "no PCM frames");
    Ok(())
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This harness requires Windows.");
}
