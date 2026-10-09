#![cfg(target_os = "linux")]

use std::{
    fs,
    io::{self, BufRead, BufReader},
    process::{Child, Command, Stdio},
    sync::mpsc::{self, RecvTimeoutError},
    thread,
    time::{Duration, Instant},
};

struct ConfigHome(std::path::PathBuf);

impl ConfigHome {
    fn new() -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock before Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ferrum-perf-config-{}-{timestamp}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create unique temporary config directory");
        Self(path)
    }
}

impl Drop for ConfigHome {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
struct AppProcess(Child);

impl Drop for AppProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn process_sample(pid: u32) -> io::Result<(u64, u64)> {
    let status = fs::read_to_string(format!("/proc/{pid}/status"))?;
    let rss_kib = status
        .lines()
        .find_map(|line| line.strip_prefix("VmRSS:"))
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::other("VmRSS missing from proc status"))?;

    let stat = fs::read_to_string(format!("/proc/{pid}/stat"))?;
    let fields = stat
        .rsplit_once(')')
        .ok_or_else(|| io::Error::other("process stat lacks command terminator"))?
        .1
        .split_whitespace()
        .collect::<Vec<_>>();
    let user_ticks = fields
        .get(11)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::other("user CPU ticks missing from proc stat"))?;
    let system_ticks = fields
        .get(12)
        .and_then(|value| value.parse::<u64>().ok())
        .ok_or_else(|| io::Error::other("system CPU ticks missing from proc stat"))?;

    Ok((rss_kib, user_ticks + system_ticks))
}

fn forward_log_lines<R>(reader: R, sender: mpsc::SyncSender<String>) -> thread::JoinHandle<()>
where
    R: io::Read + Send + 'static,
{
    thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            match line {
                Ok(line) => {
                    // Drop excess diagnostics rather than block the app; preserve readiness.
                    let is_inventory = line.contains("Loaded local container inventory");
                    let result = if is_inventory {
                        sender.send(line).map_err(|_| ())
                    } else {
                        match sender.try_send(line) {
                            Ok(()) | Err(mpsc::TrySendError::Full(_)) => Ok(()),
                            Err(mpsc::TrySendError::Disconnected(_)) => Err(()),
                        }
                    };
                    if result.is_err() {
                        break;
                    }
                }
                Err(error) => {
                    let _ = sender.try_send(format!("log reader failed: {error}"));
                    break;
                }
            }
        }
    })
}

#[test]
#[ignore = "manual release-mode sample; requires X11, a local engine, and startup auto-connect"]
fn measures_release_launch_inventory_memory_and_idle_cpu() {
    assert!(
        std::env::var_os("DISPLAY").is_some(),
        "an active X11 display is required"
    );

    let config_home = ConfigHome::new();
    let mut command = Command::new(env!("CARGO_BIN_EXE_ferrum"));
    command
        .env("WINIT_UNIX_BACKEND", "x11")
        .env("XDG_CONFIG_HOME", &config_home.0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let launch_started = Instant::now();
    let mut child = AppProcess(command.spawn().expect("start the Ferrum release binary"));
    let pid = child.0.id();
    let stdout = child.0.stdout.take().expect("stdout was piped");
    let stderr = child.0.stderr.take().expect("stderr was piped");
    let (line_sender, line_receiver) = mpsc::sync_channel(64);
    let stdout_reader = forward_log_lines(stdout, line_sender.clone());
    let stderr_reader = forward_log_lines(stderr, line_sender);

    let deadline = Instant::now() + Duration::from_secs(30);
    let mut recent_logs = Vec::new();
    let inventory_line = loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(
            !remaining.is_zero(),
            "inventory was not loaded: {recent_logs:?}"
        );
        let line = match line_receiver.recv_timeout(remaining) {
            Ok(line) => line,
            Err(error) => {
                panic!("Ferrum did not report an inventory ({error}); recent logs: {recent_logs:?}")
            }
        };
        if recent_logs.len() == 8 {
            recent_logs.remove(0);
        }
        recent_logs.push(line.clone());
        if line.contains("Loaded local container inventory") {
            break line;
        }
    };
    let inventory_ready = Instant::now();
    let container_count = inventory_line
        .split("container_count=")
        .nth(1)
        .and_then(|value| value.split_whitespace().next())
        .and_then(|value| value.parse::<usize>().ok())
        .expect("inventory log reports its container count");
    let (rss_at_inventory_kib, cpu_at_inventory_ticks) =
        process_sample(pid).expect("sample the Ferrum process after inventory load");

    let idle_sample = Duration::from_secs(30);
    let sample_deadline = Instant::now() + idle_sample;
    while Instant::now() < sample_deadline {
        match line_receiver.recv_timeout(
            sample_deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        ) {
            Ok(line) => {
                if recent_logs.len() == 8 {
                    recent_logs.remove(0);
                }
                recent_logs.push(line);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let (rss_after_idle_kib, cpu_after_idle_ticks) =
        process_sample(pid).expect("sample the Ferrum process after the idle interval");
    let clock_ticks_per_second = Command::new("getconf")
        .arg("CLK_TCK")
        .output()
        .expect("getconf is required to convert Linux process CPU ticks")
        .stdout;
    let clock_ticks_per_second = String::from_utf8(clock_ticks_per_second)
        .expect("getconf output is UTF-8")
        .trim()
        .parse::<u64>()
        .expect("getconf reports CLK_TCK");
    assert_ne!(clock_ticks_per_second, 0);

    eprintln!(
        "release UI sample: inventory_ready_ms={}, containers={}, rss_at_inventory_mib={:.1}, rss_after_30s_mib={:.1}, cpu_seconds_over_30s={:.4}",
        inventory_ready.duration_since(launch_started).as_millis(),
        container_count,
        rss_at_inventory_kib as f64 / 1024.0,
        rss_after_idle_kib as f64 / 1024.0,
        (cpu_after_idle_ticks - cpu_at_inventory_ticks) as f64 / clock_ticks_per_second as f64
    );

    let _ = child.0.kill();
    let _ = child.0.wait();
    stdout_reader
        .join()
        .expect("join the release-app stdout reader");
    stderr_reader
        .join()
        .expect("join the release-app stderr reader");
}
