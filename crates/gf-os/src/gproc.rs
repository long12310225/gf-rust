//! Port of `os/gproc` — the shell execution lane.
//!
//! GoFrame's `gproc` has two halves: a goroutine/"process" communication layer
//! built on Go's `goroutine` ids and `os.Signal`, and a shell execution lane
//! that runs commands through the platform shell. The port implements the shell
//! lane and the process identity helpers; the communication, signal and
//! manager layers are not ported (ADR-057).
//!
//! Divergences, recorded in ADR-057:
//!
//! - no `context.Context` and no OpenTelemetry environment injection;
//! - `PPidOS` is only available on Linux (through `/proc/self/stat`); other
//!   platforms return `0`;
//! - `Shell`/`ShellRun`/`ShellExec` become [`shell`]/[`shell_run`]/
//!   [`shell_exec`];
//! - the goroutine communication and signal handling are not ported: Rust has
//!   no goroutine identity and no cross-thread signal API in `std`; the
//!   process manager lane (registry, start/kill/wait, ADR-141) is available.

use std::collections::HashMap;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Condvar, Mutex, Once, OnceLock, RwLock};
use std::time::{Duration, SystemTime};

use gf_errors::gerror::{self, Error};

use crate::genv;
use crate::gfile;

/// The environment key for a custom parent pid, like Go's `envKeyPPid`.
pub const ENV_KEY_PPID: &str = "GPROC_PPID";

/// The process start time, like Go's `processStartTime`.
fn process_start_time() -> &'static SystemTime {
    static START: OnceLock<SystemTime> = OnceLock::new();
    START.get_or_init(SystemTime::now)
}

/// Returns the pid of the current process, like `Pid`.
pub fn pid() -> u32 {
    std::process::id()
}

/// Reports whether the current process was started by another gproc process,
/// like `IsChild`.
pub fn is_child() -> bool {
    match std::env::var(ENV_KEY_PPID) {
        Ok(value) => !value.is_empty() && value != "0",
        Err(_) => false,
    }
}

/// Returns the custom parent pid when set, otherwise the system parent pid,
/// like `PPid`.
pub fn ppid() -> u32 {
    if !is_child() {
        return pid();
    }
    match std::env::var(ENV_KEY_PPID) {
        Ok(value) if !value.is_empty() && value != "0" => {
            value.parse::<u32>().unwrap_or_else(|_| ppid_os())
        }
        _ => ppid_os(),
    }
}

/// Returns the system parent pid, like `PPidOS`.
///
/// Only Linux is implemented (through `/proc/self/stat`); other platforms
/// return `0`.
pub fn ppid_os() -> u32 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(stat) = std::fs::read_to_string("/proc/self/stat")
            && let Some(end) = stat.rfind(')')
        {
            // Fields after `comm`: state ppid ...
            let rest = &stat[end + 1..];
            let mut fields = rest.split_whitespace();
            let _state = fields.next();
            if let Some(ppid) = fields.next()
                && let Ok(ppid) = ppid.parse::<u32>()
            {
                return ppid;
            }
        }
        0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0
    }
}

/// Sets a custom parent pid for the current process, like `SetPPid`.
pub fn set_ppid(ppid: i64) -> Result<(), Error> {
    if ppid > 0 {
        genv::set(ENV_KEY_PPID, &ppid.to_string())
    } else {
        genv::remove(&[ENV_KEY_PPID]).map(|_| ())
    }
}

/// Returns the start time of the current process, like `StartTime`.
pub fn start_time() -> SystemTime {
    *process_start_time()
}

/// Returns how long the current process has been running, like `Uptime`.
pub fn uptime() -> Duration {
    start_time().elapsed().unwrap_or(Duration::ZERO)
}

/// Searches a binary in the current folder and `PATH`, like `SearchBinary`.
pub fn search_binary(file: &str) -> String {
    if gfile::exists(file) {
        return file.to_owned();
    }
    search_binary_path(file)
}

/// Searches a binary in `PATH`, like `SearchBinaryPath`.
pub fn search_binary_path(file: &str) -> String {
    let path = std::env::var("PATH")
        .or_else(|_| std::env::var("Path"))
        .unwrap_or_default();
    let separator = if cfg!(windows) { ';' } else { ':' };
    let candidates: Vec<&str> = path
        .split(separator)
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .collect();
    let mut names = vec![file.to_owned()];
    if cfg!(windows) && !file.to_ascii_lowercase().ends_with(".exe") {
        names.push(format!("{file}.exe"));
    }
    for name in names {
        for directory in &candidates {
            let candidate = gfile::join(&[directory, &name]);
            if gfile::exists(&candidate) && gfile::is_file(&candidate) {
                return candidate;
            }
        }
    }
    String::new()
}

