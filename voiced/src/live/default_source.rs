//! The PipeWire side of [`DefaultClaim`]: the all-apps node as the default source.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pipewire as pw;
use pw::constants::ID_ANY;
use pw::core::{Core, PW_ID_CORE};
use pw::main_loop::MainLoopRc;
use pw::metadata::{Metadata, MetadataListener};

use super::default_claim::{ClaimStep, DefaultClaim};
use super::default_metadata::{
    CONFIGURED_SOURCE_KEY, JSON_TYPE, TARGET_OBJECT_KEY, metadata_node_name, metadata_value,
};
use super::roundtrip::roundtrip;

/// Holds the default source on the all-apps node until [`DefaultSourceKeeper::release`].
pub struct DefaultSourceKeeper {
    listener: MetadataListener,
    metadata: Rc<Metadata>,
    claim: Rc<RefCell<DefaultClaim>>,
}

impl DefaultSourceKeeper {
    /// Makes the all-apps node the default; `capture_node` is the id of its capture stream.
    pub fn claim(metadata: Metadata, claim: DefaultClaim, capture_node: Rc<Cell<u32>>) -> Self {
        let metadata = Rc::new(metadata);
        let claim = Rc::new(RefCell::new(claim));
        let listener = metadata
            .add_listener_local()
            .property({
                let (metadata, claim) = (Rc::clone(&metadata), Rc::clone(&claim));
                move |subject, key, _, value| {
                    if subject == PW_ID_CORE && key.is_none_or(|key| key == CONFIGURED_SOURCE_KEY) {
                        let configured = key.and(value).and_then(metadata_node_name);
                        let step = claim.borrow_mut().on_configured(configured);
                        apply_step(&metadata, &claim.borrow(), &step, capture_node.get());
                    }
                    0
                }
            })
            .register();
        point_default_at(&metadata, Some(claim.borrow().own_node()));
        Self {
            listener,
            metadata,
            claim,
        }
    }

    /// Points the default back at the microphone and waits until PipeWire has it.
    pub fn release(self, main_loop: &MainLoopRc, core: &Core) -> anyhow::Result<()> {
        drop(self.listener);
        let claim = self.claim.borrow();
        log::info!(
            "giving the default source back to {}",
            claim.microphone().unwrap_or("the session manager")
        );
        point_default_at(&self.metadata, claim.microphone());
        roundtrip(main_loop, core)
    }
}

fn apply_step(metadata: &Metadata, claim: &DefaultClaim, step: &ClaimStep, capture_node: u32) {
    match step {
        ClaimStep::Hold => return,
        ClaimStep::Retarget => {
            if let Some(microphone) = claim.microphone()
                && capture_node != ID_ANY
            {
                log::info!("{} now captures {microphone}", claim.own_node());
                metadata.set_property(capture_node, TARGET_OBJECT_KEY, None, Some(microphone));
            }
        }
        ClaimStep::Reclaim => {}
    }
    point_default_at(metadata, Some(claim.own_node()));
}

/// Writes the configured default source as `pactl set-default-source` does; `None` clears it.
fn point_default_at(metadata: &Metadata, node_name: Option<&str>) {
    let value = node_name.map(metadata_value);
    metadata.set_property(
        PW_ID_CORE,
        CONFIGURED_SOURCE_KEY,
        value.as_ref().map(|_| JSON_TYPE),
        value.as_deref(),
    );
}
