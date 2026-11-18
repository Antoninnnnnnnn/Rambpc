//! Windows release CPU attribution for the full native session. Synthetic data,
//! an owned ephemeral IPv4 localhost peer, no credentials, device or provider.
//! Use tools/native_cpu_profile_windows.py for the gated experiment.

#[cfg(windows)]
#[path = "native_cpu_profile/run.rs"]
mod run;

fn main() {
    #[cfg(windows)]
    run::main();
    #[cfg(not(windows))]
    {
        eprintln!("native_cpu_profile requires Windows");
        std::process::exit(2);
    }
}
