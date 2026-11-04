//! Reads the default source and every `Audio/Source` node from a connected PipeWire core.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pipewire as pw;
use pw::core::{Core, PW_ID_CORE};
use pw::main_loop::MainLoopRc;
use pw::metadata::Metadata;
use pw::properties::PropertiesBox;
use pw::registry::GlobalObject;
use pw::types::ObjectType;

use super::capture_target::{SourceNode, capture_target, metadata_node_name};

const SOURCE_CLASS: &str = "Audio/Source";
const METADATA_NAME_KEY: &str = "metadata.name";
const DEFAULT_METADATA: &str = "default";
const DEFAULT_SOURCE_KEY: &str = "default.audio.source";

/// The real microphone to pin a stream to when the client asked for the default source.
pub fn resolve_capture_target(
    main_loop: &MainLoopRc,
    core: &Core,
) -> Result<Option<String>, pw::Error> {
    let registry = core.get_registry()?;
    let sources = Rc::new(RefCell::new(Vec::new()));
    let defaults = Rc::new(RefCell::new(None::<GlobalObject<PropertiesBox>>));
    let _registry_listener = registry
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

    let default_source = Rc::new(RefCell::new(None::<String>));
    let metadata = defaults
        .borrow()
        .as_ref()
        .map(|global| registry.bind::<Metadata, _>(global))
        .transpose()?;
    let _metadata_listener = metadata.as_ref().map(|metadata| {
        let default_source = Rc::clone(&default_source);
        metadata
            .add_listener_local()
            .property(move |subject, key, _, value| {
                if subject == PW_ID_CORE && key == Some(DEFAULT_SOURCE_KEY) {
                    *default_source.borrow_mut() =
                        value.and_then(metadata_node_name).map(str::to_owned);
                }
                0
            })
            .register()
    });
    if metadata.is_some() {
        roundtrip(main_loop, core)?;
    }
    let default = default_source.borrow();
    let sources = sources.borrow();
    Ok(capture_target(default.as_deref(), &sources).map(str::to_owned))
}

/// Runs the loop until the server has answered everything sent so far.
fn roundtrip(main_loop: &MainLoopRc, core: &Core) -> Result<(), pw::Error> {
    let pending = core.sync(0)?;
    let done = Rc::new(Cell::new(false));
    let _listener = core
        .add_listener_local()
        .done({
            let (done, main_loop) = (Rc::clone(&done), main_loop.clone());
            move |id, seq| {
                if id == PW_ID_CORE && seq == pending {
                    done.set(true);
                    main_loop.quit();
                }
            }
        })
        .register();
    while !done.get() {
        main_loop.run();
    }
    Ok(())
}