/// The program and option used to run a shell command, like `getShell` +
/// `getShellOption`.
fn shell_program() -> (String, &'static str) {
    if cfg!(windows) {
        let program = search_binary("cmd.exe");
        (program, "/c")
    } else {
        for candidate in ["/bin/bash", "/bin/sh"] {
            if gfile::exists(candidate) {
                return (candidate.to_owned(), "-c");
            }
        }
        let mut program = search_binary("bash");
        if program.is_empty() {
            program = search_binary("sh");
        }
        (program, "-c")
    }
}

/// A managed child process, like GoFrame's `gproc.Process` (ADR-141).
pub struct Process {
    name: String,
    pid: u32,
    child: Mutex<Option<Child>>,
}

impl Process {
    /// Wraps a spawned child under `name`.
    pub fn new(name: &str, child: Child) -> Process {
        let pid = child.id();
        Process {
            name: name.to_owned(),
            pid,
            child: Mutex::new(Some(child)),
        }
    }

    /// The process name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The process id.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Reports whether the child is still running (reaping it when it exited).
    pub fn is_running(&self) -> bool {
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(None)),
            None => false,
        }
    }

    /// Kills the child, like `Process.Kill`.
    pub fn kill(&self) -> Result<(), Error> {
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(child) = guard.as_mut() else {
            return Ok(());
        };
        child
            .kill()
            .map_err(|error| gerror::new(format!("kill process \"{}\" failed: {error}", self.name)))
    }

    /// Waits for the child and returns its exit code, like `Process.Wait`.
    pub fn wait(&self) -> Result<Option<i32>, Error> {
        let mut guard = self
            .child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(child) = guard.as_mut() else {
            return Ok(None);
        };
        let status = child.wait().map_err(|error| {
            gerror::new(format!("wait process \"{}\" failed: {error}", self.name))
        })?;
        Ok(status.code())
    }

    /// Releases the handle without killing the child.
    pub fn release(&self) {
        self.child
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take();
    }
}

/// The process registry, like GoFrame's `gproc.Manager` (ADR-141).
#[derive(Default)]
pub struct Manager {
    processes: RwLock<HashMap<String, Arc<Process>>>,
}

impl Manager {
    /// Creates an empty manager.
    pub fn new() -> Manager {
        Manager::default()
    }

    /// Spawns `program` under `name`, replacing (and killing) an existing
    /// process with that name.
    pub fn start(&self, name: &str, program: &str, args: &[&str]) -> Result<Arc<Process>, Error> {
        if let Some(existing) = self.remove_process(name) {
            let _ = existing.kill();
        }
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .spawn()
            .map_err(|error| gerror::new(format!("start \"{program}\" failed: {error}")))?;
        let process = Arc::new(Process::new(name, child));
        self.add_process(Arc::clone(&process));
        Ok(process)
    }

    /// Registers `process`.
    pub fn add_process(&self, process: Arc<Process>) {
        self.processes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(process.name().to_owned(), process);
    }

    /// Returns the process named `name`.
    pub fn get_process(&self, name: &str) -> Option<Arc<Process>> {
        self.processes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(name)
            .cloned()
    }

    /// Removes and returns the process named `name`.
    pub fn remove_process(&self, name: &str) -> Option<Arc<Process>> {
        self.processes
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(name)
    }

    /// Returns every registered process.
    pub fn processes(&self) -> Vec<Arc<Process>> {
        self.processes
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .values()
            .cloned()
            .collect()
    }

    /// Kills every registered process.
    pub fn kill_all(&self) {
        for process in self.processes() {
            let _ = process.kill();
        }
    }

    /// Waits for every registered process.
    pub fn wait_all(&self) {
        for process in self.processes() {
            let _ = process.wait();
        }
    }
}

/// The global process manager, like `gproc.GetManager`.
pub fn manager() -> &'static Manager {
    static MANAGER: OnceLock<Manager> = OnceLock::new();
    MANAGER.get_or_init(Manager::new)
}

// ---------------------------------------------------------------------------
// Signal handling (ADR-181)
// ---------------------------------------------------------------------------

/// One process signal, like the `os.Signal` values GoFrame's `gproc` listens
/// to (ADR-181).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Signal {
    /// `SIGINT` / Ctrl-C.
    Interrupt,
    /// `SIGQUIT`.
    Quit,
    /// `SIGTERM`.
    Terminate,
    /// `SIGABRT`.
    Abort,
    /// `SIGHUP`.
    Hangup,
    /// `SIGUSR1` (Unix only).
    User1,
    /// `SIGUSR2` (Unix only).
    User2,
    /// `SIGKILL` (uncatchable, kept for parity).
    Kill,
}

