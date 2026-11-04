//! `crown-voiced`: the CrownOS voice enhancement service.

mod activation;
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
mod util;

use crownos_ipc::ServiceBuilder;
use voice_protocol::voice;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    let builder = ServiceBuilder::new(voice::SERVICE);
    let server = match activation::inherited_listener() {
        Some(listener) => builder.build_with_listener(listener)?,
        None => builder.build()?,
    };
    event_loop::run(server, registry::Registry::from_env())
}
