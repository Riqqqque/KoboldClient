//! Daily rolling log files plus a panic hook, with a filter that can be changed live.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{fmt, reload, EnvFilter, Registry};

pub struct LogHandle {
    filter: reload::Handle<EnvFilter, Registry>,
    _guard: WorkerGuard,
}

fn filter_for(verbose: bool) -> EnvFilter {
    let level = if verbose { "debug" } else { "info" };
    // Keep third-party crates quiet unless something goes wrong.
    EnvFilter::new(format!("warn,kobold_lib={level},kobold_client={level}"))
}

pub fn init(log_dir: &Path, verbose: bool) -> Option<LogHandle> {
    let _ = std::fs::create_dir_all(log_dir);
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("kobold")
        .filename_suffix("log")
        .max_log_files(14)
        .build(log_dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    let (filter, handle) = reload::Layer::new(filter_for(verbose));
    let file_layer = fmt::layer()
        .with_writer(writer)
        .with_ansi(false)
        .with_target(false);
    let console = cfg!(debug_assertions).then(|| fmt::layer().with_target(false));
    tracing_subscriber::registry()
        .with(filter)
        .with(file_layer)
        .with(console)
        .try_init()
        .ok()?;

    std::panic::set_hook(Box::new(|info| {
        let bt = std::backtrace::Backtrace::force_capture();
        tracing::error!(%info, backtrace = %bt, "panic");
    }));

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        os = std::env::consts::OS,
        arch = std::env::consts::ARCH,
        "Kobold Client starting"
    );
    Some(LogHandle {
        filter: handle,
        _guard: guard,
    })
}

impl LogHandle {
    pub fn set_verbose(&self, verbose: bool) {
        if let Err(e) = self.filter.reload(filter_for(verbose)) {
            tracing::warn!(error = %e, "couldn't change log level");
        } else {
            tracing::info!(verbose, "log level changed");
        }
    }
}