impl Signal {
    /// The Go-style signal name.
    pub fn name(self) -> &'static str {
        match self {
            Signal::Interrupt => "interrupt",
            Signal::Quit => "quit",
            Signal::Terminate => "terminated",
            Signal::Abort => "aborted",
            Signal::Hangup => "hangup",
            Signal::User1 => "user defined signal 1",
            Signal::User2 => "user defined signal 2",
            Signal::Kill => "killed",
        }
    }

    #[cfg(unix)]
    fn from_unix(signo: i32) -> Option<Signal> {
        use signal_hook::consts::{
            SIGABRT, SIGHUP, SIGINT, SIGKILL, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2,
        };
        match signo {
            SIGINT => Some(Signal::Interrupt),
            SIGQUIT => Some(Signal::Quit),
            SIGTERM => Some(Signal::Terminate),
            SIGABRT => Some(Signal::Abort),
            SIGHUP => Some(Signal::Hangup),
            SIGUSR1 => Some(Signal::User1),
            SIGUSR2 => Some(Signal::User2),
            SIGKILL => Some(Signal::Kill),
            _ => None,
        }
    }
}

/// A signal handler, like GoFrame's `SigHandler`.
pub type SigHandler = Arc<dyn Fn(Signal) + Send + Sync>;

/// The shutdown signals, like GoFrame's `shutdownSignalMap`.
pub fn shutdown_signals() -> &'static [Signal] {
    &[
        Signal::Interrupt,
        Signal::Quit,
        Signal::Terminate,
        Signal::Abort,
        Signal::Kill,
    ]
}

fn signal_handler_map() -> &'static Mutex<HashMap<Signal, Vec<SigHandler>>> {
    static HANDLERS: OnceLock<Mutex<HashMap<Signal, Vec<SigHandler>>>> = OnceLock::new();
    HANDLERS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Adds `handler` for every signal in `signals`, like `AddSigHandler`.
pub fn add_sig_handler(handler: SigHandler, signals: &[Signal]) {
    let mut handlers = signal_handler_map()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    for signal in signals {
        handlers
            .entry(*signal)
            .or_default()
            .push(Arc::clone(&handler));
    }
}

/// Adds shutdown handlers for every shutdown signal, like
/// `AddSigHandlerShutdown`.
pub fn add_sig_handler_shutdown(handlers: &[SigHandler]) {
    for handler in handlers {
        add_sig_handler(Arc::clone(handler), shutdown_signals());
    }
}

fn is_shutdown_signal(signal: Signal) -> bool {
    shutdown_signals().contains(&signal)
}

fn listening_state() -> &'static (Mutex<bool>, Condvar) {
    static STATE: OnceLock<(Mutex<bool>, Condvar)> = OnceLock::new();
    STATE.get_or_init(|| (Mutex::new(false), Condvar::new()))
}

fn mark_listening_ended() {
    let (lock, condition) = listening_state();
    let mut ended = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    *ended = true;
    condition.notify_all();
}

/// Dispatches `signal` as if it had been received, returning whether the
/// listening loop ended (shutdown signal). Handlers run on their own threads;
/// shutdown handlers are joined (ADR-181).
pub fn trigger(signal: Signal) -> bool {
    let shutdown = is_shutdown_signal(signal);
    let handlers = {
        let map = signal_handler_map()
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        map.get(&signal).cloned().unwrap_or_default()
    };
    if shutdown {
        mark_listening_ended();
    }
    let mut joins = Vec::with_capacity(handlers.len());
    for handler in handlers {
        joins.push(std::thread::spawn(move || handler(signal)));
    }
    if shutdown {
        for join in joins {
            let _ = join.join();
        }
    }
    shutdown
}

/// Starts the OS signal listener once (ADR-181).
fn start_os_listeners() -> Result<(), Error> {
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGABRT, SIGHUP, SIGINT, SIGQUIT, SIGTERM, SIGUSR1, SIGUSR2};
        use signal_hook::iterator::Signals;
        let mut signals =
            Signals::new([SIGINT, SIGQUIT, SIGTERM, SIGABRT, SIGHUP, SIGUSR1, SIGUSR2]).map_err(
                |error| gerror::new(format!("install signal listeners failed: {error}")),
            )?;
        std::thread::Builder::new()
            .name("gf-gproc-signal".to_owned())
            .spawn(move || {
                for signo in signals.forever() {
                    if let Some(signal) = Signal::from_unix(signo)
                        && trigger(signal)
                    {
                        break;
                    }
                }
            })
            .map_err(|error| gerror::new(format!("start signal listener failed: {error}")))?;
        Ok(())
    }
    #[cfg(windows)]
    {
        ctrlc::set_handler(|| {
            trigger(Signal::Interrupt);
        })
        .map_err(|error| gerror::new(format!("install signal handler failed: {error}")))?;
        Ok(())
    }
    #[cfg(all(not(unix), not(windows)))]
    {
        Ok(())
    }
}

