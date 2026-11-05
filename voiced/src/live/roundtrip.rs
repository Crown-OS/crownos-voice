use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

use anyhow::bail;
use pipewire as pw;
use pw::core::{Core, PW_ID_CORE};
use pw::main_loop::MainLoopRc;

/// How long PipeWire may take to answer before the server counts as gone.
const ROUNDTRIP_TIMEOUT: Duration = Duration::from_secs(2);

/// Runs the loop until the server has answered everything sent so far.
pub fn roundtrip(main_loop: &MainLoopRc, core: &Core) -> anyhow::Result<()> {
    let pending = core.sync(0)?;
    let done = Rc::new(Cell::new(false));
    let timed_out = Rc::new(Cell::new(false));
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
    let timer = main_loop.loop_().add_timer({
        let (timed_out, main_loop) = (Rc::clone(&timed_out), main_loop.clone());
        move |_| {
            timed_out.set(true);
            main_loop.quit();
        }
    });
    timer
        .update_timer(Some(ROUNDTRIP_TIMEOUT), None)
        .into_sync_result()?;
    while !done.get() {
        if timed_out.get() {
            bail!("PipeWire did not answer within {ROUNDTRIP_TIMEOUT:?}");
        }
        main_loop.run();
    }
    Ok(())
}
