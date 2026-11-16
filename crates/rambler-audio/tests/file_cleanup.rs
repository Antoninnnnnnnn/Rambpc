// Descriptor-release coverage is only available on these platforms.
#![cfg(any(windows, target_os = "linux", target_os = "macos"))]

mod common;
use common::*;
use rambler_audio::*;
use rambler_engine::AudioSource;

#[cfg(windows)]
fn assert_released(path: &std::path::Path) {
    use std::os::windows::fs::OpenOptionsExt;
    // No sharing is permitted: success proves the earlier read handle is closed,
    // not merely scheduled for release after a Tokio blocking operation.
    drop(
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(path)
            .unwrap(),
    );
}
#[cfg(any(target_os = "linux", target_os = "macos"))]
// libc's stat field widths differ across platforms; MetadataExt exposes u64.
#[allow(clippy::unnecessary_cast)]
fn open_descriptors(path: &std::path::Path) -> usize {
    // Linux's large-file API also handles unrelated large files/inodes on 32-bit.
    #[cfg(target_os = "macos")]
    use libc::{fstat, stat as Stat};
    #[cfg(target_os = "linux")]
    use libc::{fstat64 as fstat, stat64 as Stat};
    use std::os::unix::fs::MetadataExt;

    let expected = std::fs::metadata(path).expect("cannot stat descriptor-audit target");
    assert!(expected.is_file(), "descriptor-audit target must be a file");
    #[cfg(target_os = "linux")]
    let directory = "/proc/self/fd";
    #[cfg(target_os = "macos")]
    let directory = "/dev/fd";

    std::fs::read_dir(directory)
        .unwrap_or_else(|error| panic!("cannot enumerate {directory}: {error}"))
        .filter(|entry| {
            let entry = entry.as_ref().expect("cannot enumerate descriptor entry");
            let name = entry.file_name();
            let fd: libc::c_int = name
                .to_str()
                .expect("descriptor name is not UTF-8")
                .parse()
                .expect("descriptor name is not an integer");
            assert!(fd >= 0, "descriptor must be nonnegative");
            let mut stat = std::mem::MaybeUninit::<Stat>::uninit();
            // SAFETY: stat points to writable storage of the correct type. fstat
            // only inspects metadata: it neither follows paths nor reads the fd.
            if unsafe { fstat(fd, stat.as_mut_ptr()) } != 0 {
                let error = std::io::Error::last_os_error();
                // Other test threads may close descriptors during enumeration.
                if error.raw_os_error() == Some(libc::EBADF) {
                    return false;
                }
                panic!("cannot stat descriptor {fd} from {directory}: {error}");
            }
            // SAFETY: successful fstat initialized stat; failure returned above.
            let stat = unsafe { stat.assume_init() };
            stat.st_dev as u64 == expected.dev() && stat.st_ino as u64 == expected.ino()
        })
        .count()
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn assert_released(path: &std::path::Path) {
    assert_eq!(open_descriptors(path), 0, "WAV descriptor still open");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn descriptor_audit_detects_an_independently_opened_file_and_its_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("descriptor-control");
    std::fs::write(&path, b"descriptor control").unwrap();
    assert_eq!(open_descriptors(&path), 0);
    let file = std::fs::File::open(&path).unwrap();
    assert_eq!(open_descriptors(&path), 1, "open control file not found");
    drop(file);
    assert_released(&path);
}

#[tokio::test]
async fn wav_close_awaits_inflight_reads_even_when_next_was_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("pending.wav");
    tokio::fs::write(&path, wav(&vec![0; MAX_INPUT_BYTES]))
        .await
        .unwrap();
    let mut audio = WavAudio::open(
        &path,
        WavConfig {
            chunk_ms: MAX_INPUT_BYTES as u32 / 32,
            realtime: false,
        },
    )
    .await
    .unwrap();
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    assert!(open_descriptors(&path) > 0, "open WAV descriptor not found");
    {
        let mut next = audio.next();
        // Start a real Tokio blocking read, then drop its caller's future.
        let _ = futures_util::poll!(&mut next);
    }
    audio.close().await.unwrap();
    audio.close().await.unwrap();
    assert!(audio.next().await.unwrap().is_none());
    assert_released(&path);
}

#[tokio::test]
async fn eof_header_error_and_data_error_release_wav_descriptor_before_return() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.wav");
    tokio::fs::write(&path, wav(&pcm(321))).await.unwrap();
    let mut audio = WavAudio::open(&path, WavConfig::default()).await.unwrap();
    collect(&mut audio).await.unwrap();
    assert_released(&path);
    tokio::fs::write(&path, b"bad header").await.unwrap();
    assert!(WavAudio::open(&path, WavConfig::default()).await.is_err());
    assert_released(&path);
    let data = wav(&pcm(321));
    tokio::fs::write(&path, &data).await.unwrap();
    let mut audio = WavAudio::open(&path, WavConfig::default()).await.unwrap();
    {
        let file = tokio::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .await
            .unwrap();
        file.set_len(44).await.unwrap();
    }
    assert!(audio.next().await.is_err());
    assert_released(&path);
    audio.close().await.unwrap();
    assert_released(&path);
}

#[tokio::test]
async fn an_unpolled_close_still_stops_further_wav_reads() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("stop.wav");
    tokio::fs::write(&path, wav(&pcm(321))).await.unwrap();
    let mut audio = WavAudio::open(&path, WavConfig::default()).await.unwrap();
    drop(audio.close());
    assert!(audio.next().await.unwrap().is_none());
    assert_released(&path);
}