/// Blocks until a shutdown signal is handled, like GoFrame's `Listen`.
///
/// The OS listeners are installed on the first call; `trigger` dispatches
/// manually for tests and embedding. Multiple callers share the same
/// listening loop.
pub fn listen() -> Result<(), Error> {
    static STARTED: Once = Once::new();
    static START_RESULT: OnceLock<Result<(), String>> = OnceLock::new();
    STARTED.call_once(|| {
        let result = start_os_listeners().map_err(|error| error.error_string());
        let _ = START_RESULT.set(result);
    });
    if let Some(Err(message)) = START_RESULT.get() {
        return Err(gerror::new(message.clone()));
    }
    let (lock, condition) = listening_state();
    let mut ended = lock.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    while !*ended {
        ended = condition
            .wait(ended)
            .unwrap_or_else(|poisoned| poisoned.into_inner());
    }
    Ok(())
}

/// Executes `command` synchronously with optional stdin, like `Shell`.
///
/// Returns the captured stdout and stderr.
pub fn shell(command: &str, input: Option<&str>) -> Result<(String, String), Error> {
    let (program, option) = shell_program();
    let mut child = Command::new(&program)
        .arg(option)
        .arg(command)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| gerror::new(format!("execute \"{command}\" failed: {error}")))?;
    if let Some(input) = input
        && let Some(mut stdin) = child.stdin.take()
    {
        let _ = stdin.write_all(input.as_bytes());
    }
    let output = child
        .wait_with_output()
        .map_err(|error| gerror::new(format!("wait for \"{command}\" failed: {error}")))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(gerror::new(format!(
            "exit status {}: {}{}",
            output.status.code().unwrap_or(-1),
            stdout,
            stderr
        )));
    }
    Ok((stdout, stderr))
}

/// Executes `command` synchronously and inherits stdout/stderr, like
/// `ShellRun`.
pub fn shell_run(command: &str) -> Result<(), Error> {
    let (program, option) = shell_program();
    let status = Command::new(&program)
        .arg(option)
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| gerror::new(format!("execute \"{command}\" failed: {error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(gerror::new(format!(
            "execute \"{command}\" failed with status {}",
            status.code().unwrap_or(-1)
        )))
    }
}

