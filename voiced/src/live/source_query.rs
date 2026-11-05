//! Reads the default sources and every `Audio/Source` node from a connected PipeWire core.

use std::cell::RefCell;
use std::rc::Rc;

use pipewire as pw;
use pw::core::{Core, PW_ID_CORE};
use pw::main_loop::MainLoopRc;
use pw::metadata::Metadata;
use pw::properties::PropertiesBox;
use pw::registry::{GlobalObject, Registry};
use pw::types::ObjectType;

use super::capture_target::{SourceNode, capture_target};
use super::default_metadata::{
    CONFIGURED_SOURCE_KEY, DEFAULT_METADATA, DEFAULT_SOURCE_KEY, metadata_node_name,
};
use super::roundtrip::roundtrip;

const SOURCE_CLASS: &str = "Audio/Source";
const METADATA_NAME_KEY: &str = "metadata.name";

/// The sources PipeWire lists and what the `default` metadata says about them.
pub struct SourceScan {
    pub sources: Vec<SourceNode>,
    pub configured: Option<String>,
    pub effective: Option<String>,
    /// The `default` metadata object, bound; `None` without a session manager.
    pub defaults: Option<Metadata>,
}

impl SourceScan {
    /// The real microphone to capture when the client asked for the default source.
    pub fn microphone(&self) -> Option<&str> {
        capture_target(
            &[self.configured.as_deref(), self.effective.as_deref()],
            &self.sources,
        )
    }
}

pub fn scan_sources(
    main_loop: &MainLoopRc,
    core: &Core,
    registry: &Registry,
) -> anyhow::Result<SourceScan> {
    let sources = Rc::new(RefCell::new(Vec::new()));
    let defaults = Rc::new(RefCell::new(None::<GlobalObject<PropertiesBox>>));
    let registry_listener = registry
        .add_listener_local()
        .global({
            let (sources, defaults) = (Rc::clone(&sources), Rc::clone(&defaults));
            move |global| {
                let Some(props) = &global.props else {
                    return;
                };
                match global.type_ {
                    ObjectType::Node if props.get(*pw::keys::MEDIA_CLASS) == Some(SOURCE_CLASS) => {
                        if let Some(name) = props.get(*pw::keys::NODE_NAME) {
                            sources.borrow_mut().push(SourceNode {
                                name: name.to_owned(),
                                priority: props
                                    .get(*pw::keys::PRIORITY_SESSION)
                                    .and_then(|priority| priority.parse().ok())
                                    .unwrap_or_default(),
                            });
                        }
                    }
                    ObjectType::Metadata
                        if props.get(METADATA_NAME_KEY) == Some(DEFAULT_METADATA) =>
                    {
                        *defaults.borrow_mut() = Some(global.to_owned());
                    }
                    _ => {}
                }
            }
        })
        .register();
    roundtrip(main_loop, core)?;
    drop(registry_listener);

    let values = Rc::new(RefCell::new((None::<String>, None::<String>)));
    let metadata = defaults
        .borrow()
        .as_ref()
        .map(|global| registry.bind::<Metadata, _>(global))
        .transpose()?;
    if let Some(metadata) = &metadata {
        let _metadata_listener = metadata
            .add_listener_local()
            .property({
                let values = Rc::clone(&values);
                move |subject, key, _, value| {
                    let name = || value.and_then(metadata_node_name).map(str::to_owned);
                    let mut values = values.borrow_mut();
                    match key {
                        Some(CONFIGURED_SOURCE_KEY) if subject == PW_ID_CORE => values.0 = name(),
                        Some(DEFAULT_SOURCE_KEY) if subject == PW_ID_CORE => values.1 = name(),
                        _ => {}
                    }
                    0
                }
            })
            .register();
        roundtrip(main_loop, core)?;
    }
    let (configured, effective) = values.take();
    Ok(SourceScan {
        sources: sources.take(),
        configured,
        effective,
        defaults: metadata,
    })
}
