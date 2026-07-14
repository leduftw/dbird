//! Non-blocking playback of the game's short sound effects.

use std::io::Cursor;

use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, mixer::Mixer};

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
    active_players: Vec<Player>,
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
        Self {
            output,
            active_players: Vec::new(),
        }
    }

    /// Play one effect without delaying gameplay. Decode or device errors are non-fatal.
    pub fn play(&mut self, sound: Sound) {
        self.active_players.retain(|player| !player.empty());

        let Some(output) = &self.output else {
            return;
        };
        if let Some(player) = start_sound(output.mixer(), sound) {
            self.active_players.push(player);
        }
    }
}

fn start_sound(mixer: &Mixer, sound: Sound) -> Option<Player> {
    let source = Decoder::new_vorbis(Cursor::new(sound.bytes())).ok()?;

    // A Vorbis Decoder starts with a zero-length span. Adding it straight to
    // rodio's mixer makes the adapter mistake that span for an exhausted sound.
    // Player supplies a keep-alive queue and must live until playback completes.
    let player = Player::connect_new(mixer);
    player.append(source);
    Some(player)
}

#[cfg(test)]
mod tests {
    use std::num::NonZero;

    use rodio::mixer;

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
        let mut audio = Audio::new(false);
        audio.play(Sound::Wing);
        assert!(audio.active_players.is_empty());
    }

    #[test]
    fn retained_players_deliver_every_effect() {
        for sound in [
            Sound::Wing,
            Sound::Point,
            Sound::Hit,
            Sound::Die,
            Sound::Swoosh,
        ] {
            let (mixer, mixed) = mixer::mixer(
                NonZero::new(2).expect("stereo"),
                NonZero::new(44_100).expect("sample rate"),
            );
            let player = start_sound(&mixer, sound).expect("sound player");
            let mut peak = 0.0_f32;

            for sample in mixed.take(250_000) {
                peak = peak.max(sample.abs());
                if player.empty() {
                    break;
                }
            }

            assert!(peak > 0.01, "{sound:?} emitted only silence");
            assert!(player.empty(), "{sound:?} should finish");
        }
    }
}
