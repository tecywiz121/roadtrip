use futures::pin_mut;
use futures::stream::StreamExt;

use roadtrip_ingest::ingest::Exiftool;
use roadtrip_ingest::Scanner;

use std::env::args_os;

use tokio::runtime::Runtime;

fn main() {
    let mut scanner = Scanner::default();
    let ingester = Exiftool::new(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../roadtrip-ingest/src/ingest/gpx.fmt"
        )
        .into(),
    );

    scanner.add_ingester(ingester);

    for arg in args_os().skip(1) {
        scanner.insert_path(arg);
    }

    let mut rt = Runtime::new().unwrap();
    rt.block_on(async {
        let scan = scanner.scan();
        pin_mut!(scan);

        while let Some(res) = scan.next().await {
            match res {
                Err(e) => {
                    println!("ERR: {} ({})", e.path().to_string_lossy(), e);
                }
                Ok(m) => {
                    println!(
                        " OK: {} ({} points)",
                        m.path().to_string_lossy(),
                        m.geometry().len(),
                    );
                }
            }
        }
    });
}