/// Executes `command` and returns its combined stdout/stderr, like
/// `ShellExec`.
///
/// `environment` entries are appended to the inherited environment.
pub fn shell_exec(command: &str, environment: Option<&[(&str, &str)]>) -> Result<String, Error> {
    let (program, option) = shell_program();
    let mut child = Command::new(&program);
    child.arg(option).arg(command).stdin(Stdio::null());
    if let Some(environment) = environment {
        for (key, value) in environment {
            child.env(key, value);
        }
    }
    let output = child
        .output()
        .map_err(|error| gerror::new(format!("execute \"{command}\" failed: {error}")))?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    if !output.status.success() {
        return Err(gerror::new(format!(
            "exit status {}: {combined}",
            output.status.code().unwrap_or(-1)
        )));
    }
    Ok(combined)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shell_exec_captures_output() {
        let output = shell_exec("echo gproc-hello", None).expect("exec");
        assert!(output.contains("gproc-hello"), "got {output}");
    }

    #[test]
    fn shell_exec_combines_stderr_and_reports_failure() {
        let error = shell_exec("echo gm-fail 1>&2 && exit 3", None).expect_err("exit status");
        let message = error.to_string();
        assert!(message.contains("gm-fail"), "got {message}");
        assert!(message.contains('3'), "got {message}");
    }

    #[test]
    fn shell_with_stdin_pipes_input() {
        if cfg!(windows) {
            let (stdout, _) = shell("findstr gproc", Some("gproc-input\n")).expect("shell");
            assert!(stdout.contains("gproc-input"), "got {stdout}");
        } else {
            let (stdout, _) = shell("grep gproc", Some("gproc-input\n")).expect("shell");
            assert!(stdout.contains("gproc-input"), "got {stdout}");
        }
    }

    #[test]
    fn shell_run_reports_success_and_failure() {
        shell_run("exit 0").expect("success");
        assert!(shell_run("exit 2").is_err());
    }

    #[test]
    fn process_identity_helpers() {
        assert!(pid() > 0);
        assert!(uptime() < Duration::from_secs(60 * 60));
        assert!(!start_time().elapsed().is_err());
    }

    #[test]
    fn custom_parent_pid_round_trips() {
        assert!(!is_child());
        set_ppid(4242).expect("set");
        assert!(is_child());
        assert_eq!(ppid(), 4242);
        set_ppid(0).expect("clear");
        assert!(!is_child());
        assert_eq!(ppid(), pid());
    }

    #[test]
    fn search_binary_finds_the_shell() {
        let binary = if cfg!(windows) { "cmd.exe" } else { "sh" };
        let found = search_binary(binary);
        assert!(!found.is_empty(), "{binary} should be on PATH");
        assert!(search_binary("gf-rust-definitely-missing-binary").is_empty());
    }

    fn exit_command(code: i32) -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            ("cmd", vec!["/c".to_owned(), format!("exit {code}")])
        } else {
            ("sh", vec!["-c".to_owned(), format!("exit {code}")])
        }
    }

    fn sleep_command(seconds: u32) -> (&'static str, Vec<String>) {
        if cfg!(windows) {
            (
                "cmd",
                vec![
                    "/c".to_owned(),
                    format!("ping -n {} 127.0.0.1 > nul", seconds + 1),
                ],
            )
        } else {
            ("sh", vec!["-c".to_owned(), format!("sleep {seconds}")])
        }
    }

    #[test]
    fn manager_tracks_and_waits() {
        let manager = Manager::new();
        let (program, args) = exit_command(7);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let process = manager.start("exit", program, &args).expect("start");
        assert_eq!(process.name(), "exit");
        assert_ne!(process.pid(), 0);
        assert_eq!(manager.processes().len(), 1);
        assert!(manager.get_process("exit").is_some());
        assert_eq!(process.wait().expect("wait"), Some(7));
        assert!(!process.is_running());
        assert!(manager.remove_process("exit").is_some());
        assert!(manager.get_process("exit").is_none());
        assert!(manager.processes().is_empty());
    }

    #[test]
    fn manager_replaces_process_and_kills() {
        let manager = Manager::new();
        let (program, args) = sleep_command(5);
        let args: Vec<&str> = args.iter().map(String::as_str).collect();
        let first = manager.start("sleeper", program, &args).expect("first");
        let second = manager.start("sleeper", program, &args).expect("second");
        assert_ne!(first.pid(), second.pid());
        assert_eq!(manager.processes().len(), 1);
        assert_eq!(
            manager.get_process("sleeper").expect("registered").pid(),
            second.pid()
        );

        second.kill().expect("kill");
        let mut stopped = false;
        for _ in 0..100 {
            if !second.is_running() {
                stopped = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(stopped, "the killed process should stop");

        manager.kill_all();
        manager.wait_all();
        manager.remove_process("sleeper");
    }

    #[test]
    fn signal_handlers_and_listen() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let received: Arc<Mutex<Vec<Signal>>> = Arc::new(Mutex::new(Vec::new()));
        let recorder = {
            let received = Arc::clone(&received);
            Arc::new(move |signal: Signal| {
                received
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .push(signal);
            }) as SigHandler
        };
        add_sig_handler(Arc::clone(&recorder), &[Signal::User1, Signal::User2]);

        assert!(!trigger(Signal::User1), "a user signal is not shutdown");
        assert!(!trigger(Signal::Hangup), "unregistered signals are ignored");
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(
            *received
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()),
            vec![Signal::User1]
        );

        let shutdown_called = Arc::new(AtomicBool::new(false));
        let shutdown = {
            let flag = Arc::clone(&shutdown_called);
            Arc::new(move |_signal: Signal| {
                flag.store(true, Ordering::SeqCst);
            }) as SigHandler
        };
        add_sig_handler_shutdown(&[shutdown]);

        if cfg!(unix) {
            let listener = std::thread::spawn(listen);
            std::thread::sleep(Duration::from_millis(100));
            assert!(
                trigger(Signal::Terminate),
                "a shutdown signal ends listening"
            );
            listener
                .join()
                .expect("listener thread")
                .expect("listen succeeds");
        } else {
            assert!(
                trigger(Signal::Terminate),
                "a shutdown signal ends listening"
            );
        }
        assert!(shutdown_called.load(Ordering::SeqCst));
    }
}
