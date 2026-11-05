//! The `voice` section of the CrownOS settings, followed live.

mod apply;

use crownos_config::Subscription;
use crownos_config::schema::Voice;

use crate::notice::{Notice, Notifier};

/// Reads `voice.ron`, writing the defaults out when it does not exist yet.
pub fn load() -> Voice {
    crownos_config::load(Voice::SECTION)
}

/// Reports every later edit of `voice.ron` as [`Notice::SettingsChanged`].
#[must_use = "dropping the subscription stops following voice.ron"]
pub fn follow(notifier: Notifier) -> Subscription {
    crownos_config::subscribe_typed::<Voice, _>(Voice::SECTION, move |voice| {
        notifier.notify(Notice::SettingsChanged(voice));
    })
}
