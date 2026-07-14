//! Non-blocking playback of the game's short sound effects.

use std::io::Cursor;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink};

const DIE: &[u8] = include_bytes!("../assets/sounds/sfx_die.ogg");
const HIT: &[u8] = include_bytes!("../assets/sounds/sfx_hit.ogg");
const POINT: &[u8] = include_bytes!("../assets/sounds/sfx_point.ogg");
const SWOOSH: &[u8] = include_bytes!("../assets/sounds/sfx_swooshing.ogg");
const WING: &[u8] = include_bytes!("../assets/sounds/sfx_wing.ogg");

/// A sound-producing game event.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Sound {
    Wing,
    Point,
    Hit,
    Die,
    Swoosh,
}

impl Sound {
    const fn bytes(self) -> &'static [u8] {
        match self {
            Self::Wing => WING,
            Self::Point => POINT,
            Self::Hit => HIT,
            Self::Die => DIE,
            Self::Swoosh => SWOOSH,
        }
    }
}

/// Best-effort audio output. A missing or busy audio device silently disables it.
pub struct Audio {
    output: Option<MixerDeviceSink>,
}

impl Audio {
    /// Open the default audio device unless sound was disabled by the user.
    pub fn new(enabled: bool) -> Self {
        let output = enabled
            .then(DeviceSinkBuilder::from_default_device)
            .and_then(Result::ok)
            .and_then(|builder| {
                builder
                    .with_error_callback(|_| {})
                    .open_sink_or_fallback()
                    .ok()
            })
            .map(|mut output| {
                output.log_on_drop(false);
                output
            });
        Self { output }
    }

    /// Play one effect without delaying gameplay. Decode or device errors are non-fatal.
    pub fn play(&self, sound: Sound) {
        let Some(output) = &self.output else {
            return;
        };
        let Ok(source) = Decoder::new_vorbis(Cursor::new(sound.bytes())) else {
            return;
        };
        output.mixer().add(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_embedded_effects_are_ogg_streams() {
        for sound in [
            Sound::Wing,
            Sound::Point,
            Sound::Hit,
            Sound::Die,
            Sound::Swoosh,
        ] {
            assert!(sound.bytes().starts_with(b"OggS"));
            assert!(sound.bytes().len() > 1_000);
            assert!(Decoder::new_vorbis(Cursor::new(sound.bytes())).is_ok());
        }
    }

    #[test]
    fn disabled_audio_is_a_safe_no_op() {
        let audio = Audio::new(false);
        audio.play(Sound::Wing);
    }
}
