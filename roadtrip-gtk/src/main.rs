mod main_window;

use crate::main_window::Main;

use futures::StreamExt;

use gio::prelude::*;

use glib::MainContext;

use roadtrip::viewer::{SyncHandle, Viewer};

use std::cell::RefCell;
use std::sync::mpsc::{sync_channel, SyncSender};
use std::thread;

thread_local! {
    static MAIN: RefCell<Option<Main>> = RefCell::new(None);
}

#[tokio::main]
async fn viewer(sender: SyncSender<SyncHandle>) {
    let viewer = Viewer::spawn().await.unwrap();

    let handle = viewer.handle().into_sync().clone();
    sender.send(handle.clone()).unwrap();

    let exit_handle = viewer.handle().clone();
    tokio::spawn(async move {
        // Wait for the sender to be dropped, then exit.
        tokio::task::spawn_blocking(move || sender.send(handle))
            .await
            .unwrap()
            .unwrap_err();
        exit_handle.exit().await;
    });

    let mut events = viewer.events();

    let context = MainContext::default();
    while let Some(event) = events.next().await {
        context.invoke(|| {
            MAIN.with(|m| {
                // TODO: If `Main` is `None`, there's no event handler.
                if let Some(x) = m.borrow().as_ref() {
                    x.event(event)
                }
            })
        });
    }
}

fn main() -> Result<(), i32> {
    let application = gtk::Application::new(
        Some("rocks.tabby.roadtrip"),
        gio::ApplicationFlags::empty(),
    )
    .expect("Initialization failed...");

    let (sender, receiver) = sync_channel(0);

    let viewer_thread = thread::Builder::new()
        .spawn(|| viewer(sender))
        .expect("unable to start viewer thread");

    let handle = receiver.recv().unwrap();

    application.connect_startup(move |app| {
        MAIN.with(|m| {
            let mut holder = m.borrow_mut();
            assert!(holder.is_none());

            let main = main_window::Main::new(app.clone(), handle.clone());
            main.actions();
            main.build();
            main.show_all();
            *holder = Some(main);
        })
    });

    application.connect_activate(|_| {});

    let retval = application.run(&std::env::args().collect::<Vec<_>>());
    drop(receiver);

    viewer_thread.join().expect("viewer thread panicked");

    if retval == 0 {
        Ok(())
    } else {
        Err(retval)
    }
}
