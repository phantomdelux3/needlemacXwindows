//! Real-time output level of the default speakers/headphones, used to drive
//! the visualizer bars and the beat glow. Uses the same meter Windows' own
//! volume mixer uses, so no audio is captured or recorded.

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{eConsole, eRender, IMMDeviceEnumerator, MMDeviceEnumerator};
use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_ALL, COINIT_MULTITHREADED};

fn meter() -> windows::core::Result<IAudioMeterInformation> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole)?;
        dev.Activate(CLSCTX_ALL, None)
    }
}

pub fn spawn(app: AppHandle) {
    std::thread::spawn(move || {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut m: Option<IAudioMeterInformation> = None;
        let mut n: u32 = 0;
        let mut silent = false;
        loop {
            std::thread::sleep(Duration::from_millis(33));
            n = n.wrapping_add(1);
            // Re-acquire periodically so switching output device is picked up.
            if m.is_none() || n % 90 == 0 {
                m = meter().ok();
            }
            let Some(meter) = &m else { continue };
            let Ok(peak) = (unsafe { meter.GetPeakValue() }) else {
                m = None;
                continue;
            };
            if peak < 0.002 {
                if silent {
                    continue;
                }
                silent = true;
            } else {
                silent = false;
            }
            let _ = app.emit("audio:level", peak);
        }
    });
}
