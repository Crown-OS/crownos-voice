//! `crown-voiced`: the CrownOS voice enhancement service.

mod activation;
mod all_apps;
mod daemon;
mod deferred;
mod enhancement;
mod event_loop;
mod handler;
mod job;
mod live;
mod notice;
mod processor;
mod registry;
mod settings;
mod util;

use activation::Launch;
use crownos_ipc::ServiceBuilder;
use voice_protocol::voice;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let builder = ServiceBuilder::new(voice::SERVICE);
    let (server, launch) = match activation::inherited_listener() {
        Some(listener) => (builder.build_with_listener(listener)?, Launch::Systemd),
        None => (builder.build()?, Launch::Manual),
    };
    event_loop::run(server, registry::Registry::from_env(), launch)
}
