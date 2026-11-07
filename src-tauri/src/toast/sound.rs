//! The system screenshot sound, played with each toast.
//!
//! macOS plays the sound its own screenshots make (`Screen Capture.aif`, or the older
//! `Grab.aif`), loaded once. Windows has no screenshot sound (Snipping Tool is silent), so
//! nothing plays there.

use std::path::Path;

/// The macOS screenshot sounds, in the order tried.
#[cfg(target_os = "macos")]
const FILES: &[&str] = &[
    "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Screen Capture.aif",
    "/System/Library/Components/CoreAudio.component/Contents/SharedSupport/SystemSounds/system/Grab.aif",
];
#[cfg(not(target_os = "macos"))]
const FILES: &[&str] = &[];

/// The first of `files` that `exists`.
fn pick<'a>(files: &[&'a str], exists: impl Fn(&Path) -> bool) -> Option<&'a str> {
    files.iter().copied().find(|f| exists(Path::new(f)))
}

/// The sound file to play, if this system has one.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn file() -> Option<&'static str> {
    pick(FILES, Path::exists)
}

/// Plays the sound, without waiting for it. Main thread only.
pub fn play() {
    #[cfg(all(target_os = "macos", not(test)))]
    macos::play();
}

#[cfg(all(target_os = "macos", not(test)))]
mod macos {
    use objc2::AllocAnyThread;
    use objc2::rc::Retained;
    use objc2_app_kit::NSSound;
    use objc2_foundation::NSString;
    use std::cell::OnceCell;

    thread_local! {
        /// Loaded on the first toast, then reused.
        static SOUND: OnceCell<Option<Retained<NSSound>>> = const { OnceCell::new() };
    }

    pub fn play() {
        SOUND.with(|cell| {
            let sound = cell.get_or_init(|| {
                let path = super::file()?;
                let sound = NSSound::initWithContentsOfFile_byReference(
                    NSSound::alloc(),
                    &NSString::from_str(path),
                    true,
                );
                if sound.is_none() {
                    log::warn!("could not load the screenshot sound {path}");
                }
                sound
            });
            if let Some(sound) = sound {
                // From the start, if the last toast's sound is still playing.
                sound.stop();
                if !sound.play() {
                    log::warn!("could not play the screenshot sound");
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_first_sound_that_exists() {
        let files = ["/a.aif", "/b.aif"];
        assert_eq!(pick(&files, |p| p == Path::new("/b.aif")), Some("/b.aif"));
        assert_eq!(pick(&files, |_| true), Some("/a.aif"));
        assert_eq!(pick(&files, |_| false), None);
        // Whatever this system has (macOS ships them); nothing elsewhere.
        let found = file();
        assert!(cfg!(target_os = "macos") || found.is_none());
        play();
    }
}
