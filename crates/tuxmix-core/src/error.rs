use thiserror::Error;

/// Errors that can occur during device discovery or control.
#[derive(Error, Debug)]
pub enum Error {
    #[error("No RME device found matching: {model}")]
    DeviceNotFound { model: String },

    #[error("ALSA error: {0}")]
    #[cfg(feature = "alsa")]
    Alsa(#[from] alsa::Error),

    #[error("USB error: {0}")]
    #[cfg(feature = "usb")]
    Usb(#[from] tuxmix_usb::DeviceError),

    #[error("Mixer element not found: {0}")]
    MixerElementNotFound(String),

    /// A card was found whose name matched, but whose mixer is neither
    /// grammar the ALSA backend knows — neither `snd-usb-babyface-pro`
    /// (proprietary) nor stock `snd-usb-audio` on the class-compliant
    /// personality. Both are supported, so in practice this means some
    /// *other* card whose name happens to contain the profile's
    /// `card_substring`, or a driver too old to expose the sentinel
    /// controls. Distinct from [`Error::DeviceNotFound`] so the UI can
    /// say what to actually do about it.
    #[error("{card} is connected, but Audra doesn't recognise its controls \
             (missing: {missing}). If this is a Babyface Pro, its driver may be \
             old or missing; or switch its mode: hold SELECT and DIM while \
             plugging it in to change between RME mode and Class Compliant mode.")]
    UnsupportedDeviceMode { card: String, missing: String },

    #[error("Invalid channel: {0}")]
    InvalidChannel(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Scene captured on '{scene_model}' cannot be applied to '{device_model}'")]
    SceneModelMismatch {
        scene_model: String,
        device_model: String,
    },

    /// A scene with another number of channels than the unit (a file
    /// hand-edited, cut short or from another version).
    #[error("it holds {scene}, and this unit has {device}")]
    SceneShapeMismatch { scene: String, device: String },
}
