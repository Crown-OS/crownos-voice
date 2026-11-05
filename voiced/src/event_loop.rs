//! The single-threaded IPC loop: requests, disconnects, worker notices, idle unload and exit.

use std::collections::HashSet;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use anyhow::Context;
use crownos_ipc::{PeerId, Server};
use rustix::event::{PollFd, PollFlags, Timespec};

use crate::activation::Launch;
use crate::daemon::Daemon;
use crate::notice::{NoticeInbox, notice_channel};
use crate::registry::Registry;
use crate::settings;
use crate::util::shutdown_signal::shutdown_signal;

/// The service exits after this long with no peers, streams or jobs.
const IDLE_EXIT: Duration = Duration::from_secs(30);
/// Longest sleep, so idle backends unload and the idle exit fires on time.
const TICK: Duration = Duration::from_secs(1);

struct Readiness {
    listener: bool,
    notices: bool,
    peers: Vec<(PeerId, PollFlags)>,
}

pub fn run(mut server: Server, registry: Registry, launch: Launch) -> anyhow::Result<()> {
    let shutdown = shutdown_signal().context("could not handle termination signals")?;
    let (notifier, mut inbox) = notice_channel().context("could not create the notice pipe")?;
    let _settings = settings::follow(notifier.clone());
    let mut daemon = Daemon::new(registry, notifier, settings::load());
    daemon.reconcile_all_apps();
    let mut peers = HashSet::new();
    let mut idle_since = Some(Instant::now());
    let mut idle_exit = startup_idle_exit(launch, &daemon);
    log::info!("serving {}", server.service());
    loop {
        let ready = wait(&server, &inbox)?;
        if shutdown.load(Ordering::Relaxed) {
            log::info!("asked to stop, exiting");
            return Ok(());
        }
        if ready.listener {
            while let Some(peer) = server.accept()? {
                peers.insert(peer);
            }
        }
        for (peer, events) in ready.peers {
            serve_peer(&mut server, &mut daemon, peer, events)?;
        }
        let connected: HashSet<PeerId> = server.peer_ids().into_iter().collect();
        for gone in peers.difference(&connected) {
            daemon.release(*gone);
        }
        peers = connected;
        if ready.notices {
            for notice in inbox.drain() {
                daemon.handle(&mut server, notice)?;
            }
        }
        let now = Instant::now();
        daemon.sweep(now);
        if !(peers.is_empty() && daemon.is_idle()) {
            idle_since = None;
            idle_exit = IDLE_EXIT;
        } else if now.duration_since(*idle_since.get_or_insert(now)) >= idle_exit {
            log::info!("idle for {idle_exit:?}, exiting");
            return Ok(());
        }
    }
}

/// Started at login with nothing to do, the service leaves after its first poll; a client that
/// connected meanwhile is accepted by then, and systemd starts it again for any later one.
fn startup_idle_exit(launch: Launch, daemon: &Daemon) -> Duration {
    match launch {
        Launch::Systemd if daemon.is_idle() => Duration::ZERO,
        Launch::Systemd | Launch::Manual => IDLE_EXIT,
    }
}

fn serve_peer(
    server: &mut Server,
    daemon: &mut Daemon,
    peer: PeerId,
    events: PollFlags,
) -> anyhow::Result<()> {
    if events.contains(PollFlags::OUT) && server.handle_writable(peer).is_err() {
        return Ok(());
    }
    if events.intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
        && server.handle_readable(peer).is_err()
    {
        return Ok(());
    }
    while let Some(message) = server.next_request(peer) {
        daemon.dispatch(server, peer, message)?;
    }
    server.reap(peer);
    Ok(())
}

fn wait(server: &Server, inbox: &NoticeInbox) -> anyhow::Result<Readiness> {
    let peers = server.peer_ids();
    let mut fds = Vec::with_capacity(peers.len() + 2);
    fds.push(PollFd::new(server, PollFlags::IN));
    fds.push(PollFd::new(inbox, PollFlags::IN));
    let mut polled = Vec::with_capacity(peers.len());
    for peer in peers {
        if let (Some(fd), Some(interest)) = (server.peer_fd(peer), server.peer_interest(peer)) {
            let mut flags = PollFlags::empty();
            flags.set(PollFlags::IN, interest.read);
            flags.set(PollFlags::OUT, interest.write);
            fds.push(PollFd::from_borrowed_fd(fd, flags));
            polled.push(peer);
        }
    }
    let tick = Timespec::try_from(TICK).context("tick out of range")?;
    match rustix::event::poll(&mut fds, Some(&tick)) {
        Ok(_) | Err(rustix::io::Errno::INTR) => {}
        Err(error) => return Err(std::io::Error::from(error)).context("poll failed"),
    }
    Ok(Readiness {
        listener: !fds[0].revents().is_empty(),
        notices: !fds[1].revents().is_empty(),
        peers: polled
            .into_iter()
            .zip(&fds[2..])
            .filter_map(|(peer, fd)| {
                Some((peer, fd.revents())).filter(|(_, events)| !events.is_empty())
            })
            .collect(),
    })
}
