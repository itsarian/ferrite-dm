use fdm_core::FdmConfig;
use fdm_native_host::{UnixDaemonBridge, dispatch_native_task, read_native_message, write_native_message};
use std::io;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let portable = args.iter().any(|a| a == "--portable");
    let mut data_dir: Option<PathBuf> = None;
    let mut i = 1;
    while i < args.len() {
        if args[i] == "--data-dir" && i + 1 < args.len() {
            data_dir = Some(PathBuf::from(&args[i + 1]));
            i += 2;
            continue;
        }
        i += 1;
    }

    let data_dir = data_dir.unwrap_or_else(|| FdmConfig::data_dir(portable));
    let bridge = UnixDaemonBridge::from_data_dir(data_dir, true);

    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut in_lock = stdin.lock();
    let mut out_lock = stdout.lock();

    while let Some(msg) = read_native_message(&mut in_lock)? {
        let resp = dispatch_native_task(&bridge, &msg).await;
        write_native_message(&mut out_lock, &resp)?;
    }

    Ok(())
}
