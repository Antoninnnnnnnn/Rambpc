//! Opt-in Windows clipboard test; never sends keyboard input or reads user text into logs.

#[cfg(windows)]
mod win {
    use sambler_lib::ports::ClipboardError;
    use windows::{
        Win32::System::{
            DataExchange::IsClipboardFormatAvailable,
            Memory::GMEM_ZEROINIT,
            Ole::{CF_DIB, CF_HDROP},
        },
        core::HSTRING,
    };

    // Exercise the exact production transaction with deterministic native formats.
    // This exposes no clipboard snapshot API in the shipping library.
    #[allow(dead_code)]
    mod production {
        include!("../src/platform/windows/clipboard.rs");

        pub(super) fn test() -> Result<(), ClipboardError> {
            use super::*;
            let marker_name =
                HSTRING::from(format!("SamblerClipboardFixture{}", std::process::id()));
            let marker = unsafe { RegisterClipboardFormatW(PCWSTR(marker_name.as_ptr())) };
            if marker == 0 {
                return Err(ClipboardError::Other);
            }
            let owner = Owner::new()?;
            let opening = Opening::new(&owner)?;
            // The test must be stricter than paste about preserving the pre-test
            // clipboard: skip the campaign rather than drop any prior GDI format.
            if [
                CF_BITMAP,
                CF_PALETTE,
                CF_ENHMETAFILE,
                CF_METAFILEPICT,
                CF_DSPBITMAP,
                CF_DSPENHMETAFILE,
                CF_DSPMETAFILEPICT,
            ]
            .iter()
            .any(|f| unsafe { IsClipboardFormatAvailable(u32::from(f.0)) }.is_ok())
            {
                return Err(ClipboardError::Other);
            }
            let original = Snapshot::read(&opening)?;
            opening.close()?;

            struct Scope {
                owner: Owner,
                original: Snapshot,
                marker: u32,
            }
            impl Scope {
                fn seed(&self, format: u32, mut block: GlobalBlock) -> Result<(), ClipboardError> {
                    let mut tag = GlobalBlock::new(&[0u32], false)?;
                    let opening = Opening::new(&self.owner)?;
                    if sequence() != self.original.sequence && !self.has_tag() {
                        return Err(ClipboardError::Other);
                    }
                    unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Other)?;
                    tag.transfer(self.marker)?;
                    block.transfer(format)?;
                    opening.close()
                }
                fn has_tag(&self) -> bool {
                    unsafe { IsClipboardFormatAvailable(self.marker) }.is_ok()
                }
                fn read(&self, format: u32) -> Result<Vec<u8>, ClipboardError> {
                    let opening = Opening::new(&self.owner)?;
                    let handle = HGLOBAL(
                        unsafe { GetClipboardData(format) }
                            .map_err(|_| ClipboardError::Other)?
                            .0,
                    );
                    let size = unsafe { GlobalSize(handle) };
                    if size == 0 || size > 1024 {
                        return Err(ClipboardError::Other);
                    }
                    let buffer = unsafe { GlobalLock(handle) }.cast::<u8>();
                    if buffer.is_null() {
                        return Err(ClipboardError::Other);
                    }
                    let read = unsafe { std::slice::from_raw_parts(buffer, size) }.to_vec();
                    let _ = unsafe { GlobalUnlock(handle) };
                    opening.close()?;
                    Ok(read)
                }
            }
            impl Drop for Scope {
                fn drop(&mut self) {
                    if let Ok(opening) = Opening::new(&self.owner)
                        && self.has_tag()
                    {
                        self.original.restore(&opening);
                    }
                }
            }
            let scope = Scope {
                owner,
                original,
                marker,
            };
            let unicode = u32::from(CF_UNICODETEXT.0);
            let text = |s: &str| s.encode_utf16().chain([0]).collect::<Vec<_>>();
            let mut dib = vec![0u8; 44];
            dib[0..4].copy_from_slice(&40u32.to_le_bytes()); // BITMAPINFOHEADER
            dib[4..8].copy_from_slice(&1i32.to_le_bytes()); // width
            dib[8..12].copy_from_slice(&1i32.to_le_bytes()); // height
            dib[12..14].copy_from_slice(&1u16.to_le_bytes()); // planes
            dib[14..16].copy_from_slice(&24u16.to_le_bytes()); // bits per pixel
            dib[20..24].copy_from_slice(&4u32.to_le_bytes()); // padded row size
            dib[40..43].copy_from_slice(&[80, 40, 20]);
            let mut drop_files = vec![0u8; 20];
            drop_files[0..4].copy_from_slice(&20u32.to_le_bytes()); // pFiles
            drop_files[16..20].copy_from_slice(&1u32.to_le_bytes()); // fWide
            for word in text("C:\\fixture-a.txt")
                .into_iter()
                .chain(text("C:\\fixture-b.txt"))
                .chain([0])
            {
                drop_files.extend_from_slice(&word.to_le_bytes());
            }
            let prior_text: Vec<u8> = text("Sambler test prior clipboard é🦀")
                .into_iter()
                .flat_map(u16::to_le_bytes)
                .collect();
            let temporary = text("Sambler test paste\r\nsecond line");
            let temporary_bytes: Vec<u8> = temporary
                .iter()
                .copied()
                .flat_map(u16::to_le_bytes)
                .collect();
            for (label, format, data) in [
                ("text", unicode, prior_text),
                ("DIB", u32::from(CF_DIB.0), dib),
                ("HDROP", u32::from(CF_HDROP.0), drop_files),
            ] {
                scope.seed(format, GlobalBlock::new(&data, false)?)?;
                let before = scope.read(format)?;
                let mut waits = Vec::with_capacity(20);
                for _ in 0..20 {
                    let staged = PasteClipboard::stage(&temporary)?;
                    let actual = scope.read(unicode)?;
                    assert!(
                        actual.starts_with(&temporary_bytes),
                        "temporary fixture text mismatch"
                    );
                    for name in [
                        w!("ExcludeClipboardContentFromMonitorProcessing"),
                        w!("CanIncludeInClipboardHistory"),
                        w!("CanUploadToCloudClipboard"),
                    ] {
                        let privacy_format = unsafe { RegisterClipboardFormatW(name) };
                        assert!(
                            scope.read(privacy_format)?.starts_with(&[0; 4]),
                            "privacy bytes mismatch"
                        );
                    }
                    let now = Instant::now();
                    staged.wait_for_paste();
                    drop(staged);
                    waits.push(now.elapsed());
                    assert!(
                        scope.read(format)? == before && scope.has_tag(),
                        "restored fixture bytes mismatch"
                    );
                }
                waits.sort_unstable();
                println!(
                    "clipboard {label}: exact=20/20, privacy=3/3 each, wait_ms={:.1}/{:.1} p50/p95",
                    waits[9].as_secs_f64() * 1000.0,
                    waits[18].as_secs_f64() * 1000.0
                );
            }
            let staged = PasteClipboard::stage(&temporary)?;
            assert!(
                scope.read(unicode)?.starts_with(&temporary_bytes),
                "fixture reader did not see staged text"
            );
            let replacement = text("new fixture copy");
            let replacement_bytes: Vec<u8> = replacement
                .iter()
                .copied()
                .flat_map(u16::to_le_bytes)
                .collect();
            // A separate clipboard owner writes after the test reader consumed
            // the staged text and before Sambler restores. Keep our test marker
            // so the outer guard can restore the pre-test clipboard afterwards.
            let writer = Owner::new()?;
            let opening = Opening::new(&writer)?;
            assert!(
                staged.is_current(),
                "fixture lost clipboard to another writer"
            );
            unsafe { EmptyClipboard() }.map_err(|_| ClipboardError::Other)?;
            GlobalBlock::new(&[0u32], false)?.transfer(marker)?;
            GlobalBlock::new(&replacement, false)?.transfer(unicode)?;
            opening.close()?;
            staged.wait_for_paste();
            drop(staged);
            assert!(
                scope.read(unicode)?.starts_with(&replacement_bytes),
                "concurrent fixture copy lost"
            );
            println!("clipboard concurrent writer: kept after staged text was read");

