pub mod backend;

#[cfg(target_os = "macos")]
pub mod macos;

#[cfg(target_os = "macos")]
pub mod macos_tap;

#[cfg(target_os = "linux")]
pub mod linux;

#[cfg(target_os = "windows")]
pub mod windows;

pub use backend::AudioBackend;

/// Create the platform-appropriate audio backend.
pub fn create_backend() -> anyhow::Result<Box<dyn AudioBackend>> {
    #[cfg(target_os = "macos")]
    {
        Ok(Box::new(macos::CoreAudioBackend::new()?))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(Box::new(linux::PulseAudioBackend::new()?))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(Box::new(windows::WasapiBackend::new()?))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        anyhow::bail!("Unsupported platform")
    }
}