            // Preallocated >64 MiB HGLOBAL must stay intact and suppress paste.
            let large = GlobalBlock {
                handle: Some(
                    unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, MAX_SNAPSHOT_BYTES + 1) }
                        .map_err(|_| ClipboardError::Other)?,
                ),
                len: MAX_SNAPSHOT_BYTES + 1,
                plaintext: false,
            };
            let large_format =
                unsafe { RegisterClipboardFormatW(w!("SamblerOfflineOversizeFixture")) };
            if large_format == 0 {
                return Err(ClipboardError::Other);
            }
            scope.seed(large_format, large)?;
            let original_sequence = sequence();
            assert!(
                PasteClipboard::stage(&temporary).is_err() && sequence() == original_sequence,
                "oversize refused after clipboard mutation"
            );
            println!("clipboard >64 MiB: refused without mutation");
            Ok(())
        }
    }

    pub fn run() -> Result<(), ClipboardError> {
        production::test()
    }
}

#[cfg(windows)]
use sambler_lib::ports;

#[cfg(windows)]
fn main() -> Result<(), ports::ClipboardError> {
    if std::env::args().nth(1).as_deref() != Some("--isolated-clipboard-test") {
        eprintln!(
            "Opt in with --isolated-clipboard-test when there are no competing clipboard tests."
        );
        return Ok(());
    }
    win::run()
}

#[cfg(not(windows))]
fn main() {
    eprintln!("Windows fixture only");
}
